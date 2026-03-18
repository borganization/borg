use anyhow::Result;
use std::path::PathBuf;
use tracing::debug;

use crate::config::Config;

const DEFAULT_IDENTITY: &str = r#"# Borg — Your AI Personal Assistant

You are a helpful, friendly AI personal assistant. You live on your owner's computer and help them with tasks, remember things for them, and occasionally check in to see how they're doing.

## Personality
- Warm but not overbearing
- Proactive when you notice something useful
- Honest about your limitations
- You remember context from past conversations

## Capabilities
- You can create and use tools (scripts) to extend your abilities
- You can remember information across sessions
- You can check in proactively via the heartbeat system
- You can read and modify files in your tools directory

## Guidelines
- Keep responses concise unless asked for detail
- When creating tools, prefer simple implementations
- Always explain what you're doing before executing tools
- Respect the user's time and attention
"#;

pub fn identity_path() -> Result<PathBuf> {
    Config::identity_path()
}

pub fn load_identity() -> Result<String> {
    load_identity_with_override(None)
}

/// Load identity, optionally from an override path.
/// Used by gateway routing to support per-binding identity files.
pub fn load_identity_with_override(override_path: Option<&std::path::Path>) -> Result<String> {
    let path = if let Some(p) = override_path {
        p.to_path_buf()
    } else {
        identity_path()?
    };

    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        debug!(
            "Loaded identity from {} ({} bytes)",
            path.display(),
            content.len()
        );
        Ok(content)
    } else {
        debug!(
            "No identity file found at {}, using default",
            path.display()
        );
        Ok(DEFAULT_IDENTITY.to_string())
    }
}

pub fn save_identity(content: &str) -> Result<()> {
    let path = identity_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    debug!("Saved IDENTITY.md ({} bytes)", content.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_identity_is_non_empty() {
        assert!(!DEFAULT_IDENTITY.is_empty());
    }

    #[test]
    fn default_identity_contains_personality() {
        assert!(DEFAULT_IDENTITY.contains("Personality"));
        assert!(DEFAULT_IDENTITY.contains("Capabilities"));
        assert!(DEFAULT_IDENTITY.contains("Guidelines"));
    }

    #[test]
    fn default_identity_contains_identity() {
        assert!(DEFAULT_IDENTITY.contains("Borg"));
        assert!(DEFAULT_IDENTITY.contains("AI"));
    }

    #[test]
    fn identity_path_ends_with_identity_md() {
        let path = identity_path().unwrap();
        assert!(path.to_string_lossy().ends_with("IDENTITY.md"));
        assert!(path.to_string_lossy().contains(".borg"));
    }

    #[test]
    fn load_identity_returns_content() {
        // Should return either the file content or the default
        let identity = load_identity().unwrap();
        assert!(!identity.is_empty());
        // Must contain some personality-related content
        assert!(identity.contains("Borg") || identity.contains("AI"));
    }

    #[test]
    fn save_and_load_identity_round_trip() {
        // Save original, modify, restore
        let original = load_identity().unwrap();

        let test_content = "# Test Identity\nThis is a test personality.";
        save_identity(test_content).unwrap();

        let loaded = load_identity().unwrap();
        assert_eq!(loaded, test_content);

        // Restore original
        save_identity(&original).unwrap();
    }
}
