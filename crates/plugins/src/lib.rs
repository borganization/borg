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
