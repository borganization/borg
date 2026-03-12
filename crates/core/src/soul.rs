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
