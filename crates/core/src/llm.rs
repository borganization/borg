use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};

use crate::config::Config;
use crate::provider::Provider;
use crate::retry::backoff_delay;
use crate::types::{Message, Role, ToolCall, ToolDefinition};

// ── Error classification ──

/// Why a provider failed — used for cooldown duration calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailoverReason {
    Auth,
    Billing,
    RateLimit,
    Overloaded,
    Timeout,
    Format,
    Unknown,
}

impl fmt::Display for FailoverReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth => write!(f, "auth"),
            Self::Billing => write!(f, "billing"),
            Self::RateLimit => write!(f, "rate_limit"),
            Self::Overloaded => write!(f, "overloaded"),
            Self::Timeout => write!(f, "timeout"),
            Self::Format => write!(f, "format"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug)]
pub enum LlmError {
    Retryable {
        source: anyhow::Error,
        retry_after: Option<Duration>,
        reason: FailoverReason,
    },
    Fatal {
        source: anyhow::Error,
        reason: FailoverReason,
    },
    Interrupted,
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable { source, .. } => write!(f, "{source}"),
            Self::Fatal { source, .. } => write!(f, "{source}"),
            Self::Interrupted => write!(f, "request interrupted"),
        }
    }
}

impl std::error::Error for LlmError {}

impl LlmError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    pub fn reason(&self) -> FailoverReason {
        match self {
            Self::Retryable { reason, .. } | Self::Fatal { reason, .. } => *reason,
            Self::Interrupted => FailoverReason::Unknown,
        }
    }
}

fn classify_status(status: reqwest::StatusCode, body: &str, provider: Provider) -> LlmError {
    let retry_after = parse_retry_after(body);

    match status.as_u16() {
        429 => LlmError::Retryable {
            source: anyhow::anyhow!("{provider} returned 429 (rate limited): {body}"),
            retry_after,
            reason: FailoverReason::RateLimit,
        },
        500 | 502 | 504 => LlmError::Retryable {
            source: anyhow::anyhow!("{provider} returned {status}: {body}"),
            retry_after: None,
            reason: FailoverReason::Overloaded,
        },
        503 => LlmError::Retryable {
            source: anyhow::anyhow!("{provider} returned {status} (overloaded): {body}"),
            retry_after: None,
            reason: FailoverReason::Overloaded,
        },
        401 | 403 => LlmError::Fatal {
            source: anyhow::anyhow!("{provider} returned {status} (auth error): {body}"),
            reason: FailoverReason::Auth,
        },
        402 => LlmError::Fatal {
            source: anyhow::anyhow!("{provider} returned {status} (billing error): {body}"),
            reason: FailoverReason::Billing,
        },
        400 => LlmError::Fatal {
            source: anyhow::anyhow!("{provider} returned {status} (bad request): {body}"),
            reason: FailoverReason::Format,
        },
        _ => LlmError::Fatal {
            source: anyhow::anyhow!("{provider} returned {status}: {body}"),
            reason: FailoverReason::Unknown,
        },
    }
}

fn classify_network_error(err: anyhow::Error) -> LlmError {
    LlmError::Retryable {
        source: err,
        retry_after: None,
        reason: FailoverReason::Timeout,
    }
}

fn parse_retry_after(body: &str) -> Option<Duration> {
    // Try to extract retry_after from JSON error body
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(secs) = v["error"]["retry_after"].as_f64() {
            return Some(Duration::from_secs_f64(secs));
        }
    }
    None
}

#[derive(Debug, Clone, Default)]
pub struct UsageData {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub provider: String,
    pub model: String,
}

#[derive(Debug)]
pub enum StreamEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    Usage(UsageData),
    Done,
    Error(String),
}

// ── OpenAI-compatible request/response types ──

#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    temperature: f32,
    max_tokens: u32,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
    usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<Delta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    content: Option<String>,
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct DeltaToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<DeltaFunction>,
}

#[derive(Debug, Deserialize)]
struct DeltaFunction {
    name: Option<String>,
    arguments: Option<String>,
}

/// Per-provider cooldown state for the circuit breaker pattern.
#[derive(Debug, Default)]
struct ProviderCooldown {
    /// When this provider can be retried.
    cooldown_until: Option<std::time::Instant>,
    /// Consecutive failure count (reset on cooldown expiry).
    error_count: u32,
    /// Why the provider is in cooldown.
    reason: Option<FailoverReason>,
}

impl ProviderCooldown {
    /// Whether the provider is currently in cooldown (cannot be used).
    fn is_active(&self) -> bool {
        match self.cooldown_until {
            Some(until) => std::time::Instant::now() < until,
            None => false,
        }
    }

    /// Record a failure and compute the next cooldown duration.
    fn record_failure(&mut self, reason: FailoverReason) {
        // If cooldown expired, reset counter (half-open circuit breaker).
        if !self.is_active() {
            self.error_count = 0;
        }
        self.error_count += 1;
        self.reason = Some(reason);

        let base_secs = match reason {
            FailoverReason::Auth | FailoverReason::Billing => 300.0,
            _ => 60.0,
        };
        let max_exp = match reason {
            FailoverReason::Auth | FailoverReason::Billing => 3,
            _ => 4,
        };
        let exp = self.error_count.saturating_sub(1).min(max_exp);
        let backoff_secs = base_secs * 2.0_f64.powi(exp as i32);
        self.cooldown_until =
            Some(std::time::Instant::now() + Duration::from_secs_f64(backoff_secs));
    }

    /// Reset cooldown on success.
    fn record_success(&mut self) {
        self.cooldown_until = None;
        self.error_count = 0;
        self.reason = None;
    }
}

/// A provider slot in the failover chain.
struct ProviderSlot {
    provider: Provider,
    model: String,
    keys: Vec<String>,
    temperature: f32,
    max_tokens: u32,
    base_url: Option<String>,
    cooldown: ProviderCooldown,
}

