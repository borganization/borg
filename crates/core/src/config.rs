use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use std::str::FromStr;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_initial_retry_delay_ms")]
    pub initial_retry_delay_ms: u64,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval")]
    pub interval: String,
    #[serde(default)]
    pub quiet_hours_start: Option<String>,
    #[serde(default)]
    pub quiet_hours_end: Option<String>,
    #[serde(default)]
    pub cron: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_timeout")]
    pub default_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_sandbox_mode")]
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_skills_max_tokens")]
    pub max_context_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationConfig {
    #[serde(default = "default_max_history_tokens")]
    pub max_history_tokens: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
}

fn default_api_key_env() -> String {
    "OPENROUTER_API_KEY".to_string()
}
fn default_model() -> String {
    "anthropic/claude-sonnet-4".to_string()
}
fn default_temperature() -> f32 {
    0.7
}
fn default_max_tokens() -> u32 {
    4096
}
fn default_max_retries() -> u32 {
    3
}
fn default_initial_retry_delay_ms() -> u64 {
    200
}
fn default_request_timeout_ms() -> u64 {
    60000
}
fn default_interval() -> String {
    "30m".to_string()
}
fn default_timeout() -> u64 {
    30000
}
fn default_true() -> bool {
    true
}
fn default_sandbox_mode() -> String {
    "strict".to_string()
}
fn default_max_context_tokens() -> usize {
    8000
}
fn default_skills_max_tokens() -> usize {
    4000
}
fn default_max_history_tokens() -> usize {
    32000
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: None,
            api_key_env: default_api_key_env(),
            model: default_model(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            max_retries: default_max_retries(),
            initial_retry_delay_ms: default_initial_retry_delay_ms(),
            request_timeout_ms: default_request_timeout_ms(),
        }
    }
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: default_interval(),
            quiet_hours_start: None,
            quiet_hours_end: None,
            cron: None,
        }
    }
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: default_timeout(),
        }
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            mode: default_sandbox_mode(),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: default_max_context_tokens(),
        }
    }
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            max_context_tokens: default_skills_max_tokens(),
        }
    }
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            max_history_tokens: default_max_history_tokens(),
        }
    }
}

