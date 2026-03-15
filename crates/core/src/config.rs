use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use std::str::FromStr;

use crate::policy::ExecutionPolicy;
use crate::provider::Provider;

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
    pub credentials: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub provider: Option<String>,
    pub api_key_env: String,
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
    pub enabled: bool,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugConfig {
    /// When true, log full LLM request/response to ~/.tamagotchi/logs/debug/
    #[serde(default)]
    pub llm_logging: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub secret_detection: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: None,
            api_key_env: "OPENROUTER_API_KEY".into(),
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
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            secret_detection: true,
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
        Self {
            enabled: false,
            max_concurrent: 3,
        }
    }
}

impl Config {
    pub fn data_dir() -> Result<PathBuf> {
        Ok(dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".tamagotchi"))
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

    pub fn logs_dir() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("logs"))
    }

    pub fn sessions_dir() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("sessions"))
    }

    pub fn db_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("tamagotchi.db"))
    }

    pub fn soul_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("SOUL.md"))
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
        let config: Config =
            toml::from_str(&content).with_context(|| "Failed to parse config.toml")?;
        Ok(config)
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
             budget.monthly_token_limit = {}\n  \
             budget.warning_threshold = {}",
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
            self.budget.monthly_token_limit,
            self.budget.warning_threshold,
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
            _ => anyhow::bail!(
                "Unknown setting: {key}\nAvailable: model, temperature, max_tokens, provider, \
                 sandbox.mode, sandbox.enabled, memory.max_context_tokens, skills.enabled, \
                 skills.max_context_tokens, conversation.max_iterations, conversation.show_thinking, \
                 security.secret_detection, budget.monthly_token_limit, budget.warning_threshold"
            ),
        }
    }

    pub fn api_key(&self) -> Result<String> {
        std::env::var(&self.llm.api_key_env).with_context(|| {
            format!(
                "API key not found. Set the {} environment variable.",
                self.llm.api_key_env
            )
        })
    }

    /// Resolve the provider and API key from config + environment.
    pub fn resolve_provider(&self) -> Result<(Provider, String)> {
        if let Some(ref provider_str) = self.llm.provider {
            let provider = Provider::from_str(provider_str)?;
            let key = std::env::var(&self.llm.api_key_env)
                .or_else(|_| std::env::var(provider.default_env_var()))
                .with_context(|| {
                    format!(
                        "API key not found for provider {provider}. Set {} or {}.",
                        self.llm.api_key_env,
                        provider.default_env_var()
                    )
                })?;
            return Ok((provider, key));
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
        let path = Path::new("/tmp/tamagotchi_test_nonexistent_config.toml");
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
        let env_name = "TAMAGOTCHI_TEST_API_KEY_RESOLVE";
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
        assert!(!cfg.enabled);
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
        assert!(data.to_string_lossy().ends_with(".tamagotchi"));

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
        assert_eq!(db, data.join("tamagotchi.db"));

        let soul = Config::soul_path().unwrap();
        assert_eq!(soul, data.join("SOUL.md"));

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
enabled = true
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(cfg.tasks.enabled);
        assert_eq!(cfg.tasks.max_concurrent, 3);
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
}
