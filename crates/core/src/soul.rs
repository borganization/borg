use anyhow::Result;
use std::path::PathBuf;
use tracing::debug;

use crate::config::Config;

const DEFAULT_SOUL: &str = r#"# Tamagotchi — Your AI Personal Assistant

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

pub fn soul_path() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("SOUL.md"))
}

pub fn load_soul() -> Result<String> {
    let path = soul_path()?;
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        debug!("Loaded SOUL.md ({} bytes)", content.len());
        Ok(content)
    } else {
        debug!("No SOUL.md found, using default");
        Ok(DEFAULT_SOUL.to_string())
    }
}

pub fn save_soul(content: &str) -> Result<()> {
    let path = soul_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    debug!("Saved SOUL.md ({} bytes)", content.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_soul_is_non_empty() {
        assert!(!DEFAULT_SOUL.is_empty());
    }

    #[test]
    fn default_soul_contains_personality() {
        assert!(DEFAULT_SOUL.contains("Personality"));
        assert!(DEFAULT_SOUL.contains("Capabilities"));
        assert!(DEFAULT_SOUL.contains("Guidelines"));
    }

    #[test]
    fn default_soul_contains_identity() {
        assert!(DEFAULT_SOUL.contains("Tamagotchi"));
        assert!(DEFAULT_SOUL.contains("AI"));
    }

    #[test]
    fn soul_path_ends_with_soul_md() {
        let path = soul_path().unwrap();
        assert!(path.to_string_lossy().ends_with("SOUL.md"));
        assert!(path.to_string_lossy().contains(".tamagotchi"));
    }

    #[test]
    fn load_soul_returns_content() {
        // Should return either the file content or the default
        let soul = load_soul().unwrap();
        assert!(!soul.is_empty());
        // Must contain some personality-related content
        assert!(soul.contains("Tamagotchi") || soul.contains("AI"));
    }

    #[test]
    fn save_and_load_soul_round_trip() {
        // Save original, modify, restore
        let original = load_soul().unwrap();

        let test_content = "# Test Soul\nThis is a test personality.";
        save_soul(test_content).unwrap();

        let loaded = load_soul().unwrap();
        assert_eq!(loaded, test_content);

        // Restore original
        save_soul(&original).unwrap();
    }
}