pub struct LlmClient {
    client: Client,
    config: Config,
    provider: Provider,
    api_key: String,
    /// Additional fallback keys (rotated on 401/429 errors).
    fallback_keys: Vec<String>,
    /// Provider-level failover slots.
    provider_slots: Vec<ProviderSlot>,
    /// Index of the currently active provider slot (usize::MAX = primary).
    active_slot_index: usize,
    debug_logging: bool,
}

impl LlmClient {
    /// Effective API URL: config override → provider default.
    fn effective_base_url(&self) -> &str {
        self.config
            .llm
            .base_url
            .as_deref()
            .unwrap_or_else(|| self.provider.base_url())
    }

    pub fn new(config: Config) -> Result<Self> {
        let (provider, mut keys) = config.resolve_api_keys()?;
        let debug_logging = config.debug.llm_logging;
        let client = Client::new();
        let api_key = keys.remove(0);

        // Build provider-level failover slots
        let mut provider_slots = Vec::new();
        for fb in &config.llm.fallback {
            match Self::resolve_fallback_slot(fb, &config) {
                Ok(slot) => provider_slots.push(slot),
                Err(e) => {
                    warn!("Skipping fallback provider '{}': {e}", fb.provider);
                }
            }
        }

        Ok(Self {
            client,
            config,
            provider,
            api_key,
            fallback_keys: keys,
            provider_slots,
            active_slot_index: usize::MAX,
            debug_logging,
        })
    }

    /// Resolve a fallback config into a ProviderSlot.
    fn resolve_fallback_slot(
        fb: &crate::config::LlmFallback,
        config: &Config,
    ) -> Result<ProviderSlot> {
        let provider = Provider::from_str(&fb.provider)?;
        let mut slot_keys = Vec::new();

        // Try api_keys first
        for sr in &fb.api_keys {
            if let Ok(key) = sr.resolve() {
                if !key.is_empty() {
                    slot_keys.push(key);
                }
            }
        }

        // Try api_key SecretRef
        if slot_keys.is_empty() {
            if let Some(ref sr) = fb.api_key {
                if let Ok(key) = sr.resolve() {
                    if !key.is_empty() {
                        slot_keys.push(key);
                    }
                }
            }
        }

        // Try api_key_env
        if slot_keys.is_empty() {
            let env_var = fb
                .api_key_env
                .as_deref()
                .unwrap_or(provider.default_env_var());
            if let Ok(key) = std::env::var(env_var) {
                if !key.is_empty() {
                    slot_keys.push(key);
                }
            }
            // Also try credential store
            if slot_keys.is_empty() {
                if let Some(key) = config.resolve_credential_or_env(env_var) {
                    slot_keys.push(key);
                }
            }
        }

        // Keyless providers (e.g., Ollama) don't need API keys
        if slot_keys.is_empty() && !provider.requires_api_key() {
            slot_keys.push(String::new());
        }

        if slot_keys.is_empty() {
            bail!("No API keys found for fallback provider {provider}");
        }

        Ok(ProviderSlot {
            provider,
            model: fb.model.clone(),
            keys: slot_keys,
            temperature: fb.temperature.unwrap_or(config.llm.temperature),
            max_tokens: fb.max_tokens.unwrap_or(config.llm.max_tokens),
            base_url: fb.base_url.clone(),
            cooldown: ProviderCooldown::default(),
        })
    }

    /// Try to failover to the next available (non-cooled-down) provider slot.
    /// Returns true if a failover occurred.
    fn try_failover_provider(&mut self) -> bool {
        for (i, slot) in self.provider_slots.iter().enumerate() {
            if i == self.active_slot_index {
                continue;
            }
            if slot.cooldown.is_active() {
                debug!(
                    "Provider {} in cooldown ({:?}), skipping",
                    slot.provider, slot.cooldown.reason
                );
                continue;
            }
            // Found a usable slot — switch to it
            self.provider = slot.provider;
            self.api_key = slot.keys[0].clone();
            self.fallback_keys = slot.keys[1..].to_vec();
            self.config.llm.model = slot.model.clone();
            self.config.llm.temperature = slot.temperature;
            self.config.llm.max_tokens = slot.max_tokens;
            self.config.llm.base_url = slot.base_url.clone();
            self.active_slot_index = i;
            info!(
                "Failover to provider {} (model: {})",
                slot.provider, slot.model
            );
            return true;
        }
        false
    }

    /// Record a failure on the currently active provider slot.
    fn record_provider_failure(&mut self, reason: FailoverReason) {
        if self.active_slot_index < self.provider_slots.len() {
            self.provider_slots[self.active_slot_index]
                .cooldown
                .record_failure(reason);
        }
        // If active_slot_index == usize::MAX, it's the primary provider — no cooldown tracking
    }

    /// Record success on the currently active provider slot.
    fn record_provider_success(&mut self) {
        if self.active_slot_index < self.provider_slots.len() {
            self.provider_slots[self.active_slot_index]
                .cooldown
                .record_success();
        }
    }

    fn debug_log(&self, label: &str, content: &str) {
        if !self.debug_logging {
            return;
        }
        let dir = match crate::config::Config::data_dir() {
            Ok(d) => d.join("logs").join("debug"),
            Err(_) => return,
        };
        let _ = std::fs::create_dir_all(&dir);
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S_%3f");
        let path = dir.join(format!("{timestamp}_{label}.json"));
        let redacted = crate::secrets::redact_secrets(content);
        let _ = std::fs::write(&path, redacted);
    }