impl Config {
    pub fn data_dir() -> Result<PathBuf> {
        Ok(dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".tamagotchi"))
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
             skills.max_context_tokens = {}",
            self.llm.model,
            self.llm.temperature,
            self.llm.max_tokens,
            self.sandbox.enabled,
            self.sandbox.mode,
            self.memory.max_context_tokens,
            self.skills.enabled,
            self.skills.max_context_tokens,
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
            _ => anyhow::bail!(
                "Unknown setting: {key}\nAvailable: model, temperature, max_tokens, provider, \
                 sandbox.mode, sandbox.enabled, memory.max_context_tokens, skills.enabled"
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
    ///
    /// Priority:
    /// 1. Explicit `provider` in config → use that provider, resolve key from `api_key_env` or provider default
    /// 2. If `api_key_env` is set to a non-default value → try it, auto-detect provider from the value
    /// 3. Auto-detect from environment variables in priority order
    pub fn resolve_provider(&self) -> Result<(Provider, String)> {
        // Case 1: Explicit provider in config
        if let Some(ref provider_str) = self.llm.provider {
            let provider = Provider::from_str(provider_str)?;
            // Try api_key_env first, then the provider's default env var
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

        // Case 2: api_key_env is set to a non-default value — honor it
        if self.llm.api_key_env != default_api_key_env() {
            if let Ok(key) = std::env::var(&self.llm.api_key_env) {
                if !key.is_empty() {
                    // Try to guess the provider from the env var name
                    let provider = match self.llm.api_key_env.as_str() {
                        "OPENAI_API_KEY" => Provider::OpenAi,
                        "ANTHROPIC_API_KEY" => Provider::Anthropic,
                        "GEMINI_API_KEY" => Provider::Gemini,
                        _ => Provider::OpenRouter, // fallback: treat custom key as OpenRouter-compatible
                    };
                    return Ok((provider, key));
                }
            }
        }

        // Case 3: Auto-detect from env
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

        // LLM defaults
        assert!(cfg.llm.provider.is_none());
        assert_eq!(cfg.llm.api_key_env, "OPENROUTER_API_KEY");
        assert_eq!(cfg.llm.model, "anthropic/claude-sonnet-4");
        assert!((cfg.llm.temperature - 0.7).abs() < f32::EPSILON);
        assert_eq!(cfg.llm.max_tokens, 4096);

        // Heartbeat defaults
        assert!(!cfg.heartbeat.enabled);
        assert_eq!(cfg.heartbeat.interval, "30m");
        assert!(cfg.heartbeat.quiet_hours_start.is_none());
        assert!(cfg.heartbeat.quiet_hours_end.is_none());
        assert!(cfg.heartbeat.cron.is_none());

        // Tools defaults
        assert_eq!(cfg.tools.default_timeout_ms, 30000);

        // Sandbox defaults
        assert!(cfg.sandbox.enabled);
        assert_eq!(cfg.sandbox.mode, "strict");

        // Memory defaults
        assert_eq!(cfg.memory.max_context_tokens, 8000);

        // Skills defaults
        assert!(cfg.skills.enabled);
        assert_eq!(cfg.skills.max_context_tokens, 4000);

        // Conversation defaults
        assert_eq!(cfg.conversation.max_history_tokens, 32000);

        // User defaults
        assert!(cfg.user.name.is_none());
        assert!(cfg.user.agent_name.is_none());
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
"#;

        let cfg: Config = toml::from_str(toml_str).expect("should parse complete toml");

        assert_eq!(cfg.llm.api_key_env, "MY_KEY");
        assert_eq!(cfg.llm.model, "openai/gpt-4");
        assert!((cfg.llm.temperature - 0.9).abs() < f32::EPSILON);
        assert_eq!(cfg.llm.max_tokens, 2048);

        assert!(cfg.heartbeat.enabled);
        assert_eq!(cfg.heartbeat.interval, "1h");
        assert_eq!(cfg.heartbeat.quiet_hours_start.as_deref(), Some("22:00"));
        assert_eq!(cfg.heartbeat.quiet_hours_end.as_deref(), Some("08:00"));
        assert_eq!(cfg.heartbeat.cron.as_deref(), Some("0 */2 * * *"));

        assert_eq!(cfg.tools.default_timeout_ms, 60000);

        assert!(!cfg.sandbox.enabled);
        assert_eq!(cfg.sandbox.mode, "permissive");

        assert_eq!(cfg.memory.max_context_tokens, 16000);

        assert_eq!(cfg.conversation.max_history_tokens, 64000);
    }

    #[test]
    fn parse_empty_config_toml_yields_defaults() {
        let cfg: Config = toml::from_str("").expect("should parse empty toml");

        // All fields should fall back to their defaults
        let defaults = Config::default();
        assert_eq!(cfg.llm.api_key_env, defaults.llm.api_key_env);
        assert_eq!(cfg.llm.model, defaults.llm.model);
        assert!((cfg.llm.temperature - defaults.llm.temperature).abs() < f32::EPSILON);
        assert_eq!(cfg.llm.max_tokens, defaults.llm.max_tokens);
        assert_eq!(cfg.heartbeat.enabled, defaults.heartbeat.enabled);
        assert_eq!(cfg.heartbeat.interval, defaults.heartbeat.interval);
        assert_eq!(
            cfg.tools.default_timeout_ms,
            defaults.tools.default_timeout_ms
        );
        assert_eq!(cfg.sandbox.enabled, defaults.sandbox.enabled);
        assert_eq!(cfg.sandbox.mode, defaults.sandbox.mode);
        assert_eq!(
            cfg.memory.max_context_tokens,
            defaults.memory.max_context_tokens
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
        // Other llm fields keep defaults
        assert_eq!(cfg.llm.api_key_env, "OPENROUTER_API_KEY");
        assert_eq!(cfg.llm.max_tokens, 4096);
        // Other sections keep defaults
        assert!(!cfg.heartbeat.enabled);
        assert!(cfg.sandbox.enabled);
    }

    #[test]
    fn load_from_nonexistent_path_returns_defaults() {
        let path = Path::new("/tmp/tamagotchi_test_nonexistent_config.toml");
        let cfg = Config::load_from(path).expect("should return default for missing file");
        let defaults = Config::default();
        assert_eq!(cfg.llm.model, defaults.llm.model);
        assert_eq!(cfg.sandbox.mode, defaults.sandbox.mode);
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
        // Defaults for unspecified fields
        assert_eq!(cfg.llm.api_key_env, "OPENROUTER_API_KEY");
    }

    #[test]
    fn api_key_resolved_from_env() {
        let cfg = Config::default();
        // Use a unique env var name to avoid collisions in parallel tests
        let env_name = "TAMAGOTCHI_TEST_API_KEY_RESOLVE";
        let mut cfg = cfg;
        cfg.llm.api_key_env = env_name.to_string();

        // Should fail when not set
        std::env::remove_var(env_name);
        assert!(cfg.api_key().is_err());

        // Should succeed when set
        std::env::set_var(env_name, "sk-test-12345");
        let key = cfg.api_key().expect("should resolve key from env");
        assert_eq!(key, "sk-test-12345");

        // Cleanup
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
    fn parse_config_without_user_section_yields_none() {
        let cfg: Config = toml::from_str("").expect("should parse empty toml");
        assert!(cfg.user.name.is_none());
        assert!(cfg.user.agent_name.is_none());
    }

    #[test]
    fn parse_config_with_provider_field() {
        let toml_str = r#"
[llm]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.llm.provider.as_deref(), Some("anthropic"));
        assert_eq!(cfg.llm.model, "claude-sonnet-4");
    }

    #[test]
    fn parse_config_without_provider_field() {
        let toml_str = r#"
[llm]
api_key_env = "OPENROUTER_API_KEY"
model = "anthropic/claude-sonnet-4"
"#;
        let cfg: Config = toml::from_str(toml_str).expect("should parse");
        assert!(cfg.llm.provider.is_none());
    }

    #[test]
    fn resolve_provider_explicit_uses_custom_env_var() {
        // Explicit provider resolves key from api_key_env without relying on auto-detect
        let unique_env = "TAMAGOTCHI_TEST_RESOLVE_EXPLICIT";
        std::env::set_var(unique_env, "sk-ant-test");

        let mut cfg = Config::default();
        cfg.llm.provider = Some("anthropic".to_string());
        cfg.llm.api_key_env = unique_env.to_string();

        let (provider, key) = cfg.resolve_provider().unwrap();
        assert_eq!(provider, Provider::Anthropic);
        assert_eq!(key, "sk-ant-test");

        std::env::remove_var(unique_env);
    }

    #[test]
    fn resolve_provider_explicit_errors_without_key() {
        let unique_env = "TAMAGOTCHI_TEST_RESOLVE_MISSING";
        std::env::remove_var(unique_env);

        let mut cfg = Config::default();
        cfg.llm.provider = Some("openai".to_string());
        cfg.llm.api_key_env = unique_env.to_string();

        // Also ensure the default OPENAI_API_KEY is not set (best effort)
        let saved = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");

        let result = cfg.resolve_provider();
        // Should error since neither custom env nor provider default is set
        // (may succeed if OPENAI_API_KEY happens to be set by another test)
        if saved.is_none() {
            assert!(result.is_err());
        }

        if let Some(val) = saved {
            std::env::set_var("OPENAI_API_KEY", val);
        }
    }

    #[test]
    fn resolve_provider_custom_api_key_env() {
        // When api_key_env is non-default and maps to a known provider
        let unique_env = "TAMAGOTCHI_TEST_CUSTOM_KEY";
        std::env::set_var(unique_env, "sk-custom");

        let mut cfg = Config::default();
        cfg.llm.api_key_env = unique_env.to_string();
        // No explicit provider — falls through to auto-detect path
        // Since api_key_env is not a known provider env var, it won't match case 2
        // It will try auto-detect from env

        // This mainly tests that the code doesn't panic
        let _ = cfg.resolve_provider();

        std::env::remove_var(unique_env);
    }

    #[test]
    fn display_settings_contains_key_fields() {
        let cfg = Config::default();
        let display = cfg.display_settings();
        assert!(display.contains("model"));
        assert!(display.contains("temperature"));
        assert!(display.contains("max_tokens"));
        assert!(display.contains("sandbox.enabled"));
        assert!(display.contains("sandbox.mode"));
        assert!(display.contains("(auto-detect)"));
    }

    #[test]
    fn display_settings_shows_explicit_provider() {
        let mut cfg = Config::default();
        cfg.llm.provider = Some("anthropic".to_string());
        let display = cfg.display_settings();
        assert!(display.contains("anthropic"));
        assert!(!display.contains("(auto-detect)"));
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
        let result = cfg.apply_setting("temperature", "0.5").unwrap();
        assert!(result.contains("0.5"));
        assert!((cfg.llm.temperature - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn apply_setting_temperature_out_of_range() {
        let mut cfg = Config::default();
        assert!(cfg.apply_setting("temperature", "3.0").is_err());
        assert!(cfg.apply_setting("temperature", "-1.0").is_err());
    }

    #[test]
    fn apply_setting_temperature_invalid() {
        let mut cfg = Config::default();
        assert!(cfg.apply_setting("temperature", "not_a_number").is_err());
    }

    #[test]
    fn apply_setting_max_tokens() {
        let mut cfg = Config::default();
        cfg.apply_setting("max_tokens", "8192").unwrap();
        assert_eq!(cfg.llm.max_tokens, 8192);
    }

    #[test]
    fn apply_setting_provider() {
        let mut cfg = Config::default();
        cfg.apply_setting("provider", "openai").unwrap();
        assert_eq!(cfg.llm.provider.as_deref(), Some("openai"));
    }

    #[test]
    fn apply_setting_sandbox_mode() {
        let mut cfg = Config::default();
        cfg.apply_setting("sandbox.mode", "permissive").unwrap();
        assert_eq!(cfg.sandbox.mode, "permissive");
    }

    #[test]
    fn apply_setting_sandbox_enabled() {
        let mut cfg = Config::default();
        cfg.apply_setting("sandbox.enabled", "false").unwrap();
        assert!(!cfg.sandbox.enabled);
    }

    #[test]
    fn apply_setting_memory_max_context_tokens() {
        let mut cfg = Config::default();
        cfg.apply_setting("memory.max_context_tokens", "16000")
            .unwrap();
        assert_eq!(cfg.memory.max_context_tokens, 16000);
    }

    #[test]
    fn apply_setting_skills_enabled() {
        let mut cfg = Config::default();
        cfg.apply_setting("skills.enabled", "false").unwrap();
        assert!(!cfg.skills.enabled);
    }

    #[test]
    fn apply_setting_unknown_key_errors() {
        let mut cfg = Config::default();
        assert!(cfg.apply_setting("nonexistent", "value").is_err());
    }

    #[test]
    fn save_and_reload_round_trip() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config_path = dir.path().join("config.toml");

        let mut cfg = Config::default();
        cfg.llm.model = "test-round-trip".to_string();
        cfg.llm.temperature = 1.5;
        cfg.llm.max_tokens = 2048;

        // Save manually to the temp path (can't use save() since it writes to ~/.tamagotchi)
        let content = toml::to_string_pretty(&cfg).expect("serialize");
        std::fs::write(&config_path, content).expect("write");

        let loaded = Config::load_from(&config_path).expect("reload");
        assert_eq!(loaded.llm.model, "test-round-trip");
        assert!((loaded.llm.temperature - 1.5).abs() < f32::EPSILON);
        assert_eq!(loaded.llm.max_tokens, 2048);
    }
}
