use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, warn};

use crate::manifest::ChannelManifest;

pub struct ChannelRegistry {
    channels: HashMap<String, RegisteredChannel>,
    channels_dir: PathBuf,
}

pub struct RegisteredChannel {
    pub manifest: ChannelManifest,
    pub dir: PathBuf,
}

impl ChannelRegistry {
    pub fn new() -> Result<Self> {
        let channels_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".tamagotchi")
            .join("channels");

        let mut registry = Self {
            channels: HashMap::new(),
            channels_dir,
        };

        registry.scan()?;
        Ok(registry)
    }

    pub fn with_dir(dir: PathBuf) -> Result<Self> {
        let mut registry = Self {
            channels: HashMap::new(),
            channels_dir: dir,
        };

        registry.scan()?;
        Ok(registry)
    }

    pub fn scan(&mut self) -> Result<()> {
        self.channels.clear();

        if !self.channels_dir.exists() {
            debug!(
                "Channels directory does not exist: {}",
                self.channels_dir.display()
            );
            return Ok(());
        }

        let entries = std::fs::read_dir(&self.channels_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_symlink() {
                warn!("Skipping symlinked channel directory: {}", path.display());
                continue;
            }

            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("channel.toml");
            if !manifest_path.exists() {
                continue;
            }

            match ChannelManifest::load(&manifest_path) {
                Ok(manifest) => {
                    debug!(
                        "Registered channel: {} from {}",
                        manifest.name,
                        path.display()
                    );
                    let name = manifest.name.clone();
                    self.channels.insert(
                        name,
                        RegisteredChannel {
                            manifest,
                            dir: path,
                        },
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to load channel manifest {}: {e}",
                        manifest_path.display()
                    );
                }
            }
        }

        debug!("Loaded {} channels", self.channels.len());
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&RegisteredChannel> {
        self.channels.get(name)
    }

    pub fn list_channels(&self) -> Vec<String> {
        self.channels
            .values()
            .map(|c| {
                format!(
                    "{}: {} (webhook: {})",
                    c.manifest.name,
                    c.manifest.description,
                    c.manifest.webhook_path()
                )
            })
            .collect()
    }

    pub fn all_channels(&self) -> impl Iterator<Item = &RegisteredChannel> {
        self.channels.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_lists_nothing() {
        let registry = ChannelRegistry {
            channels: HashMap::new(),
            channels_dir: PathBuf::from("/tmp/nonexistent_channels_dir"),
        };
        assert!(registry.list_channels().is_empty());
        assert!(registry.get("anything").is_none());
    }

    #[test]
    fn scan_nonexistent_dir_succeeds() {
        let mut registry = ChannelRegistry {
            channels: HashMap::new(),
            channels_dir: PathBuf::from("/tmp/nonexistent_channels_dir_xyz"),
        };
        assert!(registry.scan().is_ok());
        assert!(registry.list_channels().is_empty());
    }

    #[test]
    fn scan_dir_with_valid_channel() {
        let dir = tempfile::tempdir().unwrap();
        let channel_dir = dir.path().join("test-channel");
        std::fs::create_dir_all(&channel_dir).unwrap();
        std::fs::write(
            channel_dir.join("channel.toml"),
            r#"
name = "test-channel"
description = "A test channel"
"#,
        )
        .unwrap();

        let mut registry = ChannelRegistry {
            channels: HashMap::new(),
            channels_dir: dir.path().to_path_buf(),
        };
        registry.scan().unwrap();
        assert_eq!(registry.list_channels().len(), 1);
        assert!(registry.get("test-channel").is_some());
    }

    #[test]
    fn scan_valid_channel_via_with_dir() {
        let dir = tempfile::tempdir().unwrap();
        let channel_dir = dir.path().join("my-discord");
        std::fs::create_dir_all(&channel_dir).unwrap();
        std::fs::write(
            channel_dir.join("channel.toml"),
            "name = \"my-discord\"\ndescription = \"Discord integration\"\n",
        )
        .unwrap();

        let registry = ChannelRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        assert_eq!(registry.list_channels().len(), 1);
        assert!(registry.get("my-discord").is_some());
    }

    #[test]
    fn scan_skips_invalid_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let channel_dir = dir.path().join("bad-channel");
        std::fs::create_dir_all(&channel_dir).unwrap();
        std::fs::write(channel_dir.join("channel.toml"), "not valid {{{{").unwrap();

        let registry = ChannelRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        assert!(registry.list_channels().is_empty());
    }

    #[test]
    fn channel_definitions_format() {
        let dir = tempfile::tempdir().unwrap();
        let channel_dir = dir.path().join("slack");
        std::fs::create_dir_all(&channel_dir).unwrap();
        std::fs::write(
            channel_dir.join("channel.toml"),
            "name = \"slack\"\ndescription = \"Slack bot\"\n",
        )
        .unwrap();

        let registry = ChannelRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        let list = registry.list_channels();
        assert_eq!(list.len(), 1);
        assert!(list[0].contains("slack"));
        assert!(list[0].contains("Slack bot"));
        assert!(list[0].contains("/webhook/slack"));
    }
}