    pub async fn stream_chat(
        &mut self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<()> {
        self.stream_chat_with_cancel(messages, tools, tx, CancellationToken::new())
            .await
    }

    /// Try rotating to the next fallback API key. Returns true if rotation succeeded.
    /// If `keep_old` is false, the current key is discarded (e.g. on 401/403 auth failure).
    /// If `keep_old` is true, the current key is pushed to the back of the pool (e.g. on 429 rate limit).
    fn try_rotate_key(&mut self, keep_old: bool) -> bool {
        if self.fallback_keys.is_empty() {
            return false;
        }
        let old_key = std::mem::replace(&mut self.api_key, self.fallback_keys.remove(0));
        if keep_old {
            self.fallback_keys.push(old_key);
        } else {
            info!("Discarding revoked/invalid API key");
        }
        info!(
            "Rotated to next API key ({} fallbacks remaining)",
            self.fallback_keys.len()
        );
        true
    }

    /// Stream chat with cancellation support, retry logic, and provider-level failover.
    /// Supports multi-key fallback: on 401/429 errors, rotates to the next available key.
    /// On exhausted retries, attempts failover to the next provider in the fallback chain.
    #[instrument(skip_all, fields(llm.provider = %self.provider, llm.model = %self.config.llm.model))]
    pub async fn stream_chat_with_cancel(
        &mut self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::Sender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<()> {
        // Strip images for non-vision models (safety net)
        let messages = if !self.provider.supports_vision(&self.config.llm.model) {
            strip_images(messages)
        } else {
            messages.to_vec()
        };
        let messages = &messages;

        let max_provider_attempts = 1 + self.provider_slots.len();

        for _provider_attempt in 0..max_provider_attempts {
            let max_retries = self.config.llm.max_retries;
            let initial_delay = Duration::from_millis(self.config.llm.initial_retry_delay_ms);
            let total_keys = 1 + self.fallback_keys.len();
            let mut keys_tried = 0_usize;
            let mut should_failover = false;

            for attempt in 0..=max_retries {
                if cancel.is_cancelled() {
                    let _ = tx.send(StreamEvent::Done).await;
                    return Ok(());
                }

                let result = if self.provider.is_openai_compatible() {
                    self.stream_chat_openai_inner(messages, tools, &tx, &cancel)
                        .await
                } else {
                    self.stream_chat_anthropic_inner(messages, tools, &tx, &cancel)
                        .await
                };

                match result {
                    Ok(()) => {
                        self.record_provider_success();
                        return Ok(());
                    }
                    Err(e) => {
                        // On auth failure or rate limit, try rotating to next key before retrying
                        let is_auth_error = matches!(e.reason(), FailoverReason::Auth);
                        let is_rate_limit = matches!(e.reason(), FailoverReason::RateLimit);

                        if (is_auth_error || is_rate_limit)
                            && keys_tried < total_keys
                            && self.try_rotate_key(!is_auth_error)
                        {
                            keys_tried += 1;
                            info!("Auth/rate-limit error, trying next API key...");
                            continue;
                        }

                        if !e.is_retryable() || attempt == max_retries {
                            // Record failure and attempt provider failover
                            self.record_provider_failure(e.reason());
                            if self.try_failover_provider() {
                                should_failover = true;
                                break;
                            }
                            // No more providers — propagate error
                            let msg = format!("{e}");
                            let _ = tx.send(StreamEvent::Error(msg.clone())).await;
                            bail!("{msg}");
                        }

                        let delay = if let LlmError::Retryable {
                            retry_after: Some(ra),
                            ..
                        } = &e
                        {
                            *ra
                        } else {
                            backoff_delay(attempt, initial_delay, 2.0)
                        };

                        info!(
                            "Retryable error (attempt {}/{}): {e}. Retrying in {}ms...",
                            attempt + 1,
                            max_retries,
                            delay.as_millis()
                        );

                        tokio::select! {
                            _ = cancel.cancelled() => {
                                let _ = tx.send(StreamEvent::Done).await;
                                return Ok(());
                            }
                            _ = tokio::time::sleep(delay) => {}
                        }
                    }
                }
            }

            if !should_failover {
                break;
            }
            // Continue outer loop with new provider
        }

        bail!("All providers exhausted")
    }

    /// Non-streaming call for heartbeat and simple requests
    pub async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<Message> {
        if self.provider.is_openai_compatible() {
            self.chat_openai(messages, tools).await
        } else {
            self.chat_anthropic(messages, tools).await
        }
    }

    // ── OpenAI-compatible path ──

    async fn send_request(
        &self,
        body: &impl Serialize,
        cancel: &CancellationToken,
    ) -> std::result::Result<reqwest::Response, LlmError> {
        let timeout = Duration::from_millis(self.config.llm.request_timeout_ms);

        let fut =
            self.client
                .post(self.effective_base_url())
                .headers(self.provider.build_headers(&self.api_key).map_err(|e| {
                    LlmError::Fatal {
                        source: e,
                        reason: FailoverReason::Unknown,
                    }
                })?)
                .json(body)
                .send();

        let response = tokio::select! {
            _ = cancel.cancelled() => {
                return Err(LlmError::Interrupted);
            }
            result = tokio::time::timeout(timeout, fut) => {
                match result {
                    Ok(Ok(resp)) => resp,
                    Ok(Err(e)) => {
                        return Err(classify_network_error(
                            anyhow::anyhow!("Failed to connect to {}: {e}", self.provider)
                        ));
                    }
                    Err(_) => {
                        return Err(LlmError::Retryable {
                            source: anyhow::anyhow!(
                                "Request to {} timed out after {}ms",
                                self.provider,
                                self.config.llm.request_timeout_ms
                            ),
                            retry_after: None,
                            reason: FailoverReason::Timeout,
                        });
                    }
                }
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(classify_status(status, &body, self.provider));
        }

        Ok(response)
    }

    async fn stream_chat_openai_inner(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        tx: &mpsc::Sender<StreamEvent>,
        cancel: &CancellationToken,
    ) -> std::result::Result<(), LlmError> {
        let model = self.provider.normalize_model(&self.config.llm.model);
        let request = ChatRequest {
            model: model.clone(),
            messages: messages.to_vec(),
            tools: tools.map(<[ToolDefinition]>::to_vec),
            temperature: self.config.llm.temperature,
            max_tokens: self.config.llm.max_tokens,
            stream: true,
        };

        debug!(
            "Sending streaming request to {} (model: {})",
            self.provider, model
        );

        if self.debug_logging {
            if let Ok(json) = serde_json::to_string_pretty(&request) {
                self.debug_log("request_openai", &json);
            }
        }

        let response = self.send_request(&request, cancel).await?;

        const MAX_SSE_BUFFER: usize = 10 * 1024 * 1024; // 10 MB

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = tx.send(StreamEvent::Done).await;
                    return Ok(());
                }
                maybe_chunk = stream.next() => {
                    match maybe_chunk {
                        Some(Ok(c)) => c,
                        Some(Err(e)) => {
                            return Err(LlmError::Retryable {
                                source: anyhow::anyhow!("Stream read error: {e}"),
                                retry_after: None,
                                reason: FailoverReason::Timeout,
                            });
                        }
                        None => {
                            let _ = tx.send(StreamEvent::Done).await;
                            return Ok(());
                        }
                    }
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            if buffer.len() > MAX_SSE_BUFFER {
                return Err(LlmError::Retryable {
                    source: anyhow::anyhow!("SSE buffer exceeded {MAX_SSE_BUFFER} bytes"),
                    retry_after: None,
                    reason: FailoverReason::Overloaded,
                });
            }

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_owned();
                buffer.drain(..line_end + 1);

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    if data.trim() == "[DONE]" {
                        let _ = tx.send(StreamEvent::Done).await;
                        return Ok(());
                    }

                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            // Parse usage data if present
                            if let Some(usage) = chunk.usage {
                                let _ = tx
                                    .send(StreamEvent::Usage(UsageData {
                                        prompt_tokens: usage.prompt_tokens.unwrap_or(0),
                                        completion_tokens: usage.completion_tokens.unwrap_or(0),
                                        total_tokens: usage.total_tokens.unwrap_or(0),
                                        provider: self.provider.as_str().to_string(),
                                        model: self.config.llm.model.clone(),
                                    }))
                                    .await;
                            }
                            if let Some(choices) = chunk.choices {
                                for choice in choices {
                                    if let Some(delta) = choice.delta {
                                        if let Some(content) = delta.content {
                                            let _ = tx.send(StreamEvent::TextDelta(content)).await;
                                        }
                                        if let Some(tool_calls) = delta.tool_calls {
                                            for tc in tool_calls {
                                                let _ = tx
                                                    .send(StreamEvent::ToolCallDelta {
                                                        index: tc.index.unwrap_or(0),
                                                        id: tc.id,
                                                        name: tc
                                                            .function
                                                            .as_ref()
                                                            .and_then(|f| f.name.clone()),
                                                        arguments_delta: tc
                                                            .function
                                                            .as_ref()
                                                            .and_then(|f| f.arguments.clone())
                                                            .unwrap_or_default(),
                                                    })
                                                    .await;
                                            }
                                        }
                                    }
                                    if choice.finish_reason.is_some() {
                                        let _ = tx.send(StreamEvent::Done).await;
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse SSE chunk: {e}");
                        }
                    }
                }
            }
        }
    }

    async fn chat_openai(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<Message> {
        let model = self.provider.normalize_model(&self.config.llm.model);
        let request = ChatRequest {
            model,
            messages: messages.to_vec(),
            tools: tools.map(<[ToolDefinition]>::to_vec),
            temperature: self.config.llm.temperature,
            max_tokens: self.config.llm.max_tokens,
            stream: false,
        };

        let response = self
            .client
            .post(self.effective_base_url())
            .headers(self.provider.build_headers(&self.api_key)?)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("Failed to connect to {}", self.provider))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("{} returned {status}: {body}", self.provider);
        }

        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<ChatChoice>,
        }
        #[derive(Deserialize)]
        struct ChatChoice {
            message: Message,
        }

