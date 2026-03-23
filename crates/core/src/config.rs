use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use std::str::FromStr;

use crate::constants;
use crate::policy::ExecutionPolicy;
use crate::provider::Provider;
use crate::secrets_resolve::SecretRef;

/// A credential value that can be either a plain env var name (legacy) or a full SecretRef.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CredentialValue {
    /// Legacy: bare string = env var name (backward compat)
    EnvVar(String),
    /// Full SecretRef (env, file, exec)
    Ref(SecretRef),
}

impl CredentialValue {
    pub fn resolve(&self) -> Result<String> {
        match self {
            CredentialValue::EnvVar(var) => {
                std::env::var(var).with_context(|| format!("Env var {var} not set"))
            }
            CredentialValue::Ref(sr) => sr.resolve(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub sandbox: SandboxConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub conversation: ConversationConfig,
    #[serde(default)]
    pub user: UserConfig,
    #[serde(default)]
    pub policy: ExecutionPolicy,
    #[serde(default)]
    pub debug: DebugConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub tasks: TasksConfig,
    #[serde(default)]
    pub budget: BudgetConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default, alias = "customizations")]
    pub plugins: PluginsConfig,
    #[serde(default)]
    pub agents: MultiAgentConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub browser: BrowserConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub media: MediaConfig,
    #[serde(default)]
    pub credentials: HashMap<String, CredentialValue>,
    /// Transient identity override (not serialized). Set by gateway routing.
    #[serde(skip)]
    pub identity_override: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    pub enabled: bool,
    pub auto_verify: bool,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_verify: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiAgentConfig {
    pub enabled: bool,
    pub max_spawn_depth: u32,
    pub max_children_per_agent: u32,
    pub max_concurrent: u32,
}

impl Default for MultiAgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_spawn_depth: 1,
            max_children_per_agent: 5,
            max_concurrent: 3,
        }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub default_timeout_ms: u64,
    #[serde(default)]
    pub policy: ToolPolicyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolPolicyConfig {
    /// Tool profile: minimal, coding, messaging, full (default: full)
    pub profile: String,
    /// Explicit allow list (supports `group:xxx` references). Empty = all allowed.
    pub allow: Vec<String>,
    /// Explicit deny list (supports `group:xxx` references). Deny always wins.
    pub deny: Vec<String>,
    /// Tools denied to subagents.
    pub subagent_deny: Vec<String>,
}

impl Default for ToolPolicyConfig {
    fn default() -> Self {
        Self {
            profile: "full".to_string(),
            allow: Vec::new(),
            deny: Vec::new(),
            subagent_deny: vec![
                "manage_tasks".to_string(),
                "security_audit".to_string(),
                "browser".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub max_context_tokens: usize,
    #[serde(default)]
    pub embeddings: EmbeddingsConfig,
    /// When set, load memory from ~/.borg/memory/scopes/{scope}/ instead of ~/.borg/memory/.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_scope: Option<String>,
    /// When true, extract durable information from messages about to be dropped before compaction.
    pub flush_before_compaction: bool,
    /// Minimum token count of dropped messages to trigger a pre-compaction flush.
    pub flush_soft_threshold_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingsConfig {
    pub enabled: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub dimension: Option<usize>,
    pub api_key_env: Option<String>,
    pub recency_weight: f32,
    /// Target token size per chunk when generating chunked embeddings.
    pub chunk_size_tokens: usize,
    /// Overlap tokens between adjacent chunks.
    pub chunk_overlap_tokens: usize,
    /// Weight for BM25/FTS scores in hybrid search (default 0.3).
    pub bm25_weight: f32,
    /// Weight for vector similarity scores in hybrid search (default 0.7).
    pub vector_weight: f32,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl Default for SkillEntryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            env: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsConfig {
    pub enabled: bool,
    pub max_context_tokens: usize,
    #[serde(default)]
    pub entries: HashMap<String, SkillEntryConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversationConfig {
    pub max_history_tokens: usize,
    pub max_iterations: u32,
    pub show_thinking: bool,
    pub tool_output_max_tokens: usize,
    pub compaction_marker_tokens: usize,
    pub max_transcript_chars: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    pub enabled: bool,
    pub search_provider: String,
    pub search_api_key_env: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TasksConfig {
    pub max_concurrent: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetConfig {
    /// Monthly token limit. 0 = unlimited.
    pub monthly_token_limit: u64,
    /// Fraction of budget at which to warn (0.0–1.0).
    pub warning_threshold: f64,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            monthly_token_limit: 1_000_000,
            warning_threshold: 0.8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    pub tracing_enabled: bool,
    pub metrics_enabled: bool,
    pub otlp_endpoint: String,
    pub service_name: String,
    pub sampling_ratio: f64,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            tracing_enabled: false,
            metrics_enabled: false,
            otlp_endpoint: "http://localhost:4317".to_string(),
            service_name: "borg".to_string(),
            sampling_ratio: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    pub enabled: bool,
    pub headless: bool,
    pub executable: Option<String>,
    pub cdp_port: u16,
    pub no_sandbox: bool,
    pub timeout_ms: u64,
    pub startup_timeout_ms: u64,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            headless: true,
            executable: None,
            cdp_port: 9222,
            no_sandbox: false,
            timeout_ms: 30000,
            startup_timeout_ms: 15000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
    pub max_concurrent: usize,
    pub request_timeout_ms: u64,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_minute: u32,
    #[serde(default)]
    pub public_url: Option<String>,
    pub max_body_size: usize,
    pub telegram_poll_timeout_secs: u64,
    pub telegram_circuit_failure_threshold: u32,
    pub telegram_circuit_suspension_secs: u64,
    pub telegram_dedup_capacity: usize,
    /// Per-channel agent routing bindings (first match wins).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<GatewayBinding>,
    /// Optional allowlist of Slack channel IDs. Empty = allow all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slack_channel_allowlist: Option<Vec<String>>,
    /// Optional allowlist of Discord guild IDs. Empty = allow all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord_guild_allowlist: Option<Vec<String>>,
    /// Default access policy for direct messages: pairing, open, disabled.
    #[serde(default)]
    pub dm_policy: crate::pairing::DmPolicy,
    /// Per-channel DM policy overrides. Key = channel name.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub channel_policies: std::collections::HashMap<String, crate::pairing::DmPolicy>,
    /// Pairing code TTL in seconds (default 3600 = 60 minutes).
    #[serde(default = "default_pairing_ttl")]
    pub pairing_ttl_secs: i64,
    /// signal-cli daemon host (default "localhost").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_cli_host: Option<String>,
    /// signal-cli daemon port (default 8080).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_cli_port: Option<u16>,
    /// Default activation mode for group chats (default: Mention).
    /// DMs always activate regardless of this setting.
    #[serde(default)]
    pub group_activation: ActivationMode,
}

/// Activation mode for group chats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActivationMode {
    /// Bot responds to every message in groups.
    Always,
    /// Bot only responds when @mentioned in groups.
    #[default]
    Mention,
}

/// Route a gateway channel to specific agent configuration overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayBinding {
    /// Channel name or glob pattern.
    pub channel: String,
    /// Optional sender ID filter (glob pattern).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    /// Optional peer kind filter: "direct" or "group".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_kind: Option<String>,
    /// Override LLM provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Override LLM model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Override API key env var.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Override temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Override max tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Custom identity file (relative to ~/.borg/).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Scoped memory directory name (~/.borg/memory/scopes/{name}/).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_scope: Option<String>,
    /// Per-binding fallback providers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<LlmFallback>,
    /// Activation mode override for this binding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation: Option<ActivationMode>,
    /// Extended thinking level override for this binding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingLevel>,
}

fn default_rate_limit() -> u32 {
    60
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 7842,
            max_concurrent: 10,
            request_timeout_ms: 120_000,
            rate_limit_per_minute: default_rate_limit(),
            public_url: None,
            max_body_size: constants::GATEWAY_MAX_BODY_SIZE,
            telegram_poll_timeout_secs: constants::TELEGRAM_POLL_TIMEOUT_SECS,
            telegram_circuit_failure_threshold: constants::TELEGRAM_CIRCUIT_FAILURE_THRESHOLD,
            telegram_circuit_suspension_secs: constants::TELEGRAM_CIRCUIT_SUSPENSION_SECS,
            telegram_dedup_capacity: constants::TELEGRAM_DEDUP_CAPACITY,
            bindings: Vec::new(),
            slack_channel_allowlist: None,
            discord_guild_allowlist: None,
            dm_policy: crate::pairing::DmPolicy::default(),
            channel_policies: std::collections::HashMap::new(),
            pairing_ttl_secs: default_pairing_ttl(),
            signal_cli_host: None,
            signal_cli_port: None,
            group_activation: ActivationMode::default(),
        }
    }
}

fn default_pairing_ttl() -> i64 {
    3600
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugConfig {
    /// When true, log full LLM request/response to ~/.borg/logs/debug/
    #[serde(default)]
    pub llm_logging: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub secret_detection: bool,
    pub blocked_paths: Vec<String>,
    pub host_audit: bool,
    #[serde(default = "default_hitl_dangerous_ops")]
    pub hitl_dangerous_ops: bool,
    #[serde(default)]
    pub action_limits: crate::rate_guard::ActionLimits,
    #[serde(default = "default_gateway_action_limits")]
    pub gateway_action_limits: crate::rate_guard::ActionLimits,
}

fn default_hitl_dangerous_ops() -> bool {
    false
}

fn default_gateway_action_limits() -> crate::rate_guard::ActionLimits {
    crate::rate_guard::ActionLimits::gateway_defaults()
}

/// Single transcription provider entry (cloud API or local CLI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioModelConfig {
    /// Provider name: "openai", "groq", "deepgram".
    pub provider: String,
    /// Model name (e.g. "whisper-1", "nova-3").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Override API key env var for this provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Language hint (BCP-47, e.g. "en").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Per-provider timeout override in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// Audio transcription configuration with multi-provider fallback.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub enabled: bool,
    /// Ordered fallback chain of transcription providers.
    #[serde(default)]
    pub models: Vec<AudioModelConfig>,
    /// Maximum audio file size in bytes (default: 20 MB).
    pub max_file_size: u64,
    /// Minimum audio file size in bytes (default: 1024).
    pub min_file_size: u64,
    /// Global language hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Default timeout in milliseconds.
    pub timeout_ms: u64,
    /// Echo transcript back to the sender.
    pub echo_transcript: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            models: Vec::new(),
            max_file_size: 20 * 1024 * 1024,
            min_file_size: 1024,
            language: None,
            timeout_ms: 60_000,
            echo_transcript: false,
        }
    }
}

/// Image compression and media processing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaConfig {
    /// Max image size in bytes after compression. Default: 6MB (matches OpenClaw).
    pub max_image_bytes: usize,
    /// Enable/disable image compression. Default: true.
    pub compression_enabled: bool,
    /// Max image dimension in pixels (longest side). Default: 2048.
    pub max_dimension_px: u32,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            max_image_bytes: 6 * 1024 * 1024,
            compression_enabled: true,
            max_dimension_px: 2048,
        }
    }
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
            base_url: None,
            fallback: Vec::new(),
            thinking: ThinkingLevel::Off,
        }
    }
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

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 30000,
            policy: ToolPolicyConfig::default(),
        }
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "strict".into(),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 8000,
            embeddings: EmbeddingsConfig::default(),
            memory_scope: None,
            flush_before_compaction: false,
            flush_soft_threshold_tokens: 2000,
        }
    }
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: None,
            model: None,
            dimension: None,
            api_key_env: None,
            recency_weight: 0.2,
            chunk_size_tokens: 400,
            chunk_overlap_tokens: 80,
            bm25_weight: 0.3,
            vector_weight: 0.7,
        }
    }
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_context_tokens: 4000,
            entries: HashMap::new(),
        }
    }
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            max_history_tokens: 32000,
            max_iterations: 25,
            show_thinking: true,
            tool_output_max_tokens: constants::TOOL_OUTPUT_MAX_TOKENS,
            compaction_marker_tokens: constants::COMPACTION_MARKER_TOKENS,
            max_transcript_chars: constants::MAX_TRANSCRIPT_CHARS,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            secret_detection: true,
            blocked_paths: vec![
                ".ssh".into(),
                ".aws".into(),
                ".gnupg".into(),
                ".config/gh".into(),
                ".env".into(),
                "credentials".into(),
                "private_key".into(),
            ],
            host_audit: true,
            hitl_dangerous_ops: false,
            action_limits: crate::rate_guard::ActionLimits::default(),
            gateway_action_limits: crate::rate_guard::ActionLimits::gateway_defaults(),
        }
    }
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            search_provider: "duckduckgo".into(),
            search_api_key_env: None,
        }
    }
}

