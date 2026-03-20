use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::manifest::ChannelManifest;
use borg_tools::{ManifestItem, ManifestRegistry, RegisteredItem};

// --- ManifestItem impl for ChannelManifest ---

impl ManifestItem for ChannelManifest {
    fn load(path: &Path) -> Result<Self> {
        ChannelManifest::load(path)
    }
    fn item_name(&self) -> &str {
        &self.name
    }
    const MANIFEST_FILENAME: &'static str = "channel.toml";
    const SUBDIR: &'static str = "channels";
    const ITEM_TYPE: &'static str = "channel";
}

/// Legacy type alias for backward compatibility.
pub type RegisteredChannel = RegisteredItem<ChannelManifest>;

pub struct ChannelRegistry {
    inner: ManifestRegistry<ChannelManifest>,
}

impl ChannelRegistry {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: ManifestRegistry::new()?,
        })
    }

    pub fn with_dir(dir: PathBuf) -> Result<Self> {
        Ok(Self {
            inner: ManifestRegistry::with_dir(dir)?,
        })
    }

    pub fn scan(&mut self) -> Result<()> {
        self.inner.scan()
    }

    pub fn get(&self, name: &str) -> Option<&RegisteredChannel> {
        self.inner.get(name)
    }

    pub fn list_channels(&self) -> Vec<String> {
        self.inner
            .items()
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
        self.inner.items()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_lists_nothing() {
        let registry =
            ChannelRegistry::with_dir(PathBuf::from("/tmp/nonexistent_channels_dir")).unwrap();
        assert!(registry.list_channels().is_empty());
        assert!(registry.get("anything").is_none());
    }

    #[test]
    fn scan_nonexistent_dir_succeeds() {
        let mut registry =
            ChannelRegistry::with_dir(PathBuf::from("/tmp/nonexistent_channels_dir_xyz")).unwrap();
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

        let mut registry = ChannelRegistry::with_dir(dir.path().to_path_buf()).unwrap();
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
