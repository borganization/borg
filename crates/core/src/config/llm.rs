use serde::{Deserialize, Serialize};

use crate::secrets_resolve::SecretRef;

/// Extended thinking level for supported LLM providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    #[default]
    Off,
    /// 1024 budget tokens (Anthropic) / low effort (OpenAI)
    Low,
    /// 4096 budget tokens (Anthropic) / medium effort (OpenAI)
    Medium,
    /// 16384 budget tokens (Anthropic) / high effort (OpenAI)
    High,
    /// 32768 budget tokens (Anthropic) / high effort (OpenAI)
    Xhigh,
}

impl ThinkingLevel {
    /// Budget tokens for Anthropic's thinking parameter. Returns `None` when thinking is off.
    pub fn budget_tokens(&self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Low => Some(1024),
            Self::Medium => Some(4096),
            Self::High => Some(16384),
            Self::Xhigh => Some(32768),
        }
    }

    /// Reasoning effort string for OpenAI o-series models. Returns `None` when thinking is off.
    pub fn openai_reasoning_effort(&self) -> Option<&str> {
        match self {
            Self::Off => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High | Self::Xhigh => Some("high"),
        }
    }

    pub fn is_enabled(&self) -> bool {
        *self != Self::Off
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub provider: Option<String>,
    pub api_key_env: String,
    /// Single SecretRef for API key (takes priority over api_key_env).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<SecretRef>,
    /// Multiple API keys for fallback/rotation (tried in order on auth failure).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub api_keys: Vec<SecretRef>,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub max_retries: u32,
    pub initial_retry_delay_ms: u64,
    pub request_timeout_ms: u64,
    /// Timeout in seconds for receiving each SSE chunk during streaming. 0 = no timeout.
    pub stream_chunk_timeout_secs: u64,
    /// Override the provider's default API URL (e.g., for Ollama on a remote host, Azure OpenAI, or proxies).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Provider-level failover chain (tried in order when primary provider fails).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<LlmFallback>,
    /// Extended thinking level: off, low, medium, high, xhigh.
    /// Enables native thinking for supported providers (Anthropic, OpenAI o-series).
    #[serde(default)]
    pub thinking: ThinkingLevel,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: None,
            api_key_env: "OPENROUTER_API_KEY".into(),
            api_key: None,
            api_keys: Vec::new(),
            model: "anthropic/claude-sonnet-4".into(),
            temperature: 0.7,
            max_tokens: 4096,
            max_retries: 3,
            initial_retry_delay_ms: 200,
            request_timeout_ms: 60000,
            stream_chunk_timeout_secs: 30,
            base_url: None,
            fallback: Vec::new(),
            thinking: ThinkingLevel::Off,
        }
    }
}

/// A fallback provider configuration for provider-level failover.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmFallback {
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<SecretRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub api_keys: Vec<SecretRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

/// Configuration for the compaction model — use a different (cheaper/faster)
/// model for context compaction than the primary conversation model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CompactionConfig {
    /// Override provider for compaction (e.g., "openrouter", "anthropic").
    /// If omitted, uses the primary LLM provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Override model for compaction (e.g., "anthropic/claude-haiku-4-5").
    /// If omitted, uses the primary LLM model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Override API key env var for compaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Override temperature for compaction (default: primary LLM temperature).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Override max tokens for compaction (default: primary LLM max_tokens).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Timeout for compaction requests in ms (default: primary LLM timeout).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl CompactionConfig {
    /// Returns true if any compaction override is configured.
    pub fn has_overrides(&self) -> bool {
        self.provider.is_some()
            || self.model.is_some()
            || self.api_key_env.is_some()
            || self.temperature.is_some()
            || self.max_tokens.is_some()
            || self.timeout_ms.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HeartbeatConfig {
    pub enabled: bool,
    pub interval: String,
    pub quiet_hours_start: Option<String>,
    pub quiet_hours_end: Option<String>,
    pub cron: Option<String>,
    pub channels: Vec<String>,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: "30m".into(),
            quiet_hours_start: Some("00:00".into()),
            quiet_hours_end: Some("06:00".into()),
            cron: None,
            channels: Vec::new(),
        }
    }
}
