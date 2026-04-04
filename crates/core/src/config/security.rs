use serde::{Deserialize, Serialize};

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
            subagent_deny: vec!["schedule".to_string(), "browser".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub mode: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "strict".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub secret_detection: bool,
    pub blocked_paths: Vec<String>,
    pub host_audit: bool,
    #[serde(default)]
    pub action_limits: crate::rate_guard::ActionLimits,
    #[serde(default = "default_gateway_action_limits")]
    pub gateway_action_limits: crate::rate_guard::ActionLimits,
}

fn default_gateway_action_limits() -> crate::rate_guard::ActionLimits {
    crate::rate_guard::ActionLimits::gateway_defaults()
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
            action_limits: crate::rate_guard::ActionLimits::default(),
            gateway_action_limits: crate::rate_guard::ActionLimits::gateway_defaults(),
        }
    }
}