        let resp: ChatResponse = response.json().await?;
        resp.choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .context("No response from LLM")
    }

    // ── Anthropic path ──

    fn build_anthropic_request(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        stream: bool,
    ) -> serde_json::Value {
        let model = self.provider.normalize_model(&self.config.llm.model);

        // Extract system message
        let system_text: Option<String> = messages
            .iter()
            .find(|m| m.role == Role::System)
            .and_then(|m| m.text_content().map(String::from));

        // Convert messages (skip system)
        let anthropic_messages = build_anthropic_messages(messages);

        // Convert tools to Anthropic format
        let anthropic_tools: Option<Vec<serde_json::Value>> = tools.map(|ts| {
            ts.iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters,
                    })
                })
                .collect()
        });

        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": self.config.llm.max_tokens,
            "temperature": self.config.llm.temperature,
            "stream": stream,
        });

        if let Some(sys) = system_text {
            body["system"] = serde_json::json!(sys);
        }
        body["messages"] = serde_json::json!(anthropic_messages);
        if let Some(tools) = anthropic_tools {
            body["tools"] = serde_json::json!(tools);
        }

        body
    }

    async fn stream_chat_anthropic_inner(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        tx: &mpsc::Sender<StreamEvent>,
        cancel: &CancellationToken,
    ) -> std::result::Result<(), LlmError> {
        let body = self.build_anthropic_request(messages, tools, true);
        let model = self.provider.normalize_model(&self.config.llm.model);

        debug!("Sending streaming request to Anthropic (model: {})", model);

        if self.debug_logging {
            if let Ok(json) = serde_json::to_string_pretty(&body) {
                self.debug_log("request_anthropic", &json);
            }
        }

        let response = self.send_request(&body, cancel).await?;

        const MAX_SSE_BUFFER: usize = 10 * 1024 * 1024; // 10 MB

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut current_tool_index: usize = 0;
        let mut current_block_is_tool = false;
        let mut current_block_is_thinking = false;

        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = tx.send(StreamEvent::Done).await;
                    return Ok(());
                }
                maybe_chunk = stream.next() => {
                    match maybe_chunk {
                        Some(Ok(c)) => c,
                        Some(Err(e)) => {
                            return Err(LlmError::Retryable {
                                source: anyhow::anyhow!("Stream read error: {e}"),
                                retry_after: None,
                                reason: FailoverReason::Timeout,
                            });
                        }
                        None => {
                            let _ = tx.send(StreamEvent::Done).await;
                            return Ok(());
                        }
                    }
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            if buffer.len() > MAX_SSE_BUFFER {
                return Err(LlmError::Retryable {
                    source: anyhow::anyhow!("SSE buffer exceeded {MAX_SSE_BUFFER} bytes"),
                    retry_after: None,
                    reason: FailoverReason::Overloaded,
                });
            }

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_owned();
                buffer.drain(..line_end + 1);

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                // Anthropic SSE uses "event: <type>" followed by "data: <json>"
                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        let event_type = event["type"].as_str().unwrap_or("");

                        match event_type {
                            "content_block_start" => {
                                let block = &event["content_block"];
                                let block_type = block["type"].as_str().unwrap_or("");
                                current_block_is_tool = false;
                                current_block_is_thinking = false;
                                match block_type {
                                    "tool_use" => {
                                        current_block_is_tool = true;
                                        let id = block["id"].as_str().map(String::from);
                                        let name = block["name"].as_str().map(String::from);
                                        let _ = tx
                                            .send(StreamEvent::ToolCallDelta {
                                                index: current_tool_index,
                                                id,
                                                name,
                                                arguments_delta: String::new(),
                                            })
                                            .await;
                                    }
                                    "thinking" => {
                                        current_block_is_thinking = true;
                                    }
                                    _ => {}
                                }
                            }
                            "content_block_delta" => {
                                let delta = &event["delta"];
                                match delta["type"].as_str() {
                                    Some("text_delta") => {
                                        if let Some(text) = delta["text"].as_str() {
                                            let _ = tx
                                                .send(StreamEvent::TextDelta(text.to_string()))
                                                .await;
                                        }
                                    }
                                    Some("thinking_delta") => {
                                        if let Some(text) = delta["thinking"].as_str() {
                                            let _ = tx
                                                .send(StreamEvent::ThinkingDelta(text.to_string()))
                                                .await;
                                        }
                                    }
                                    Some("input_json_delta") => {
                                        if let Some(json_delta) = delta["partial_json"].as_str() {
                                            let _ = tx
                                                .send(StreamEvent::ToolCallDelta {
                                                    index: current_tool_index,
                                                    id: None,
                                                    name: None,
                                                    arguments_delta: json_delta.to_string(),
                                                })
                                                .await;
                                        }
                                    }
                                    _ => {
                                        // For thinking blocks, text comes as text_delta
                                        if current_block_is_thinking {
                                            if let Some(text) = delta["text"].as_str() {
                                                let _ = tx
                                                    .send(StreamEvent::ThinkingDelta(
                                                        text.to_string(),
                                                    ))
                                                    .await;
                                            }
                                        }
                                    }
                                }
                            }
                            "content_block_stop" => {
                                if current_block_is_tool {
                                    current_tool_index += 1;
                                }
                            }
                            "message_stop" => {
                                let _ = tx.send(StreamEvent::Done).await;
                                return Ok(());
                            }
                            "message_delta" => {
                                // Parse usage from message_delta
                                if let Some(usage) = event["usage"].as_object() {
                                    let input = usage
                                        .get("input_tokens")
                                        .and_then(serde_json::Value::as_u64)
                                        .unwrap_or(0);
                                    let output = usage
                                        .get("output_tokens")
                                        .and_then(serde_json::Value::as_u64)
                                        .unwrap_or(0);
                                    let _ = tx
                                        .send(StreamEvent::Usage(UsageData {
                                            prompt_tokens: input,
                                            completion_tokens: output,
                                            total_tokens: input + output,
                                            provider: self.provider.as_str().to_string(),
                                            model: self.config.llm.model.clone(),
                                        }))
                                        .await;
                                }
                                // message_delta with stop_reason indicates end
                                if event["delta"]["stop_reason"].as_str().is_some() {
                                    let _ = tx.send(StreamEvent::Done).await;
                                    return Ok(());
                                }
                            }
                            "error" => {
                                let err_msg = event["error"]["message"]
                                    .as_str()
                                    .unwrap_or("unknown error");
                                return Err(LlmError::Retryable {
                                    source: anyhow::anyhow!("Anthropic stream error: {err_msg}"),
                                    retry_after: None,
                                    reason: FailoverReason::Overloaded,
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    async fn chat_anthropic(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<Message> {
        let body = self.build_anthropic_request(messages, tools, false);

        let response = self
            .client
            .post(self.effective_base_url())
            .headers(self.provider.build_headers(&self.api_key)?)
            .json(&body)
            .send()
            .await
            .context("Failed to connect to Anthropic")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Anthropic returned {status}: {body}");
        }

        let resp: serde_json::Value = response.json().await?;
        parse_anthropic_response(&resp)
    }
}

// ── Anthropic message conversion helpers ──

/// Convert internal Messages to Anthropic format.
fn build_anthropic_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut result: Vec<serde_json::Value> = Vec::new();

    for msg in messages {
        if msg.role == Role::System {
            continue;
        }

        match msg.role {
            Role::User => match &msg.content {
                Some(crate::types::MessageContent::Parts(parts)) => {
                    let mut content_blocks: Vec<serde_json::Value> = Vec::new();
                    for part in parts {
                        match part {
                            crate::types::ContentPart::Text(t) => {
                                content_blocks.push(serde_json::json!({
                                    "type": "text",
                                    "text": t,
                                }));
                            }
                            crate::types::ContentPart::ImageBase64 { media } => {
                                content_blocks.push(serde_json::json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": media.mime_type,
                                        "data": media.data,
                                    },
                                }));
                            }
                            crate::types::ContentPart::ImageUrl { url } => {
                                content_blocks.push(serde_json::json!({
                                    "type": "image",
                                    "source": {
                                        "type": "url",
                                        "url": url,
                                    },
                                }));
                            }
                            crate::types::ContentPart::AudioBase64 { media } => {
                                content_blocks.push(serde_json::json!({
                                        "type": "text",
                                        "text": format!("[audio: {}]", media.filename.as_deref().unwrap_or("audio")),
                                    }));
                            }
                        }
                    }
                    result.push(serde_json::json!({
                        "role": "user",
                        "content": content_blocks,
                    }));
                }
                _ => {
                    result.push(serde_json::json!({
                        "role": "user",
                        "content": msg.text_content().unwrap_or(""),
                    }));
                }
            },
            Role::Assistant => {
                let mut content_blocks: Vec<serde_json::Value> = Vec::new();

                if let Some(text) = msg.text_content() {
                    if !text.is_empty() {
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                }

                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(serde_json::json!({}));
                        content_blocks.push(serde_json::json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.function.name,
                            "input": args,
                        }));
                    }
                }

                if content_blocks.is_empty() {
                    content_blocks.push(serde_json::json!({
                        "type": "text",
                        "text": "",
                    }));
                }

                result.push(serde_json::json!({
                    "role": "assistant",
                    "content": content_blocks,
                }));
            }
            Role::Tool => {
                let tool_result_block = serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": msg.tool_call_id.as_deref().unwrap_or(""),
                    "content": msg.text_content().unwrap_or(""),
                });

                // Anthropic requires tool results in a user message. Merge with previous
                // user message if the last message is already a user message with tool results.
                if let Some(last) = result.last_mut() {
                    if last["role"].as_str() == Some("user") {
                        if let Some(arr) = last["content"].as_array_mut() {
                            arr.push(tool_result_block);
                            continue;
                        }
                    }
                }

                result.push(serde_json::json!({
                    "role": "user",
                    "content": [tool_result_block],
                }));
            }
            Role::System => unreachable!(),
        }
    }

    result
}

