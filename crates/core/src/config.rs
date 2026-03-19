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
    pub credentials: HashMap<String, CredentialValue>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HeartbeatConfig {
    pub enabled: bool,
    pub interval: String,
    pub quiet_hours_start: Option<String>,
    pub quiet_hours_end: Option<String>,
    pub cron: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub default_timeout_ms: u64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsConfig {
    pub enabled: bool,
    pub max_context_tokens: usize,
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
            monthly_token_limit: 0,
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
        }
    }
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
}

fn default_hitl_dangerous_ops() -> bool {
    true
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
        }
    }
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: "30m".into(),
            quiet_hours_start: None,
            quiet_hours_end: None,
            cron: None,
        }
    }
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 30000,
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
        }
    }
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_context_tokens: 4000,
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
            hitl_dangerous_ops: true,
            action_limits: crate::rate_guard::ActionLimits::default(),
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
        Ok(dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".borg"))
    }

    pub fn memory_dir() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("memory"))
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
             telemetry.tracing_enabled = {}\n  \
             telemetry.metrics_enabled = {}\n  \
             telemetry.otlp_endpoint = {}\n  \
             telemetry.service_name = {}\n  \
             telemetry.sampling_ratio = {}\n  \
             browser.enabled = {}\n  \
             browser.headless = {}\n  \
             browser.cdp_port = {}\n  \
             browser.timeout_ms = {}",
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
            self.telemetry.tracing_enabled,
            self.telemetry.metrics_enabled,
            self.telemetry.otlp_endpoint,
            self.telemetry.service_name,
            self.telemetry.sampling_ratio,
            self.browser.enabled,
            self.browser.headless,
            self.browser.cdp_port,
            self.browser.timeout_ms,
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
            "conversation.tool_output_max_tokens" => {
                let v: usize = value.parse().with_context(|| "Invalid integer")?;
                self.conversation.tool_output_max_tokens = v;
                Ok(format!("conversation.tool_output_max_tokens = {v}"))
            }
            "conversation.compaction_marker_tokens" => {
                let v: usize = value.parse().with_context(|| "Invalid integer")?;
                self.conversation.compaction_marker_tokens = v;
                Ok(format!("conversation.compaction_marker_tokens = {v}"))
            }
            "conversation.max_transcript_chars" => {
                let v: usize = value.parse().with_context(|| "Invalid integer")?;
                self.conversation.max_transcript_chars = v;
                Ok(format!("conversation.max_transcript_chars = {v}"))
            }
            "gateway.max_body_size" => {
                let v: usize = value.parse().with_context(|| "Invalid integer")?;
                self.gateway.max_body_size = v;
                Ok(format!("gateway.max_body_size = {v}"))
            }
            "gateway.telegram_poll_timeout_secs" => {
                let v: u64 = value.parse().with_context(|| "Invalid integer")?;
                self.gateway.telegram_poll_timeout_secs = v;
                Ok(format!("gateway.telegram_poll_timeout_secs = {v}"))
            }
            "gateway.telegram_circuit_failure_threshold" => {
                let v: u32 = value.parse().with_context(|| "Invalid integer")?;
                self.gateway.telegram_circuit_failure_threshold = v;
                Ok(format!("gateway.telegram_circuit_failure_threshold = {v}"))
            }
            "gateway.telegram_circuit_suspension_secs" => {
                let v: u64 = value.parse().with_context(|| "Invalid integer")?;
                self.gateway.telegram_circuit_suspension_secs = v;
                Ok(format!("gateway.telegram_circuit_suspension_secs = {v}"))
            }
            "gateway.telegram_dedup_capacity" => {
                let v: usize = value.parse().with_context(|| "Invalid integer")?;
                self.gateway.telegram_dedup_capacity = v;
                Ok(format!("gateway.telegram_dedup_capacity = {v}"))
            }
            "telemetry.tracing_enabled" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for telemetry.tracing_enabled")?;
                self.telemetry.tracing_enabled = v;
                Ok(format!("telemetry.tracing_enabled = {v}"))
            }
            "telemetry.metrics_enabled" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for telemetry.metrics_enabled")?;
                self.telemetry.metrics_enabled = v;
                Ok(format!("telemetry.metrics_enabled = {v}"))
            }
            "telemetry.otlp_endpoint" => {
                self.telemetry.otlp_endpoint = value.to_string();
                Ok(format!("telemetry.otlp_endpoint = {value}"))
            }
            "telemetry.service_name" => {
                self.telemetry.service_name = value.to_string();
                Ok(format!("telemetry.service_name = {value}"))
            }
            "telemetry.sampling_ratio" => {
                let v: f64 = value
                    .parse()
                    .with_context(|| "Invalid float for telemetry.sampling_ratio")?;
                if !(0.0..=1.0).contains(&v) {
                    anyhow::bail!("telemetry.sampling_ratio must be between 0.0 and 1.0");
                }
                self.telemetry.sampling_ratio = v;
                Ok(format!("telemetry.sampling_ratio = {v}"))
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
            "browser.cdp_port" => {
                let v: u16 = value
                    .parse()
                    .with_context(|| "Invalid integer for browser.cdp_port")?;
                self.browser.cdp_port = v;
                Ok(format!("browser.cdp_port = {v}"))
            }
            "browser.timeout_ms" => {
                let v: u64 = value
                    .parse()
                    .with_context(|| "Invalid integer for browser.timeout_ms")?;
                self.browser.timeout_ms = v;
                Ok(format!("browser.timeout_ms = {v}"))
            }
            "browser.startup_timeout_ms" => {
                let v: u64 = value
                    .parse()
                    .with_context(|| "Invalid integer for browser.startup_timeout_ms")?;
                self.browser.startup_timeout_ms = v;
                Ok(format!("browser.startup_timeout_ms = {v}"))
            }
            "browser.no_sandbox" => {
                let v: bool = value
                    .parse()
                    .with_context(|| "Invalid bool for browser.no_sandbox")?;
                self.browser.no_sandbox = v;
                Ok(format!("browser.no_sandbox = {v}"))
            }
            _ => anyhow::bail!(
                "Unknown setting: {key}\nAvailable: model, temperature, max_tokens, provider, \
                 sandbox.mode, sandbox.enabled, memory.max_context_tokens, skills.enabled, \
                 skills.max_context_tokens, conversation.max_iterations, conversation.show_thinking, \
                 conversation.tool_output_max_tokens, conversation.compaction_marker_tokens, \
                 conversation.max_transcript_chars, security.secret_detection, security.host_audit, \
                 budget.monthly_token_limit, budget.warning_threshold, gateway.max_body_size, \
                 gateway.telegram_poll_timeout_secs, gateway.telegram_circuit_failure_threshold, \
                 gateway.telegram_circuit_suspension_secs, gateway.telegram_dedup_capacity, \
                 telemetry.tracing_enabled, telemetry.metrics_enabled, telemetry.otlp_endpoint, \
                 telemetry.service_name, telemetry.sampling_ratio, \
                 browser.enabled, browser.headless, browser.cdp_port, browser.timeout_ms, \
                 browser.startup_timeout_ms, browser.no_sandbox"
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
        assert_eq!(cfg.monthly_token_limit, 0);
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
        assert_eq!(cfg.budget.monthly_token_limit, 0);
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
    fn apply_setting_telemetry() {
        let mut cfg = Config::default();
        cfg.apply_setting("telemetry.tracing_enabled", "true")
            .unwrap();
        assert!(cfg.telemetry.tracing_enabled);
        cfg.apply_setting("telemetry.metrics_enabled", "true")
            .unwrap();
        assert!(cfg.telemetry.metrics_enabled);
        cfg.apply_setting("telemetry.otlp_endpoint", "http://custom:4317")
            .unwrap();
        assert_eq!(cfg.telemetry.otlp_endpoint, "http://custom:4317");
        cfg.apply_setting("telemetry.service_name", "test-borg")
            .unwrap();
        assert_eq!(cfg.telemetry.service_name, "test-borg");
        cfg.apply_setting("telemetry.sampling_ratio", "0.25")
            .unwrap();
        assert!((cfg.telemetry.sampling_ratio - 0.25).abs() < f64::EPSILON);
        assert!(cfg
            .apply_setting("telemetry.sampling_ratio", "1.5")
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
        let mut cfg = Config::default();
        cfg.credentials.insert(
            "GOOD".to_string(),
            CredentialValue::Ref(SecretRef::Exec {
                command: "echo".to_string(),
                args: vec!["good-value".to_string()],
            }),
        );
        cfg.credentials.insert(
            "BAD".to_string(),
            CredentialValue::EnvVar("DEFINITELY_NOT_SET_XYZ_12345".to_string()),
        );
        let resolved = cfg.resolve_credentials();
        assert_eq!(resolved.get("GOOD").unwrap(), "good-value");
        assert!(!resolved.contains_key("BAD"));
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
    fn apply_setting_browser_cdp_port() {
        let mut cfg = Config::default();
        cfg.apply_setting("browser.cdp_port", "9333").unwrap();
        assert_eq!(cfg.browser.cdp_port, 9333);
    }

    #[test]
    fn display_settings_contains_browser() {
        let cfg = Config::default();
        let display = cfg.display_settings();
        assert!(display.contains("browser.enabled"));
        assert!(display.contains("browser.headless"));
        assert!(display.contains("browser.cdp_port"));
    }
}
