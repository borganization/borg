use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelManifest {
    pub name: String,
    pub description: String,
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default)]
    pub scripts: ScriptsSection,
    #[serde(default)]
    pub sandbox: SandboxSection,
    #[serde(default)]
    pub auth: AuthSection,
    #[serde(default)]
    pub settings: SettingsSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptsSection {
    #[serde(default = "default_inbound")]
    pub inbound: String,
    #[serde(default = "default_outbound")]
    pub outbound: String,
    #[serde(default)]
    pub verify: Option<String>,
}

impl Default for ScriptsSection {
    fn default() -> Self {
        Self {
            inbound: default_inbound(),
            outbound: default_outbound(),
            verify: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxSection {
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub fs_read: Vec<String>,
    #[serde(default)]
    pub fs_write: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthSection {
    pub secret_env: Option<String>,
    pub token_env: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSection {
    pub webhook_path: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
}

impl Default for SettingsSection {
    fn default() -> Self {
        Self {
            webhook_path: None,
            timeout_ms: default_timeout(),
            max_concurrent: default_max_concurrent(),
        }
    }
}

fn default_runtime() -> String {
    "python".to_string()
}
fn default_inbound() -> String {
    "parse_inbound.py".to_string()
}
fn default_outbound() -> String {
    "send_outbound.py".to_string()
}
fn default_timeout() -> u64 {
    15000
}
fn default_max_concurrent() -> usize {
    5
}

impl ChannelManifest {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let manifest: Self = toml::from_str(&content)?;
        Ok(manifest)
    }

    pub fn webhook_path(&self) -> String {
        self.settings
            .webhook_path
            .clone()
            .unwrap_or_else(|| format!("/webhook/{}", self.name))
    }

    pub fn sandbox_policy(&self) -> tamagotchi_sandbox::policy::SandboxPolicy {
        tamagotchi_sandbox::policy::SandboxPolicy {
            network: self.sandbox.network,
            fs_read: self.sandbox.fs_read.clone(),
            fs_write: self.sandbox.fs_write.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_TOML: &str = r#"
name = "my-slack"
description = "Slack workspace integration"
runtime = "python"

[scripts]
inbound = "parse_inbound.py"
outbound = "send_outbound.py"
verify = "verify.py"

[sandbox]
network = true
fs_read = ["/etc/ssl"]
fs_write = []

[auth]
secret_env = "SLACK_SIGNING_SECRET"
token_env = "SLACK_BOT_TOKEN"

[settings]
webhook_path = "/webhook/my-slack"
timeout_ms = 15000
max_concurrent = 5
"#;

    #[test]
    fn parse_full_manifest() {
        let manifest: ChannelManifest = toml::from_str(FULL_TOML).unwrap();
        assert_eq!(manifest.name, "my-slack");
        assert_eq!(manifest.description, "Slack workspace integration");
        assert_eq!(manifest.runtime, "python");
        assert_eq!(manifest.scripts.inbound, "parse_inbound.py");
        assert_eq!(manifest.scripts.outbound, "send_outbound.py");
        assert_eq!(manifest.scripts.verify.as_deref(), Some("verify.py"));
        assert!(manifest.sandbox.network);
        assert_eq!(
            manifest.auth.secret_env.as_deref(),
            Some("SLACK_SIGNING_SECRET")
        );
        assert_eq!(manifest.auth.token_env.as_deref(), Some("SLACK_BOT_TOKEN"));
        assert_eq!(manifest.settings.timeout_ms, 15000);
        assert_eq!(manifest.settings.max_concurrent, 5);
    }

    #[test]
    fn parse_minimal_manifest() {
        let toml_str = r#"
name = "test-channel"
description = "A test channel"
"#;
        let manifest: ChannelManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "test-channel");
        assert_eq!(manifest.runtime, "python");
        assert_eq!(manifest.scripts.inbound, "parse_inbound.py");
        assert_eq!(manifest.scripts.outbound, "send_outbound.py");
        assert!(manifest.scripts.verify.is_none());
        assert!(!manifest.sandbox.network);
        assert_eq!(manifest.settings.timeout_ms, 15000);
        assert_eq!(manifest.settings.max_concurrent, 5);
    }

    #[test]
    fn webhook_path_default() {
        let toml_str = r#"
name = "discord"
description = "Discord bot"
"#;
        let manifest: ChannelManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.webhook_path(), "/webhook/discord");
    }

    #[test]
    fn webhook_path_custom() {
        let manifest: ChannelManifest = toml::from_str(FULL_TOML).unwrap();
        assert_eq!(manifest.webhook_path(), "/webhook/my-slack");
    }

    #[test]
    fn sandbox_policy_conversion() {
        let manifest: ChannelManifest = toml::from_str(FULL_TOML).unwrap();
        let policy = manifest.sandbox_policy();
        assert!(policy.network);
        assert_eq!(policy.fs_read, vec!["/etc/ssl"]);
        assert!(policy.fs_write.is_empty());
    }

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("channel.toml");
        std::fs::write(&path, FULL_TOML).unwrap();
        let manifest = ChannelManifest::load(&path).unwrap();
        assert_eq!(manifest.name, "my-slack");
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let result = ChannelManifest::load(std::path::Path::new("/tmp/nonexistent_channel.toml"));
        assert!(result.is_err());
    }
}