/// Parse Anthropic non-streaming response into internal Message.
fn parse_anthropic_response(resp: &serde_json::Value) -> Result<Message> {
    let content_blocks = resp["content"]
        .as_array()
        .with_context(|| format!("Missing 'content' array in Anthropic response: {resp}"))?;

    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for block in content_blocks {
        match block["type"].as_str() {
            Some("text") => {
                if let Some(text) = block["text"].as_str() {
                    text_parts.push(text.to_string());
                }
            }
            Some("tool_use") => {
                let id = block["id"].as_str().unwrap_or("").to_string();
                let name = block["name"].as_str().unwrap_or("").to_string();
                let input = block["input"].clone();
                tool_calls.push(ToolCall {
                    id,
                    call_type: "function".to_string(),
                    function: crate::types::FunctionCall {
                        name,
                        arguments: serde_json::to_string(&input).unwrap_or_default(),
                    },
                });
            }
            _ => {}
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(crate::types::MessageContent::Text(text_parts.join("")))
    };

    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    Ok(Message {
        role: Role::Assistant,
        content,
        tool_calls,
        tool_call_id: None,
        timestamp: Some(chrono::Local::now().to_rfc3339()),
    })
}

/// Strip image content parts from messages when model doesn't support vision.
/// Simplifies `Parts([Text])` → `Text` after stripping.
fn strip_images(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .map(|msg| {
            let content = match &msg.content {
                Some(crate::types::MessageContent::Parts(parts)) => {
                    let filtered: Vec<crate::types::ContentPart> = parts
                        .iter()
                        .filter(|p| {
                            !matches!(
                                p,
                                crate::types::ContentPart::ImageBase64 { .. }
                                    | crate::types::ContentPart::ImageUrl { .. }
                            )
                        })
                        .cloned()
                        .collect();

                    if filtered.is_empty() {
                        Some(crate::types::MessageContent::Text(String::new()))
                    } else if filtered.len() == 1 {
                        if let crate::types::ContentPart::Text(t) = &filtered[0] {
                            Some(crate::types::MessageContent::Text(t.clone()))
                        } else {
                            Some(crate::types::MessageContent::Parts(filtered))
                        }
                    } else {
                        Some(crate::types::MessageContent::Parts(filtered))
                    }
                }
                other => other.clone(),
            };

            Message {
                role: msg.role.clone(),
                content,
                tool_calls: msg.tool_calls.clone(),
                tool_call_id: msg.tool_call_id.clone(),
                timestamp: msg.timestamp.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, ToolCall, ToolDefinition};

    /// Helper to create an LlmClient with Anthropic provider for testing.
    /// Uses a unique env var to avoid races with parallel tests.
    fn make_test_anthropic_client() -> LlmClient {
        let env_var = "BORG_TEST_ANTHROPIC_LLM";
        std::env::set_var(env_var, "sk-test");
        let mut config = Config::default();
        config.llm.provider = Some("anthropic".to_string());
        config.llm.api_key_env = env_var.to_string();
        config.llm.model = "claude-sonnet-4".to_string();
        LlmClient::new(config).unwrap()
    }

    #[test]
    fn anthropic_system_extraction() {
        let messages = vec![Message::system("You are helpful."), Message::user("Hello")];

        let client = make_test_anthropic_client();
        let body = client.build_anthropic_request(&messages, None, false);

        assert_eq!(body["system"].as_str(), Some("You are helpful."));

        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn anthropic_tool_result_conversion() {
        let messages = vec![
            Message::user("test"),
            Message {
                role: Role::Assistant,
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: "read_memory".to_string(),
                        arguments: r#"{"filename":"test.md"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
                timestamp: None,
            },
            Message::tool_result("call_1", "file contents here"),
        ];

        let anthropic_msgs = build_anthropic_messages(&messages);

        // Should have: user, assistant, user (with tool_result)
        assert_eq!(anthropic_msgs.len(), 3);

        // Assistant message should have tool_use block
        let assistant = &anthropic_msgs[1];
        let content = assistant["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_use");
        assert_eq!(content[0]["name"], "read_memory");

        // Tool result should be in a user message
        let tool_user = &anthropic_msgs[2];
        assert_eq!(tool_user["role"], "user");
        let tool_content = tool_user["content"].as_array().unwrap();
        assert_eq!(tool_content[0]["type"], "tool_result");
        assert_eq!(tool_content[0]["tool_use_id"], "call_1");
    }

    #[test]
    fn anthropic_tool_definition_conversion() {
        let tools = vec![ToolDefinition::new(
            "test_tool",
            "A test tool",
            serde_json::json!({"type": "object", "properties": {"x": {"type": "string"}}}),
        )];

        let client = make_test_anthropic_client();

        let body = client.build_anthropic_request(&[Message::user("hi")], Some(&tools), false);

        let api_tools = body["tools"].as_array().unwrap();
        assert_eq!(api_tools.len(), 1);
        assert_eq!(api_tools[0]["name"], "test_tool");
        assert_eq!(api_tools[0]["description"], "A test tool");
        assert!(api_tools[0]["input_schema"].is_object());
        // Should NOT have "function" wrapping or "type":"function"
        assert!(api_tools[0]["function"].is_null());
    }

    #[test]
    fn anthropic_adjacent_tool_results_merge() {
        let messages = vec![
            Message::user("test"),
            Message {
                role: Role::Assistant,
                content: None,
                tool_calls: Some(vec![
                    ToolCall {
                        id: "call_1".to_string(),
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: "tool_a".to_string(),
                            arguments: "{}".to_string(),
                        },
                    },
                    ToolCall {
                        id: "call_2".to_string(),
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: "tool_b".to_string(),
                            arguments: "{}".to_string(),
                        },
                    },
                ]),
                tool_call_id: None,
                timestamp: None,
            },
            Message::tool_result("call_1", "result a"),
            Message::tool_result("call_2", "result b"),
        ];

        let anthropic_msgs = build_anthropic_messages(&messages);

        // user, assistant, user (merged tool results)
        assert_eq!(anthropic_msgs.len(), 3);

        let tool_user = &anthropic_msgs[2];
        let tool_content = tool_user["content"].as_array().unwrap();
        assert_eq!(tool_content.len(), 2);
        assert_eq!(tool_content[0]["tool_use_id"], "call_1");
        assert_eq!(tool_content[1]["tool_use_id"], "call_2");
    }

    #[test]
    fn parse_anthropic_response_text_only() {
        let resp = serde_json::json!({
            "content": [
                {"type": "text", "text": "Hello there!"}
            ],
            "stop_reason": "end_turn"
        });

        let msg = parse_anthropic_response(&resp).unwrap();
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.text_content(), Some("Hello there!"));
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn parse_anthropic_response_with_tool_use() {
        let resp = serde_json::json!({
            "content": [
                {"type": "text", "text": "Let me check."},
                {
                    "type": "tool_use",
                    "id": "toolu_123",
                    "name": "read_memory",
                    "input": {"filename": "test.md"}
                }
            ],
            "stop_reason": "tool_use"
        });

        let msg = parse_anthropic_response(&resp).unwrap();
        assert_eq!(msg.text_content(), Some("Let me check."));
        let tcs = msg.tool_calls.unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "toolu_123");
        assert_eq!(tcs[0].function.name, "read_memory");
        assert!(tcs[0].function.arguments.contains("test.md"));
    }

    #[test]
    fn classify_status_429_is_retryable() {
        let err = classify_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "rate limited",
            Provider::OpenRouter,
        );
        assert!(err.is_retryable());
    }

    #[test]
    fn classify_status_500_is_retryable() {
        let err = classify_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "server error",
            Provider::OpenRouter,
        );
        assert!(err.is_retryable());
    }

    #[test]
    fn classify_status_401_is_fatal() {
        let err = classify_status(
            reqwest::StatusCode::UNAUTHORIZED,
            "bad key",
            Provider::OpenRouter,
        );
        assert!(!err.is_retryable());
    }

    #[test]
    fn classify_status_400_is_fatal() {
        let err = classify_status(
            reqwest::StatusCode::BAD_REQUEST,
            "bad request",
            Provider::OpenRouter,
        );
        assert!(!err.is_retryable());
    }

    // ── parse_retry_after tests ──

    #[test]
    fn parse_retry_after_valid_json() {
        let body = r#"{"error":{"retry_after":2.5}}"#;
        let result = parse_retry_after(body);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), Duration::from_secs_f64(2.5));
    }

    #[test]
    fn parse_retry_after_missing_field() {
        let body = r#"{"error":{"message":"rate limited"}}"#;
        assert!(parse_retry_after(body).is_none());
    }

    #[test]
    fn parse_retry_after_non_json() {
        assert!(parse_retry_after("not json at all").is_none());
    }

    // ── classify_network_error tests ──

    #[test]
    fn classify_network_error_is_retryable() {
        let err = classify_network_error(anyhow::anyhow!("connection reset"));
        assert!(err.is_retryable());
        if let LlmError::Retryable {
            retry_after,
            reason,
            ..
        } = &err
        {
            assert!(retry_after.is_none());
            assert_eq!(*reason, FailoverReason::Timeout);
        } else {
            panic!("expected Retryable");
        }
    }

    // ── LlmError display tests ──

    #[test]
    fn llm_error_retryable_display() {
        let err = LlmError::Retryable {
            source: anyhow::anyhow!("timeout"),
            retry_after: None,
            reason: FailoverReason::Timeout,
        };
        assert!(err.is_retryable());
        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn llm_error_fatal_display() {
        let err = LlmError::Fatal {
            source: anyhow::anyhow!("bad key"),
            reason: FailoverReason::Auth,
        };
        assert!(!err.is_retryable());
        assert!(err.to_string().contains("bad key"));
    }

    #[test]
    fn llm_error_interrupted_display() {
        let err = LlmError::Interrupted;
        assert!(!err.is_retryable());
        assert_eq!(err.to_string(), "request interrupted");
    }

    // ── FailoverReason classification tests ──

    #[test]
    fn failover_reason_from_status_codes() {
        let err_401 = classify_status(
            reqwest::StatusCode::UNAUTHORIZED,
            "bad key",
            Provider::OpenAi,
        );
        assert_eq!(err_401.reason(), FailoverReason::Auth);

        let err_402 = classify_status(
            reqwest::StatusCode::PAYMENT_REQUIRED,
            "no credits",
            Provider::OpenAi,
        );
        assert_eq!(err_402.reason(), FailoverReason::Billing);

        let err_429 = classify_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "rate limited",
            Provider::OpenAi,
        );
        assert_eq!(err_429.reason(), FailoverReason::RateLimit);

        let err_503 = classify_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            Provider::OpenAi,
        );
        assert_eq!(err_503.reason(), FailoverReason::Overloaded);

        let err_400 = classify_status(
            reqwest::StatusCode::BAD_REQUEST,
            "bad format",
            Provider::OpenAi,
        );
        assert_eq!(err_400.reason(), FailoverReason::Format);
    }

    // ── ProviderCooldown tests ──

    #[test]
    fn cooldown_initially_inactive() {
        let cd = ProviderCooldown::default();
        assert!(!cd.is_active());
        assert_eq!(cd.error_count, 0);
    }

    #[test]
    fn cooldown_active_after_failure() {
        let mut cd = ProviderCooldown::default();
        cd.record_failure(FailoverReason::RateLimit);
        assert!(cd.is_active());
        assert_eq!(cd.error_count, 1);
        assert_eq!(cd.reason, Some(FailoverReason::RateLimit));
    }

    #[test]
    fn cooldown_reset_on_success() {
        let mut cd = ProviderCooldown::default();
        cd.record_failure(FailoverReason::Overloaded);
        assert!(cd.is_active());

        cd.record_success();
        assert!(!cd.is_active());
        assert_eq!(cd.error_count, 0);
        assert!(cd.reason.is_none());
    }

    #[test]
    fn cooldown_backoff_escalates() {
        let mut cd = ProviderCooldown::default();
        cd.record_failure(FailoverReason::RateLimit);
        let first = cd.cooldown_until.unwrap();

        cd.cooldown_until = None; // simulate expiry
        cd.error_count = 0; // reset will happen in record_failure
        cd.record_failure(FailoverReason::RateLimit);
        let second = cd.cooldown_until.unwrap();

        // Both cooldowns should be in the future, second may be different
        // since error_count resets on expiry
        assert!(first > std::time::Instant::now() - Duration::from_secs(120));
        assert!(second > std::time::Instant::now() - Duration::from_secs(120));
    }

    #[test]
    fn cooldown_auth_has_longer_base() {
        let mut cd1 = ProviderCooldown::default();
        cd1.record_failure(FailoverReason::RateLimit);
        let rl_until = cd1.cooldown_until.unwrap();

        let mut cd2 = ProviderCooldown::default();
        cd2.record_failure(FailoverReason::Auth);
        let auth_until = cd2.cooldown_until.unwrap();

        // Auth base is 300s, RateLimit base is 60s — auth should be further out
        assert!(auth_until > rl_until);
    }

    #[test]
    fn failover_reason_display() {
        assert_eq!(FailoverReason::Auth.to_string(), "auth");
        assert_eq!(FailoverReason::Billing.to_string(), "billing");
        assert_eq!(FailoverReason::RateLimit.to_string(), "rate_limit");
        assert_eq!(FailoverReason::Overloaded.to_string(), "overloaded");
        assert_eq!(FailoverReason::Timeout.to_string(), "timeout");
        assert_eq!(FailoverReason::Format.to_string(), "format");
        assert_eq!(FailoverReason::Unknown.to_string(), "unknown");
    }

    #[test]
    fn llm_client_new_ollama_no_panic() {
        let mut config = Config::default();
        config.llm.provider = Some("ollama".to_string());
        config.llm.model = "llama3.3".to_string();
        let client = LlmClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn effective_base_url_uses_config_override() {
        let mut config = Config::default();
        config.llm.provider = Some("ollama".to_string());
        config.llm.model = "llama3.3".to_string();
        config.llm.base_url = Some("http://custom:8080/v1/chat/completions".to_string());
        let client = LlmClient::new(config).expect("should create client");
        assert_eq!(
            client.effective_base_url(),
            "http://custom:8080/v1/chat/completions"
        );
    }

    #[test]
    fn effective_base_url_falls_back_to_provider_default() {
        let mut config = Config::default();
        config.llm.provider = Some("ollama".to_string());
        config.llm.model = "llama3.3".to_string();
        let client = LlmClient::new(config).expect("should create client");
        assert_eq!(
            client.effective_base_url(),
            "http://localhost:11434/v1/chat/completions"
        );
    }
}
