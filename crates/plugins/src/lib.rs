pub mod catalog;
pub mod installer;
pub mod keychain;
pub mod verifier;

use serde::{Deserialize, Serialize};

/// The kind of plugin — determines where files are installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginKind {
    Channel,
    Tool,
}

impl std::fmt::Display for PluginKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Channel => write!(f, "channel"),
            Self::Tool => write!(f, "tool"),
        }
    }
}

/// Integration category for grouping in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    Messaging,
    Email,
    Productivity,
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Messaging => write!(f, "MESSAGING"),
            Self::Email => write!(f, "EMAIL"),
            Self::Productivity => write!(f, "PRODUCTIVITY"),
        }
    }
}

/// Target platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    All,
    MacOS,
}

impl Platform {
    /// Returns true if this platform matches the current OS.
    pub fn is_available(&self) -> bool {
        match self {
            Self::All => true,
            Self::MacOS => cfg!(target_os = "macos"),
        }
    }

    pub fn label(&self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::MacOS => Some("macOS only"),
        }
    }
}

/// Specification for a credential required by a plugin.
#[derive(Debug, Clone)]
pub struct CredentialSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub help_url: &'static str,
    pub is_optional: bool,
}

/// A file template to be extracted during installation.
#[derive(Debug, Clone)]
pub struct TemplateFile {
    pub relative_path: &'static str,
    pub content: &'static str,
    pub target: TemplateTarget,
}

/// Where template files are installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateTarget {
    Channels,
    Tools,
}

impl TemplateTarget {
    /// Base directory for this target type under the Borg data directory.
    pub fn base_dir(&self, data_dir: &std::path::Path) -> std::path::PathBuf {
        match self {
            TemplateTarget::Channels => data_dir.join("channels"),
            TemplateTarget::Tools => data_dir.join("tools"),
        }
    }
}

/// Metadata about a credential stored in the OS keychain during installation.
#[derive(Debug, Clone)]
pub struct CredentialEntry {
    pub key: String,
    pub service: String,
    pub account: String,
}

/// Result returned from a successful installation with user-facing notes.
#[derive(Debug, Clone, Default)]
pub struct InstallResult {
    pub notes: Vec<String>,
    pub credential_entries: Vec<CredentialEntry>,
    pub file_hashes: Vec<(String, String)>,
}

/// Events emitted during installation for progress display.
#[derive(Debug, Clone)]
pub enum InstallEvent {
    Starting { id: String, name: String },
    WritingFiles { id: String },
    CredentialPrompt { id: String, label: String },
    CredentialStored { id: String, key: String },
    Complete { id: String },
    Error { id: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- PluginKind --

    #[test]
    fn plugin_kind_display() {
        assert_eq!(PluginKind::Channel.to_string(), "channel");
        assert_eq!(PluginKind::Tool.to_string(), "tool");
    }

    #[test]
    fn plugin_kind_equality() {
        assert_eq!(PluginKind::Channel, PluginKind::Channel);
        assert_ne!(PluginKind::Channel, PluginKind::Tool);
    }

    // -- Category --

    #[test]
    fn category_display() {
        assert_eq!(Category::Messaging.to_string(), "MESSAGING");
        assert_eq!(Category::Email.to_string(), "EMAIL");
        assert_eq!(Category::Productivity.to_string(), "PRODUCTIVITY");
    }

    // -- Platform --

    #[test]
    fn platform_all_is_always_available() {
        assert!(Platform::All.is_available());
    }

    #[test]
    fn platform_all_has_no_label() {
        assert!(Platform::All.label().is_none());
    }

    #[test]
    fn platform_macos_has_label() {
        assert_eq!(Platform::MacOS.label(), Some("macOS only"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn platform_macos_available_on_macos() {
        assert!(Platform::MacOS.is_available());
    }

    // -- TemplateTarget --

    #[test]
    fn template_target_base_dir_channels() {
        let dir = std::path::Path::new("/home/user/.borg");
        assert_eq!(
            TemplateTarget::Channels.base_dir(dir),
            std::path::PathBuf::from("/home/user/.borg/channels")
        );
    }

    #[test]
    fn template_target_base_dir_tools() {
        let dir = std::path::Path::new("/home/user/.borg");
        assert_eq!(
            TemplateTarget::Tools.base_dir(dir),
            std::path::PathBuf::from("/home/user/.borg/tools")
        );
    }

    // -- InstallResult --

    #[test]
    fn install_result_default_is_empty() {
        let result = InstallResult::default();
        assert!(result.notes.is_empty());
        assert!(result.credential_entries.is_empty());
        assert!(result.file_hashes.is_empty());
    }

    // -- CredentialSpec --

    #[test]
    fn credential_spec_fields() {
        let spec = CredentialSpec {
            key: "SLACK_TOKEN",
            label: "Slack Bot Token",
            help_url: "https://api.slack.com",
            is_optional: false,
        };
        assert_eq!(spec.key, "SLACK_TOKEN");
        assert!(!spec.is_optional);
    }

    // -- Serde roundtrip --

    #[test]
    fn plugin_kind_serde_roundtrip() {
        let json = serde_json::to_string(&PluginKind::Channel).unwrap();
        let deserialized: PluginKind = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, PluginKind::Channel);
    }

    #[test]
    fn category_serde_roundtrip() {
        let json = serde_json::to_string(&Category::Email).unwrap();
        let deserialized: Category = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Category::Email);
    }
}
