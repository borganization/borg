use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
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
            api_key_env: default_api_key_env(),
            model: default_model(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
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

    pub fn api_key(&self) -> Result<String> {
        std::env::var(&self.llm.api_key_env).with_context(|| {
            format!(
                "API key not found. Set the {} environment variable.",
                self.llm.api_key_env
            )
        })
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
}