impl Default for TasksConfig {
    fn default() -> Self {
        Self { max_concurrent: 3 }
    }
}

impl Config {
    pub fn data_dir() -> Result<PathBuf> {
        if let Ok(dir) = std::env::var("BORG_DATA_DIR") {
            return Ok(PathBuf::from(dir));
        }
        Ok(dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".borg"))
    }

    pub fn memory_dir() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("memory"))
    }

    /// Resolve the user's configured timezone, falling back to UTC.
    pub fn user_timezone(&self) -> chrono_tz::Tz {
        self.user
            .timezone
            .as_deref()
            .and_then(|s| s.parse::<chrono_tz::Tz>().ok())
            .unwrap_or(chrono_tz::Tz::UTC)
    }

    pub fn skills_dir() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("skills"))
    }

    pub fn tools_dir() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("tools"))
    }

    pub fn channels_dir() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("channels"))
    }

    pub fn logs_dir() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("logs"))
    }

    pub fn sessions_dir() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("sessions"))
    }

    pub fn db_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("borg.db"))
    }

    pub fn identity_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("IDENTITY.md"))
    }

    pub fn memory_index_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("MEMORY.md"))
    }

    pub fn load() -> Result<Self> {
        let config_path = Self::data_dir()?.join("config.toml");
        Self::load_from(&config_path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let content = Self::dedup_toml_tables(&content);
        let config: Config =
            toml::from_str(&content).with_context(|| "Failed to parse config.toml")?;
        Ok(config)
    }

    /// Remove duplicate TOML table headers that would cause parse errors.
    /// Keeps the first occurrence of each `[table]` header and drops subsequent
    /// duplicates along with their content until the next section header.
    fn dedup_toml_tables(input: &str) -> String {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        let mut output = String::with_capacity(input.len());
        let mut skip = false;

        for line in input.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
                if seen.contains(trimmed) {
                    skip = true;
                    continue;
                }
                seen.insert(trimmed.to_string());
                skip = false;
            } else if skip {
                continue;
            }

            output.push_str(line);
            output.push('\n');
        }
        output
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::data_dir()?.join("config.toml");
        let content = toml::to_string_pretty(self).with_context(|| "Failed to serialize config")?;
        std::fs::write(&config_path, content)
            .with_context(|| format!("Failed to write config to {}", config_path.display()))?;
        Ok(())
    }

    pub fn display_settings(&self) -> String {
        let provider = self.llm.provider.as_deref().unwrap_or("(auto-detect)");
        format!(
            "Settings:\n  \
             provider       = {provider}\n  \
             model          = {}\n  \
             temperature    = {}\n  \
             max_tokens     = {}\n  \
             sandbox.enabled = {}\n  \
             sandbox.mode   = {}\n  \
             memory.max_context_tokens = {}\n  \
             skills.enabled = {}\n  \
             skills.max_context_tokens = {}\n  \
             conversation.max_iterations = {}\n  \
             conversation.show_thinking = {}\n  \
             security.secret_detection = {}\n  \
             security.host_audit = {}\n  \
             budget.monthly_token_limit = {}\n  \
             budget.warning_threshold = {}\n  \
             browser.enabled = {}\n  \
             browser.headless = {}",
            self.llm.model,
            self.llm.temperature,
            self.llm.max_tokens,
            self.sandbox.enabled,
            self.sandbox.mode,
            self.memory.max_context_tokens,
            self.skills.enabled,
            self.skills.max_context_tokens,
            self.conversation.max_iterations,
            self.conversation.show_thinking,
            self.security.secret_detection,
            self.security.host_audit,
            self.budget.monthly_token_limit,
            self.budget.warning_threshold,
            self.browser.enabled,
            self.browser.headless,
        )
    }

    pub fn apply_setting(&mut self, key: &str, value: &str) -> Result<String> {
        match key {
            "model" => {
                self.llm.model = value.to_string();
                Ok(format!("model = {value}"))
            }
            "temperature" => {
                let v: f32 = value
                    .parse()
                    .with_context(|| "Invalid float for temperature")?;
                if !(0.0..=2.0).contains(&v) {
                    anyhow::bail!("temperature must be between 0.0 and 2.0");
                }
                self.llm.temperature = v;
                Ok(format!("temperature = {v}"))
            }
            "max_tokens" => {
                let v: u32 = value
                    .parse()
                    .with_context(|| "Invalid integer for max_tokens")?;
                self.llm.max_tokens = v;
                Ok(format!("max_tokens = {v}"))
            }
            "provider" => {
                self.llm.provider = Some(value.to_string());
                Ok(format!("provider = {value}"))
            }
            "sandbox.mode" => {
                self.sandbox.mode = value.to_string();
                Ok(format!("sandbox.mode = {value}"))
            }
            "sandbox.enabled" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for sandbox.enabled")?;
                self.sandbox.enabled = v;
                Ok(format!("sandbox.enabled = {v}"))
            }
            "memory.max_context_tokens" => {
                let v: usize = value.parse().with_context(|| "Invalid integer")?;
                self.memory.max_context_tokens = v;
                Ok(format!("memory.max_context_tokens = {v}"))
            }
            "skills.enabled" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for skills.enabled")?;
                self.skills.enabled = v;
                Ok(format!("skills.enabled = {v}"))
            }
            "skills.max_context_tokens" => {
                let v: usize = value
                    .parse()
                    .with_context(|| "Invalid integer for skills.max_context_tokens")?;
                self.skills.max_context_tokens = v;
                Ok(format!("skills.max_context_tokens = {v}"))
            }
            "conversation.max_iterations" => {
                let v: u32 = value
                    .parse()
                    .with_context(|| "Invalid integer for max_iterations")?;
                self.conversation.max_iterations = v;
                Ok(format!("conversation.max_iterations = {v}"))
            }
            "conversation.show_thinking" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for show_thinking")?;
                self.conversation.show_thinking = v;
                Ok(format!("conversation.show_thinking = {v}"))
            }
            "security.secret_detection" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for secret_detection")?;
                self.security.secret_detection = v;
                Ok(format!("security.secret_detection = {v}"))
            }
            "security.hitl_dangerous_ops" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for hitl_dangerous_ops")?;
                self.security.hitl_dangerous_ops = v;
                Ok(format!("security.hitl_dangerous_ops = {v}"))
            }
            "security.host_audit" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for host_audit")?;
                self.security.host_audit = v;
                Ok(format!("security.host_audit = {v}"))
            }
            "budget.monthly_token_limit" => {
                let v: u64 = value
                    .parse()
                    .with_context(|| "Invalid integer for monthly_token_limit")?;
                self.budget.monthly_token_limit = v;
                Ok(format!("budget.monthly_token_limit = {v}"))
            }
            "budget.warning_threshold" => {
                let v: f64 = value
                    .parse()
                    .with_context(|| "Invalid float for warning_threshold")?;
                if !(0.0..=1.0).contains(&v) {
                    anyhow::bail!("warning_threshold must be between 0.0 and 1.0");
                }
                self.budget.warning_threshold = v;
                Ok(format!("budget.warning_threshold = {v}"))
            }
            "browser.enabled" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for browser.enabled")?;
                self.browser.enabled = v;
                Ok(format!("browser.enabled = {v}"))
            }
            "browser.headless" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for browser.headless")?;
                self.browser.headless = v;
                Ok(format!("browser.headless = {v}"))
            }
            _ => anyhow::bail!(
                "Unknown setting: {key}\nAvailable: model, temperature, max_tokens, provider, \
                 sandbox.mode, sandbox.enabled, memory.max_context_tokens, skills.enabled, \
                 skills.max_context_tokens, conversation.max_iterations, conversation.show_thinking, \
                 security.secret_detection, security.host_audit, \
                 budget.monthly_token_limit, budget.warning_threshold, \
                 browser.enabled, browser.headless"
            ),
        }
    }

    pub fn api_key(&self) -> Result<String> {
        // Try SecretRef first, then fall back to env var name
        if let Some(ref secret_ref) = self.llm.api_key {
            return secret_ref.resolve();
        }
        std::env::var(&self.llm.api_key_env).with_context(|| {
            format!(
                "API key not found. Set the {} environment variable or configure api_key in config.toml.",
                self.llm.api_key_env
            )
        })
    }

    /// Resolve the provider and API key from config + environment.
    /// Resolution priority: api_key (SecretRef) → api_key_env → provider default env var → auto-detect.
    pub fn resolve_provider(&self) -> Result<(Provider, String)> {
        if let Some(ref provider_str) = self.llm.provider {
            let provider = Provider::from_str(provider_str)?;

            // Keyless providers (e.g., Ollama) don't need API key resolution
            if !provider.requires_api_key() {
                return Ok((provider, String::new()));
            }

            // Try SecretRef first
            if let Some(ref secret_ref) = self.llm.api_key {
                match secret_ref.resolve() {
                    Ok(key) if !key.is_empty() => return Ok((provider, key)),
                    Ok(_) => {
                        eprintln!("Warning: api_key SecretRef resolved to empty string, falling back to api_key_env");
                    }
                    Err(e) => {
                        eprintln!("Warning: api_key SecretRef failed to resolve: {e}, falling back to api_key_env");
                    }
                }
            }

            let key = std::env::var(&self.llm.api_key_env)
                .or_else(|_| std::env::var(provider.default_env_var()))
                .with_context(|| {
                    format!(
                        "API key not found for provider {provider}. Set {} or {} or configure api_key in config.toml.",
                        self.llm.api_key_env,
                        provider.default_env_var()
                    )
                })?;
            return Ok((provider, key));
        }

        // Try SecretRef with auto-detect
        if let Some(ref secret_ref) = self.llm.api_key {
            match secret_ref.resolve() {
                Ok(key) if !key.is_empty() => {
                    // Infer provider from api_key_env name
                    let provider = match self.llm.api_key_env.as_str() {
                        "OPENAI_API_KEY" => Provider::OpenAi,
                        "ANTHROPIC_API_KEY" => Provider::Anthropic,
                        "GEMINI_API_KEY" => Provider::Gemini,
                        "OLLAMA_HOST" => Provider::Ollama,
                        _ => Provider::OpenRouter,
                    };
                    return Ok((provider, key));
                }
                Ok(_) => {
                    eprintln!("Warning: api_key SecretRef resolved to empty string, falling back to env detection");
                }
                Err(e) => {
                    eprintln!("Warning: api_key SecretRef failed to resolve: {e}, falling back to env detection");
                }
            }
        }

        if self.llm.api_key_env != LlmConfig::default().api_key_env {
            if let Ok(key) = std::env::var(&self.llm.api_key_env) {
                if !key.is_empty() {
                    let provider = match self.llm.api_key_env.as_str() {
                        "OPENAI_API_KEY" => Provider::OpenAi,
                        "ANTHROPIC_API_KEY" => Provider::Anthropic,
                        "GEMINI_API_KEY" => Provider::Gemini,
                        "OLLAMA_HOST" => Provider::Ollama,
                        _ => Provider::OpenRouter,
                    };
                    return Ok((provider, key));
                }
            }
        }

        Provider::detect_from_env()
    }

    /// Resolve all available API keys for the configured provider.
    /// Returns the provider and a list of resolved keys (for multi-key fallback).
    /// Falls back to a single key from `resolve_provider` if no `api_keys` are configured.
    pub fn resolve_api_keys(&self) -> Result<(Provider, Vec<String>)> {
        // Try multi-key resolution first (avoids requiring a default API key env var)
        if !self.llm.api_keys.is_empty() {
            let mut keys = Vec::new();
            for secret_ref in &self.llm.api_keys {
                if let Ok(key) = secret_ref.resolve() {
                    if !key.is_empty() {
                        keys.push(key);
                    }
                }
            }
            if !keys.is_empty() {
                let provider = if let Some(ref provider_str) = self.llm.provider {
                    Provider::from_str(provider_str)?
                } else {
                    // Infer provider via resolve_provider; ignore errors since we have keys
                    self.resolve_provider()
                        .map(|(p, _)| p)
                        .unwrap_or(Provider::OpenRouter)
                };
                return Ok((provider, keys));
            }
        }

        // Fall back to single key via resolve_provider
        let (provider, key) = self.resolve_provider()?;
        Ok((provider, vec![key]))
    }

    /// Resolve all credentials to their plaintext values.
    /// Logs warnings for credentials that fail to resolve.
    pub fn resolve_credentials(&self) -> HashMap<String, String> {
        self.credentials
            .iter()
            .filter_map(|(name, cv)| match cv.resolve() {
                Ok(v) => Some((name.clone(), v)),
                Err(e) => {
                    tracing::warn!("Failed to resolve credential '{name}': {e}");
                    None
                }
            })
            .collect()
    }

    /// Resolve a credential by name: credential store first, then env var fallback.
    /// Returns `None` if not found or empty.
    pub fn resolve_credential_or_env(&self, name: &str) -> Option<String> {
        self.credentials
            .get(name)
            .and_then(|cv| cv.resolve().ok())
            .or_else(|| std::env::var(name).ok())
            .filter(|t| !t.is_empty())
    }

    /// Returns true if any native channel credentials are configured.
    pub fn has_any_native_channel(&self) -> bool {
        const NATIVE_KEYS: &[&str] = &[
            "TELEGRAM_BOT_TOKEN",
            "SLACK_BOT_TOKEN",
            "DISCORD_BOT_TOKEN",
            "TWILIO_ACCOUNT_SID",
            "TEAMS_APP_ID",
            "GOOGLE_CHAT_SERVICE_TOKEN",
            "SIGNAL_ACCOUNT",
        ];
        NATIVE_KEYS
            .iter()
            .any(|k| self.resolve_credential_or_env(k).is_some())
    }

    /// Returns a list of detected native channels with their names and descriptions.
    pub fn detected_native_channels(&self) -> Vec<(&'static str, &'static str)> {
        const NATIVE_CHANNELS: &[(&str, &str, &str)] = &[
            (
                "telegram",
                "Telegram Bot API (native)",
                "TELEGRAM_BOT_TOKEN",
            ),
            ("slack", "Slack Bot API (native)", "SLACK_BOT_TOKEN"),
            ("discord", "Discord Bot (native)", "DISCORD_BOT_TOKEN"),
            (
                "twilio",
                "Twilio SMS/WhatsApp (native)",
                "TWILIO_ACCOUNT_SID",
            ),
            ("teams", "Microsoft Teams (native)", "TEAMS_APP_ID"),
            (
                "google-chat",
                "Google Chat (native)",
                "GOOGLE_CHAT_SERVICE_TOKEN",
            ),
            (
                "signal",
                "Signal Messenger (native via signal-cli)",
                "SIGNAL_ACCOUNT",
            ),
        ];
        NATIVE_CHANNELS
            .iter()
            .filter(|(_, _, key)| self.resolve_credential_or_env(key).is_some())
            .map(|(name, desc, _)| (*name, *desc))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn default_config_values() {
        let cfg = Config::default();
        assert!(cfg.llm.provider.is_none());
        assert_eq!(cfg.llm.api_key_env, "OPENROUTER_API_KEY");
        assert_eq!(cfg.llm.model, "anthropic/claude-sonnet-4");
        assert!((cfg.llm.temperature - 0.7).abs() < f32::EPSILON);
        assert_eq!(cfg.llm.max_tokens, 4096);
        assert!(cfg.llm.base_url.is_none());
        assert!(!cfg.heartbeat.enabled);
        assert_eq!(cfg.heartbeat.interval, "30m");
        assert_eq!(cfg.tools.default_timeout_ms, 30000);
        assert!(cfg.sandbox.enabled);
        assert_eq!(cfg.sandbox.mode, "strict");
        assert_eq!(cfg.memory.max_context_tokens, 8000);
        assert!(cfg.skills.enabled);
        assert_eq!(cfg.skills.max_context_tokens, 4000);
        assert_eq!(cfg.conversation.max_history_tokens, 32000);
        assert_eq!(cfg.conversation.max_iterations, 25);
        assert!(cfg.conversation.show_thinking);
        assert!(cfg.security.secret_detection);
    }

    #[test]
    fn parse_complete_config_toml() {
        let toml_str = r#"
[llm]
api_key_env = "MY_KEY"
model = "openai/gpt-4"
temperature = 0.9
max_tokens = 2048

[heartbeat]
enabled = true
interval = "1h"
quiet_hours_start = "22:00"
quiet_hours_end = "08:00"
cron = "0 */2 * * *"

[tools]
default_timeout_ms = 60000

[sandbox]
enabled = false
mode = "permissive"

[memory]
max_context_tokens = 16000

[conversation]
max_history_tokens = 64000
max_iterations = 50
show_thinking = false

[security]
secret_detection = false
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse complete toml");
        assert_eq!(cfg.llm.api_key_env, "MY_KEY");
        assert_eq!(cfg.llm.model, "openai/gpt-4");
        assert_eq!(cfg.conversation.max_iterations, 50);
        assert!(!cfg.conversation.show_thinking);
        assert!(!cfg.security.secret_detection);
    }

    #[test]
    fn parse_empty_config_toml_yields_defaults() {
        let cfg: Config = toml::from_str("").expect("should parse empty toml");
        let defaults = Config::default();
        assert_eq!(cfg.llm.api_key_env, defaults.llm.api_key_env);
        assert_eq!(cfg.llm.model, defaults.llm.model);
        assert_eq!(
            cfg.conversation.max_iterations,
            defaults.conversation.max_iterations
        );
    }

    #[test]
    fn parse_minimal_config_toml_with_partial_sections() {
        let toml_str = r#"
[llm]
model = "meta/llama-3"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse partial toml");
        assert_eq!(cfg.llm.model, "meta/llama-3");
        assert_eq!(cfg.llm.api_key_env, "OPENROUTER_API_KEY");
    }

    #[test]
    fn load_from_nonexistent_path_returns_defaults() {
        let path = Path::new("/tmp/borg_test_nonexistent_config.toml");
        let cfg = Config::load_from(path).expect("should return default for missing file");
        let defaults = Config::default();
        assert_eq!(cfg.llm.model, defaults.llm.model);
    }

    #[test]
    fn load_from_file_on_disk() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("config.toml");
        {
            let mut f = std::fs::File::create(&path).expect("create file");
            write!(
                f,
                "[llm]\nmodel = \"test-model\"\n[sandbox]\nenabled = false\n"
            )
            .expect("write file");
        }
        let cfg = Config::load_from(&path).expect("should load from temp file");
        assert_eq!(cfg.llm.model, "test-model");
        assert!(!cfg.sandbox.enabled);
    }

    #[test]
    fn api_key_resolved_from_env() {
        let env_name = "BORG_TEST_API_KEY_RESOLVE";
        let mut cfg = Config::default();
        cfg.llm.api_key_env = env_name.to_string();
        std::env::remove_var(env_name);
        assert!(cfg.api_key().is_err());
        std::env::set_var(env_name, "sk-test-12345");
        let key = cfg.api_key().expect("should resolve key from env");
        assert_eq!(key, "sk-test-12345");
        std::env::remove_var(env_name);
    }

    #[test]
    fn parse_config_with_user_section() {
        let toml_str = r#"
[user]
name = "Mike"
agent_name = "Buddy"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.user.name.as_deref(), Some("Mike"));
        assert_eq!(cfg.user.agent_name.as_deref(), Some("Buddy"));
    }

    #[test]
    fn apply_setting_model() {
        let mut cfg = Config::default();
        let result = cfg.apply_setting("model", "gpt-4o").unwrap();
        assert!(result.contains("gpt-4o"));
        assert_eq!(cfg.llm.model, "gpt-4o");
    }

    #[test]
    fn apply_setting_temperature() {
        let mut cfg = Config::default();
        cfg.apply_setting("temperature", "0.5").unwrap();
        assert!((cfg.llm.temperature - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_setting_temperature_out_of_range() {
        let mut cfg = Config::default();
        assert!(cfg.apply_setting("temperature", "3.0").is_err());
        assert!(cfg.apply_setting("temperature", "-1.0").is_err());
    }

    #[test]
    fn apply_setting_max_iterations() {
        let mut cfg = Config::default();
        cfg.apply_setting("conversation.max_iterations", "50")
            .unwrap();
        assert_eq!(cfg.conversation.max_iterations, 50);
    }

    #[test]
    fn apply_setting_show_thinking() {
        let mut cfg = Config::default();
        cfg.apply_setting("conversation.show_thinking", "false")
            .unwrap();
        assert!(!cfg.conversation.show_thinking);
    }

    #[test]
    fn apply_setting_secret_detection() {
        let mut cfg = Config::default();
        cfg.apply_setting("security.secret_detection", "false")
            .unwrap();
        assert!(!cfg.security.secret_detection);
    }

    #[test]
    fn default_hitl_dangerous_ops_is_false() {
        let cfg = Config::default();
        assert!(!cfg.security.hitl_dangerous_ops);
    }

    #[test]
    fn apply_setting_hitl_dangerous_ops() {
        let mut cfg = Config::default();
        cfg.apply_setting("security.hitl_dangerous_ops", "true")
            .unwrap();
        assert!(cfg.security.hitl_dangerous_ops);
    }

    #[test]
    fn apply_setting_unknown_key_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_setting("nonexistent", "value").is_err());
    }

    #[test]
    fn display_settings_contains_key_fields() {
        let cfg = Config::default();
        let display = cfg.display_settings();
        assert!(display.contains("model"));
        assert!(display.contains("max_iterations"));
        assert!(display.contains("secret_detection"));
    }

    #[test]
    fn default_web_config_values() {
        let cfg = WebConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.search_provider, "duckduckgo");
        assert!(cfg.search_api_key_env.is_none());
    }

    #[test]
    fn default_tasks_config_values() {
        let cfg = TasksConfig::default();
        assert_eq!(cfg.max_concurrent, 3);
    }

    #[test]
    fn default_llm_config_retry_fields() {
        let cfg = LlmConfig::default();
        assert_eq!(cfg.max_retries, 3);
        assert_eq!(cfg.initial_retry_delay_ms, 200);
        assert_eq!(cfg.request_timeout_ms, 60000);
    }

    #[test]
    fn path_helpers() {
        let data = Config::data_dir().unwrap();
        assert!(data.to_string_lossy().ends_with(".borg"));

        let memory = Config::memory_dir().unwrap();
        assert_eq!(memory, data.join("memory"));

        let skills = Config::skills_dir().unwrap();
        assert_eq!(skills, data.join("skills"));

        let tools = Config::tools_dir().unwrap();
        assert_eq!(tools, data.join("tools"));

        let logs = Config::logs_dir().unwrap();
        assert_eq!(logs, data.join("logs"));

        let sessions = Config::sessions_dir().unwrap();
        assert_eq!(sessions, data.join("sessions"));

        let db = Config::db_path().unwrap();
        assert_eq!(db, data.join("borg.db"));

        let identity = Config::identity_path().unwrap();
        assert_eq!(identity, data.join("IDENTITY.md"));

        let mem_index = Config::memory_index_path().unwrap();
        assert_eq!(mem_index, data.join("MEMORY.md"));
    }

    #[test]
    fn partial_section_defaults_remaining_fields() {
        // Only set model; temperature, max_tokens, etc. should get defaults
        let toml_str = r#"
[llm]
model = "custom-model"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.llm.model, "custom-model");
        assert!((cfg.llm.temperature - 0.7).abs() < f32::EPSILON);
        assert_eq!(cfg.llm.max_tokens, 4096);
        assert_eq!(cfg.llm.max_retries, 3);
        assert_eq!(cfg.llm.initial_retry_delay_ms, 200);
        assert_eq!(cfg.llm.request_timeout_ms, 60000);
        assert_eq!(cfg.llm.api_key_env, "OPENROUTER_API_KEY");
        assert!(cfg.llm.base_url.is_none());
    }

    #[test]
    fn partial_web_config_defaults_remaining_fields() {
        let toml_str = r#"
[web]
enabled = false
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(!cfg.web.enabled);
        assert_eq!(cfg.web.search_provider, "duckduckgo");
        assert!(cfg.web.search_api_key_env.is_none());
    }

    #[test]
    fn partial_tasks_config_defaults_remaining_fields() {
        let toml_str = r#"
[tasks]
max_concurrent = 5
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.tasks.max_concurrent, 5);
    }

    #[test]
    fn save_and_reload_round_trip() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config_path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.llm.model = "test-round-trip".to_string();
        cfg.llm.temperature = 1.5;
        let content = toml::to_string_pretty(&cfg).expect("serialize");
        std::fs::write(&config_path, content).expect("write");
        let loaded = Config::load_from(&config_path).expect("reload");
        assert_eq!(loaded.llm.model, "test-round-trip");
        assert!((loaded.llm.temperature - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn default_budget_config_values() {
        let cfg = BudgetConfig::default();
        assert_eq!(cfg.monthly_token_limit, 1_000_000);
        assert!((cfg.warning_threshold - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_budget_config_from_toml() {
        let toml_str = r#"
[budget]
monthly_token_limit = 5000000
warning_threshold = 0.9
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.budget.monthly_token_limit, 5_000_000);
        assert!((cfg.budget.warning_threshold - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_budget_config_defaults_when_absent() {
        let cfg: Config = toml::from_str("").expect("should parse");
        assert_eq!(cfg.budget.monthly_token_limit, 1_000_000);
        assert!((cfg.budget.warning_threshold - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_setting_budget_monthly_token_limit() {
        let mut cfg = Config::default();
        cfg.apply_setting("budget.monthly_token_limit", "1000000")
            .expect("should succeed");
        assert_eq!(cfg.budget.monthly_token_limit, 1_000_000);
    }

    #[test]
    fn apply_setting_budget_warning_threshold() {
        let mut cfg = Config::default();
        cfg.apply_setting("budget.warning_threshold", "0.9")
            .expect("should succeed");
        assert!((cfg.budget.warning_threshold - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_setting_budget_warning_threshold_out_of_range() {
        let mut cfg = Config::default();
        assert!(cfg
            .apply_setting("budget.warning_threshold", "1.5")
            .is_err());
        assert!(cfg
            .apply_setting("budget.warning_threshold", "-0.1")
            .is_err());
    }

    #[test]
    fn display_settings_contains_budget() {
        let cfg = Config::default();
        let display = cfg.display_settings();
        assert!(display.contains("budget.monthly_token_limit"));
        assert!(display.contains("budget.warning_threshold"));
    }

    #[test]
    fn telemetry_config_defaults() {
        let cfg = TelemetryConfig::default();
        assert!(!cfg.tracing_enabled);
        assert!(!cfg.metrics_enabled);
        assert_eq!(cfg.otlp_endpoint, "http://localhost:4317");
        assert_eq!(cfg.service_name, "borg");
        assert!((cfg.sampling_ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn telemetry_config_parsing() {
        let toml_str = r#"
[telemetry]
tracing_enabled = true
metrics_enabled = true
otlp_endpoint = "http://otel:4317"
service_name = "my-borg"
sampling_ratio = 0.5
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(cfg.telemetry.tracing_enabled);
        assert!(cfg.telemetry.metrics_enabled);
        assert_eq!(cfg.telemetry.otlp_endpoint, "http://otel:4317");
        assert_eq!(cfg.telemetry.service_name, "my-borg");
        assert!((cfg.telemetry.sampling_ratio - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_setting_telemetry_hidden_from_user() {
        let mut cfg = Config::default();
        assert!(cfg
            .apply_setting("telemetry.tracing_enabled", "true")
            .is_err());
    }

    #[test]
    fn parse_config_with_secret_ref_env() {
        let toml_str = r#"
[llm]
provider = "openrouter"
api_key = { source = "env", var = "MY_SECRET_KEY" }
model = "anthropic/claude-sonnet-4"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(cfg.llm.api_key.is_some());
        if let Some(SecretRef::Env { var }) = &cfg.llm.api_key {
            assert_eq!(var, "MY_SECRET_KEY");
        } else {
            panic!("expected Env variant");
        }
    }

    #[test]
    fn parse_config_with_secret_ref_exec() {
        let toml_str = r#"
[llm]
provider = "openrouter"
api_key = { source = "exec", command = "security", args = ["find-generic-password", "-s", "borg", "-w"] }
model = "anthropic/claude-sonnet-4"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(cfg.llm.api_key.is_some());
        if let Some(SecretRef::Exec { command, args }) = &cfg.llm.api_key {
            assert_eq!(command, "security");
            assert_eq!(args.len(), 4);
        } else {
            panic!("expected Exec variant");
        }
    }

    #[test]
    fn parse_config_with_api_keys_list() {
        let toml_str = r#"
[llm]
provider = "openrouter"
model = "anthropic/claude-sonnet-4"

[[llm.api_keys]]
source = "env"
var = "PRIMARY_KEY"

[[llm.api_keys]]
source = "env"
var = "FALLBACK_KEY"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.llm.api_keys.len(), 2);
    }

    #[test]
    fn parse_config_without_secret_ref_uses_defaults() {
        let toml_str = r#"
[llm]
api_key_env = "MY_KEY"
model = "test-model"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(cfg.llm.api_key.is_none());
        assert!(cfg.llm.api_keys.is_empty());
        assert_eq!(cfg.llm.api_key_env, "MY_KEY");
    }

    #[test]
    fn resolve_provider_prefers_secret_ref() {
        let env_name = "BORG_TEST_SECRET_REF_RESOLVE";
        std::env::set_var(env_name, "secret-ref-key");
        let mut cfg = Config::default();
        cfg.llm.provider = Some("openrouter".to_string());
        cfg.llm.api_key = Some(SecretRef::Env {
            var: env_name.to_string(),
        });
        let (provider, key) = cfg.resolve_provider().expect("should resolve");
        assert_eq!(key, "secret-ref-key");
        assert_eq!(provider, Provider::OpenRouter);
        std::env::remove_var(env_name);
    }

    #[test]
    fn resolve_api_keys_multi() {
        let env1 = "BORG_TEST_MULTI_KEY_1";
        let env2 = "BORG_TEST_MULTI_KEY_2";
        std::env::set_var(env1, "key-one");
        std::env::set_var(env2, "key-two");
        let mut cfg = Config::default();
        cfg.llm.provider = Some("openrouter".to_string());
        cfg.llm.api_keys = vec![
            SecretRef::Env {
                var: env1.to_string(),
            },
            SecretRef::Env {
                var: env2.to_string(),
            },
        ];
        let (_, keys) = cfg.resolve_api_keys().expect("should resolve");
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], "key-one");
        assert_eq!(keys[1], "key-two");
        std::env::remove_var(env1);
        std::env::remove_var(env2);
    }

    /// Ensure that serializing a Config and parsing it back produces valid TOML.
    /// This catches issues like duplicate table headers.
    #[test]
    fn save_produces_parseable_toml() {
        let cfg = Config::default();
        let serialized = toml::to_string_pretty(&cfg).expect("serialize default config");
        let _parsed: Config = toml::from_str(&serialized).unwrap_or_else(|e| {
            panic!("default config round-trip failed:\n{serialized}\nerror: {e}")
        });
    }

    /// Same as above but with various fields populated.
    #[test]
    fn save_with_populated_fields_produces_parseable_toml() {
        let mut cfg = Config::default();
        cfg.llm.provider = Some("openrouter".to_string());
        cfg.llm.model = "anthropic/claude-sonnet-4".to_string();
        cfg.llm.api_key = Some(SecretRef::Env {
            var: "MY_KEY".to_string(),
        });
        cfg.llm.api_keys = vec![
            SecretRef::Env {
                var: "KEY1".to_string(),
            },
            SecretRef::Exec {
                command: "security".to_string(),
                args: vec!["find-generic-password".to_string(), "-w".to_string()],
            },
        ];
        cfg.user.name = Some("Test".to_string());
        cfg.user.agent_name = Some("Buddy".to_string());
        cfg.credentials.insert(
            "test".to_string(),
            CredentialValue::EnvVar("value".to_string()),
        );
        cfg.budget.monthly_token_limit = 1_000_000;

        let serialized = toml::to_string_pretty(&cfg).expect("serialize");
        let parsed: Config = toml::from_str(&serialized).unwrap_or_else(|e| {
            panic!("populated config round-trip failed:\n{serialized}\nerror: {e}")
        });
        assert_eq!(parsed.llm.model, "anthropic/claude-sonnet-4");
        assert!(parsed.llm.api_key.is_some());
        assert_eq!(parsed.llm.api_keys.len(), 2);
        assert_eq!(parsed.budget.monthly_token_limit, 1_000_000);
    }

    /// Verify that a realistic config.toml (matching the format produced by onboarding) parses.
    #[test]
    fn parse_realistic_config_toml() {
        let toml_str = r#"
[user]
name = "Mike"
agent_name = "Buddy"

[llm]
provider = "openrouter"
api_key_env = "OPENROUTER_API_KEY"
model = "anthropic/claude-sonnet-4"
temperature = 0.7
max_tokens = 4096
max_retries = 3
initial_retry_delay_ms = 200
request_timeout_ms = 60000

[heartbeat]
enabled = false
interval = "30m"

[tools]
default_timeout_ms = 30000

[sandbox]
enabled = true
mode = "strict"

[memory]
max_context_tokens = 8000

[skills]
enabled = true
max_context_tokens = 4000

[conversation]
max_history_tokens = 32000
max_iterations = 25
show_thinking = true

[policy]
auto_approve = []
deny = []

[debug]
llm_logging = false

[security]
secret_detection = true
blocked_paths = [".ssh", ".aws", ".gnupg", ".config/gh", ".env", "credentials", "private_key"]

[web]
enabled = true
search_provider = "duckduckgo"

[tasks]
enabled = false
max_concurrent = 3

[budget]
monthly_token_limit = 0
warning_threshold = 0.8

[gateway]
enabled = false
host = "127.0.0.1"
port = 7842
max_concurrent = 10
request_timeout_ms = 120000

[credentials]
"#;
        let cfg: Config = toml::from_str(toml_str)
            .unwrap_or_else(|e| panic!("realistic config parse failed: {e}"));
        assert_eq!(cfg.user.name.as_deref(), Some("Mike"));
        assert_eq!(cfg.llm.model, "anthropic/claude-sonnet-4");
        assert_eq!(cfg.llm.api_key_env, "OPENROUTER_API_KEY");
        assert!(cfg.llm.api_key.is_none());
    }

    #[test]
    fn parse_credentials_legacy_string() {
        let toml_str = r#"
[credentials]
JIRA_API_TOKEN = "JIRA_API_TOKEN"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(cfg.credentials.contains_key("JIRA_API_TOKEN"));
        if let CredentialValue::EnvVar(var) = &cfg.credentials["JIRA_API_TOKEN"] {
            assert_eq!(var, "JIRA_API_TOKEN");
        } else {
            panic!("expected EnvVar variant for legacy string");
        }
    }

    #[test]
    fn parse_credentials_secret_ref() {
        let toml_str = r#"
[credentials]
SLACK_TOKEN = { source = "exec", command = "echo", args = ["slack-secret"] }
GH_TOKEN = { source = "file", path = "/tmp/token" }
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(cfg.credentials.contains_key("SLACK_TOKEN"));
        assert!(cfg.credentials.contains_key("GH_TOKEN"));
        if let CredentialValue::Ref(SecretRef::Exec { command, .. }) =
            &cfg.credentials["SLACK_TOKEN"]
        {
            assert_eq!(command, "echo");
        } else {
            panic!("expected Ref(Exec) variant");
        }
    }

    #[test]
    fn resolve_credentials_filters_failures() {
        let var_name = "BORG_TEST_CRED_GOOD";
        std::env::set_var(var_name, "good-value");
        let mut cfg = Config::default();
        cfg.credentials.insert(
            "GOOD".to_string(),
            CredentialValue::EnvVar(var_name.to_string()),
        );
        cfg.credentials.insert(
            "BAD".to_string(),
            CredentialValue::EnvVar("DEFINITELY_NOT_SET_XYZ_12345".to_string()),
        );
        let resolved = cfg.resolve_credentials();
        assert_eq!(resolved.get("GOOD").unwrap(), "good-value");
        assert!(!resolved.contains_key("BAD"));
        std::env::remove_var(var_name);
    }

    #[test]
    fn credential_value_round_trip() {
        let mut cfg = Config::default();
        cfg.credentials.insert(
            "LEGACY".to_string(),
            CredentialValue::EnvVar("MY_VAR".to_string()),
        );
        cfg.credentials.insert(
            "EXEC_CRED".to_string(),
            CredentialValue::Ref(SecretRef::Exec {
                command: "security".to_string(),
                args: vec!["find-generic-password".to_string(), "-w".to_string()],
            }),
        );
        let serialized = toml::to_string_pretty(&cfg).expect("serialize");
        let parsed: Config = toml::from_str(&serialized).expect("deserialize");
        assert!(parsed.credentials.contains_key("LEGACY"));
        assert!(parsed.credentials.contains_key("EXEC_CRED"));
    }

    #[test]
    fn save_round_trip_no_duplicate_credentials() {
        // Simulate the plugin install flow: load config, add a keychain credential, save.
        // Verify the serialized output is valid TOML (no duplicate [credentials] section).
        let mut cfg = Config::default();
        cfg.llm.model = "test-model".to_string();
        cfg.credentials.insert(
            "TELEGRAM_BOT_TOKEN".to_string(),
            CredentialValue::Ref(SecretRef::Keychain {
                service: "borg-messaging-telegram".to_string(),
                account: "borg-TELEGRAM_BOT_TOKEN".to_string(),
            }),
        );
        let serialized = toml::to_string_pretty(&cfg).expect("serialize");
        // Must be valid TOML on re-parse
        let reparsed: Config = toml::from_str(&serialized).unwrap_or_else(|e| {
            panic!("serialized config is invalid TOML: {e}\n---\n{serialized}")
        });
        assert!(reparsed.credentials.contains_key("TELEGRAM_BOT_TOKEN"));

        // No duplicate [credentials] header
        let count = serialized
            .lines()
            .filter(|l| l.trim() == "[credentials]")
            .count();
        assert!(
            count <= 1,
            "expected at most 1 [credentials] section, got {count}\n---\n{serialized}"
        );
    }

    #[test]
    fn save_round_trip_with_existing_credentials_section() {
        // Reproduce the bug: config already has an empty [credentials] section,
        // then we load, add a credential, and re-serialize.
        let original = r#"
[llm]
model = "test"

[credentials]
"#;
        let mut cfg: Config = toml::from_str(original).expect("parse original");
        cfg.credentials.insert(
            "MY_KEY".to_string(),
            CredentialValue::Ref(SecretRef::Keychain {
                service: "svc".to_string(),
                account: "acct".to_string(),
            }),
        );
        let serialized = toml::to_string_pretty(&cfg).expect("serialize");
        // Must still be valid TOML
        let _reparsed: Config = toml::from_str(&serialized).unwrap_or_else(|e| {
            panic!("re-serialized config is invalid TOML: {e}\n---\n{serialized}")
        });

        let count = serialized
            .lines()
            .filter(|l| l.trim() == "[credentials]" || l.trim().starts_with("[credentials."))
            .count();
        assert!(
            count <= 1,
            "expected at most 1 credentials header, got {count}\n---\n{serialized}"
        );
    }

    #[test]
    fn dedup_toml_tables_removes_duplicate_credentials() {
        let input = r#"[llm]
model = "test"

[credentials]

[credentials]
"#;
        let output = Config::dedup_toml_tables(input);
        let count = output
            .lines()
            .filter(|l| l.trim() == "[credentials]")
            .count();
        assert_eq!(
            count, 1,
            "should have exactly 1 [credentials]\n---\n{output}"
        );
        // Must parse successfully
        let _cfg: Config = toml::from_str(&output).expect("deduped config should parse");
    }

    #[test]
    fn dedup_toml_tables_keeps_distinct_sections() {
        let input = r#"[llm]
model = "test"

[credentials]
KEY = "val"

[security]
secret_detection = true
"#;
        let output = Config::dedup_toml_tables(input);
        assert!(output.contains("[credentials]"));
        assert!(output.contains("[security]"));
        assert!(output.contains("KEY = \"val\""));
    }

    #[test]
    fn dedup_toml_tables_drops_duplicate_content() {
        let input = r#"[gateway]
enabled = true

[gateway]
enabled = false
"#;
        let output = Config::dedup_toml_tables(input);
        let count = output.lines().filter(|l| l.trim() == "[gateway]").count();
        assert_eq!(count, 1);
        // The first occurrence's content is kept
        assert!(output.contains("enabled = true"));
        // The duplicate's content is dropped
        assert!(!output.contains("enabled = false"));
    }

    #[test]
    fn load_from_handles_duplicate_credentials() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config_path = dir.path().join("config.toml");
        // Write a config with duplicate [credentials] — the exact bug scenario
        std::fs::write(
            &config_path,
            r#"[llm]
model = "test"

[credentials]

[credentials]
"#,
        )
        .expect("write");
        let cfg = Config::load_from(&config_path).expect("load should succeed despite duplicates");
        assert_eq!(cfg.llm.model, "test");
    }

    #[test]
    fn parse_credentials_secret_ref_env_variant() {
        let toml_str = r#"
[credentials]
MY_KEY = { source = "env", var = "MY_KEY_VAR" }
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        match &cfg.credentials["MY_KEY"] {
            CredentialValue::Ref(SecretRef::Env { var }) => {
                assert_eq!(var, "MY_KEY_VAR");
            }
            other => panic!("expected Ref(Env), got {other:?}"),
        }
    }

    #[test]
    fn default_browser_config_values() {
        let cfg = BrowserConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.headless);
        assert!(cfg.executable.is_none());
        assert_eq!(cfg.cdp_port, 9222);
        assert!(!cfg.no_sandbox);
        assert_eq!(cfg.timeout_ms, 30000);
        assert_eq!(cfg.startup_timeout_ms, 15000);
    }

    #[test]
    fn parse_browser_config_toml() {
        let toml_str = r#"
[browser]
enabled = false
headless = false
executable = "/usr/bin/chromium"
cdp_port = 9333
no_sandbox = true
timeout_ms = 60000
startup_timeout_ms = 20000
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(!cfg.browser.enabled);
        assert!(!cfg.browser.headless);
        assert_eq!(cfg.browser.executable.as_deref(), Some("/usr/bin/chromium"));
        assert_eq!(cfg.browser.cdp_port, 9333);
        assert!(cfg.browser.no_sandbox);
        assert_eq!(cfg.browser.timeout_ms, 60000);
        assert_eq!(cfg.browser.startup_timeout_ms, 20000);
    }

    #[test]
    fn parse_empty_toml_yields_browser_defaults() {
        let cfg: Config = toml::from_str("").expect("should parse");
        assert!(cfg.browser.enabled);
        assert!(cfg.browser.headless);
        assert_eq!(cfg.browser.cdp_port, 9222);
    }

    #[test]
    fn apply_setting_browser_headless() {
        let mut cfg = Config::default();
        cfg.apply_setting("browser.headless", "false").unwrap();
        assert!(!cfg.browser.headless);
    }

    #[test]
    fn apply_setting_browser_cdp_port_hidden() {
        let mut cfg = Config::default();
        assert!(cfg.apply_setting("browser.cdp_port", "9333").is_err());
    }

    #[test]
    fn display_settings_contains_browser() {
        let cfg = Config::default();
        let display = cfg.display_settings();
        assert!(display.contains("browser.enabled"));
        assert!(display.contains("browser.headless"));
        assert!(!display.contains("browser.cdp_port"));
    }

    #[test]
    fn embeddings_config_defaults() {
        let cfg = EmbeddingsConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.provider.is_none());
        assert!(cfg.model.is_none());
        assert!(cfg.dimension.is_none());
        assert!(cfg.api_key_env.is_none());
        assert!((cfg.recency_weight - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn memory_config_includes_embeddings() {
        let cfg = MemoryConfig::default();
        assert_eq!(cfg.max_context_tokens, 8000);
        assert!(cfg.embeddings.enabled);
    }

    #[test]
    fn embeddings_config_toml_deserialization() {
        let toml_str = r#"
[memory]
max_context_tokens = 4000

[memory.embeddings]
enabled = false
provider = "gemini"
model = "text-embedding-004"
dimension = 768
recency_weight = 0.5
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.memory.max_context_tokens, 4000);
        assert!(!cfg.memory.embeddings.enabled);
        assert_eq!(cfg.memory.embeddings.provider.as_deref(), Some("gemini"));
        assert_eq!(
            cfg.memory.embeddings.model.as_deref(),
            Some("text-embedding-004")
        );
        assert_eq!(cfg.memory.embeddings.dimension, Some(768));
        assert!((cfg.memory.embeddings.recency_weight - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn embeddings_config_absent_uses_defaults() {
        let toml_str = r#"
[memory]
max_context_tokens = 6000
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.memory.max_context_tokens, 6000);
        assert!(cfg.memory.embeddings.enabled);
        assert!(cfg.memory.embeddings.provider.is_none());
    }

    // ── Feature #11: Provider Failover config tests ──

    #[test]
    fn parse_llm_fallback_config() {
        let toml_str = r#"
[llm]
provider = "openrouter"
model = "anthropic/claude-sonnet-4"

[[llm.fallback]]
provider = "anthropic"
model = "claude-sonnet-4"
api_key_env = "ANTHROPIC_API_KEY"

[[llm.fallback]]
provider = "openai"
model = "gpt-4.1"
temperature = 0.5
max_tokens = 8192
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.llm.fallback.len(), 2);
        assert_eq!(cfg.llm.fallback[0].provider, "anthropic");
        assert_eq!(cfg.llm.fallback[0].model, "claude-sonnet-4");
        assert_eq!(
            cfg.llm.fallback[0].api_key_env.as_deref(),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(cfg.llm.fallback[1].provider, "openai");
        assert_eq!(cfg.llm.fallback[1].model, "gpt-4.1");
        assert!((cfg.llm.fallback[1].temperature.unwrap() - 0.5).abs() < f32::EPSILON);
        assert_eq!(cfg.llm.fallback[1].max_tokens, Some(8192));
    }

    #[test]
    fn parse_no_fallback_config() {
        let toml_str = r#"
[llm]
model = "test-model"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert!(cfg.llm.fallback.is_empty());
    }

    // ── Feature #10: Audio config tests ──

    #[test]
    fn parse_audio_config() {
        let toml_str = r#"
[audio]
enabled = true
max_file_size = 20971520
min_file_size = 1024
language = "en"
timeout_ms = 60000

[[audio.models]]
provider = "openai"
model = "whisper-1"

[[audio.models]]
provider = "groq"
model = "whisper-large-v3-turbo"
api_key_env = "GROQ_API_KEY"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert!(cfg.audio.enabled);
        assert_eq!(cfg.audio.max_file_size, 20_971_520);
        assert_eq!(cfg.audio.min_file_size, 1024);
        assert_eq!(cfg.audio.language.as_deref(), Some("en"));
        assert_eq!(cfg.audio.timeout_ms, 60_000);
        assert_eq!(cfg.audio.models.len(), 2);
        assert_eq!(cfg.audio.models[0].provider, "openai");
        assert_eq!(cfg.audio.models[0].model.as_deref(), Some("whisper-1"));
        assert_eq!(cfg.audio.models[1].provider, "groq");
        assert_eq!(
            cfg.audio.models[1].api_key_env.as_deref(),
            Some("GROQ_API_KEY")
        );
    }

    #[test]
    fn audio_config_defaults() {
        let cfg = AudioConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.models.is_empty());
        assert_eq!(cfg.max_file_size, 20 * 1024 * 1024);
        assert_eq!(cfg.min_file_size, 1024);
        assert!(!cfg.echo_transcript);
    }

    // ── Feature #12: Gateway bindings config tests ──

    #[test]
    fn parse_gateway_bindings_config() {
        let toml_str = r#"
[[gateway.bindings]]
channel = "telegram"
provider = "anthropic"
model = "claude-sonnet-4"
identity = "work-identity.md"
memory_scope = "work"

[[gateway.bindings]]
channel = "slack"
sender = "U12345*"
provider = "openai"
model = "gpt-4.1"
temperature = 0.3

[[gateway.bindings]]
channel = "discord"
peer_kind = "group"
memory_scope = "team"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.gateway.bindings.len(), 3);

        assert_eq!(cfg.gateway.bindings[0].channel, "telegram");
        assert_eq!(
            cfg.gateway.bindings[0].provider.as_deref(),
            Some("anthropic")
        );
        assert_eq!(
            cfg.gateway.bindings[0].identity.as_deref(),
            Some("work-identity.md")
        );
        assert_eq!(
            cfg.gateway.bindings[0].memory_scope.as_deref(),
            Some("work")
        );

        assert_eq!(cfg.gateway.bindings[1].channel, "slack");
        assert_eq!(cfg.gateway.bindings[1].sender.as_deref(), Some("U12345*"));
        assert!((cfg.gateway.bindings[1].temperature.unwrap() - 0.3).abs() < f32::EPSILON);

        assert_eq!(cfg.gateway.bindings[2].channel, "discord");
        assert_eq!(cfg.gateway.bindings[2].peer_kind.as_deref(), Some("group"));
    }

    #[test]
    fn gateway_bindings_empty_by_default() {
        let cfg = Config::default();
        assert!(cfg.gateway.bindings.is_empty());
    }

    // -- ToolPolicyConfig --

    #[test]
    fn tool_policy_default_values() {
        let policy = ToolPolicyConfig::default();
        assert_eq!(policy.profile, "full");
        assert!(policy.allow.is_empty());
        assert!(policy.deny.is_empty());
        assert!(policy.subagent_deny.contains(&"manage_tasks".to_string()));
        assert!(policy.subagent_deny.contains(&"security_audit".to_string()));
        assert!(policy.subagent_deny.contains(&"browser".to_string()));
    }

    #[test]
    fn tool_policy_config_is_part_of_tools_config() {
        let cfg = Config::default();
        assert_eq!(cfg.tools.policy.profile, "full");
    }

    #[test]
    fn parse_tool_policy_from_toml() {
        let toml_str = r#"
[tools]
default_timeout_ms = 30000

[tools.policy]
profile = "coding"
allow = ["write_memory", "group:fs"]
deny = ["security_audit"]
subagent_deny = ["manage_tasks"]
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse tools policy");
        assert_eq!(cfg.tools.policy.profile, "coding");
        assert_eq!(cfg.tools.policy.allow.len(), 2);
        assert!(cfg.tools.policy.allow.contains(&"write_memory".to_string()));
        assert!(cfg.tools.policy.allow.contains(&"group:fs".to_string()));
        assert_eq!(cfg.tools.policy.deny, vec!["security_audit".to_string()]);
        assert_eq!(
            cfg.tools.policy.subagent_deny,
            vec!["manage_tasks".to_string()]
        );
    }

    #[test]
    fn parse_tool_policy_empty_defaults() {
        let toml_str = r#"
[tools]
default_timeout_ms = 30000
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        let default_policy = ToolPolicyConfig::default();
        assert_eq!(cfg.tools.policy.profile, default_policy.profile);
        assert_eq!(cfg.tools.policy.allow.len(), default_policy.allow.len());
        assert_eq!(cfg.tools.policy.deny.len(), default_policy.deny.len());
    }

    #[test]
    fn test_skills_entries_deserialize() {
        let toml_str = r#"
[skills]
enabled = true
max_context_tokens = 4000

[skills.entries.slack]
enabled = true
env = { SLACK_BOT_TOKEN = "xoxb-test" }

[skills.entries.docker]
enabled = false
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert!(cfg.skills.entries.contains_key("slack"));
        assert!(cfg.skills.entries.contains_key("docker"));
        let slack = &cfg.skills.entries["slack"];
        assert!(slack.enabled);
        assert_eq!(slack.env.get("SLACK_BOT_TOKEN").unwrap(), "xoxb-test");
        let docker = &cfg.skills.entries["docker"];
        assert!(!docker.enabled);
    }

    #[test]
    fn test_skills_entries_default_empty() {
        let cfg = SkillsConfig::default();
        assert!(cfg.entries.is_empty());
    }

    #[test]
    fn test_skill_entry_enabled_default_true() {
        let entry = SkillEntryConfig::default();
        assert!(entry.enabled);
        assert!(entry.env.is_empty());
    }

    #[test]
    fn parse_config_with_base_url() {
        let toml_str = r#"
[llm]
provider = "ollama"
model = "llama3.3"
base_url = "http://my-server:11434/v1/chat/completions"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.llm.provider.as_deref(), Some("ollama"));
        assert_eq!(
            cfg.llm.base_url.as_deref(),
            Some("http://my-server:11434/v1/chat/completions")
        );
    }

    #[test]
    fn parse_ollama_config_no_api_key_required() {
        let toml_str = r#"
[llm]
provider = "ollama"
model = "llama3.3"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        let (provider, key) = cfg
            .resolve_provider()
            .expect("should resolve ollama without key");
        assert_eq!(provider, Provider::Ollama);
        assert!(key.is_empty());
    }

    #[test]
    fn resolve_api_keys_ollama_returns_empty_key() {
        let toml_str = r#"
[llm]
provider = "ollama"
model = "llama3.3"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        let (provider, keys) = cfg.resolve_api_keys().expect("should resolve");
        assert_eq!(provider, Provider::Ollama);
        assert_eq!(keys.len(), 1);
        assert!(keys[0].is_empty());
    }

    #[test]
    fn parse_config_with_base_url_for_cloud_provider() {
        let toml_str = r#"
[llm]
provider = "openai"
model = "gpt-4.1"
base_url = "https://my-azure-proxy.example.com/v1/chat/completions"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.llm.provider.as_deref(), Some("openai"));
        assert_eq!(
            cfg.llm.base_url.as_deref(),
            Some("https://my-azure-proxy.example.com/v1/chat/completions")
        );
    }

    #[test]
    fn parse_realistic_ollama_config() {
        let toml_str = r#"
[user]
name = "Mike"
agent_name = "Buddy"

[llm]
provider = "ollama"
model = "llama3.3"
temperature = 0.7
max_tokens = 4096

[sandbox]
enabled = true
mode = "strict"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.llm.provider.as_deref(), Some("ollama"));
        assert_eq!(cfg.llm.model, "llama3.3");
        assert!(cfg.llm.api_key.is_none());
        assert!(cfg.llm.api_keys.is_empty());
    }

    #[test]
    fn parse_fallback_with_base_url() {
        let toml_str = r#"
[llm]
provider = "ollama"
model = "llama3.3"

[[llm.fallback]]
provider = "openai"
model = "gpt-4.1-mini"
base_url = "https://proxy.example.com/v1/chat/completions"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.llm.fallback.len(), 1);
        assert_eq!(
            cfg.llm.fallback[0].base_url.as_deref(),
            Some("https://proxy.example.com/v1/chat/completions")
        );
    }

    #[test]
    fn has_any_native_channel_detects_telegram_env() {
        // Use a unique env var name to avoid conflicts with real credentials
        std::env::set_var("TELEGRAM_BOT_TOKEN", "test-token-for-unit-test");
        let cfg = Config::default();
        assert!(cfg.has_any_native_channel());
        std::env::remove_var("TELEGRAM_BOT_TOKEN");
    }

    #[test]
    fn has_any_native_channel_false_when_no_creds() {
        // Temporarily clear all native channel env vars
        let keys = [
            "TELEGRAM_BOT_TOKEN",
            "SLACK_BOT_TOKEN",
            "DISCORD_BOT_TOKEN",
            "TWILIO_ACCOUNT_SID",
            "TEAMS_APP_ID",
            "GOOGLE_CHAT_SERVICE_TOKEN",
        ];
        let saved: Vec<_> = keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        for k in &keys {
            std::env::remove_var(k);
        }
        let cfg = Config::default();
        assert!(!cfg.has_any_native_channel());
        // Restore
        for (k, v) in saved {
            if let Some(val) = v {
                std::env::set_var(k, val);
            }
        }
    }

    #[test]
    fn detected_native_channels_returns_configured() {
        std::env::set_var("SLACK_BOT_TOKEN", "xoxb-test-token");
        let cfg = Config::default();
        let channels = cfg.detected_native_channels();
        assert!(channels.iter().any(|(name, _)| *name == "slack"));
        std::env::remove_var("SLACK_BOT_TOKEN");
    }

    #[test]
    fn thinking_level_defaults_to_off() {
        let cfg = Config::default();
        assert_eq!(cfg.llm.thinking, ThinkingLevel::Off);
        assert!(cfg.llm.thinking.budget_tokens().is_none());
        assert!(cfg.llm.thinking.openai_reasoning_effort().is_none());
        assert!(!cfg.llm.thinking.is_enabled());
    }

    #[test]
    fn thinking_level_budget_tokens() {
        assert_eq!(ThinkingLevel::Low.budget_tokens(), Some(1024));
        assert_eq!(ThinkingLevel::Medium.budget_tokens(), Some(4096));
        assert_eq!(ThinkingLevel::High.budget_tokens(), Some(16384));
        assert_eq!(ThinkingLevel::Xhigh.budget_tokens(), Some(32768));
    }

    #[test]
    fn thinking_level_openai_reasoning_effort() {
        assert_eq!(ThinkingLevel::Low.openai_reasoning_effort(), Some("low"));
        assert_eq!(
            ThinkingLevel::Medium.openai_reasoning_effort(),
            Some("medium")
        );
        assert_eq!(ThinkingLevel::High.openai_reasoning_effort(), Some("high"));
        assert_eq!(ThinkingLevel::Xhigh.openai_reasoning_effort(), Some("high"));
    }

    #[test]
    fn thinking_level_serde_roundtrip() {
        let level: ThinkingLevel = serde_json::from_str(r#""high""#).unwrap();
        assert_eq!(level, ThinkingLevel::High);
        let json = serde_json::to_string(&level).unwrap();
        assert_eq!(json, r#""high""#);
    }

    #[test]
    fn parse_thinking_level_in_config_toml() {
        let toml_str = r#"
            [llm]
            model = "claude-sonnet-4"
            thinking = "medium"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&config_path).unwrap();
        f.write_all(toml_str.as_bytes()).unwrap();

        let cfg = Config::load_from(&config_path).unwrap();
        assert_eq!(cfg.llm.thinking, ThinkingLevel::Medium);
        assert_eq!(cfg.llm.thinking.budget_tokens(), Some(4096));
    }

    #[test]
    fn group_activation_defaults_to_mention() {
        let cfg = Config::default();
        assert_eq!(cfg.gateway.group_activation, ActivationMode::Mention);
    }
}
