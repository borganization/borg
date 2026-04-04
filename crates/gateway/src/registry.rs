use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

use crate::manifest::ChannelManifest;

// --- Generic ManifestRegistry ---

/// Trait for manifest types that can be used with `ManifestRegistry`.
pub trait ManifestItem: Sized {
    fn load(path: &Path) -> Result<Self>;
    fn item_name(&self) -> &str;
    const MANIFEST_FILENAME: &'static str;
    const SUBDIR: &'static str;
    const ITEM_TYPE: &'static str;
}

#[derive(Clone)]
pub struct RegisteredItem<M: Clone> {
    pub manifest: M,
    pub dir: PathBuf,
}

pub struct ManifestRegistry<M: Clone> {
    items: HashMap<String, RegisteredItem<M>>,
    base_dir: PathBuf,
}

impl<M: ManifestItem + Clone> ManifestRegistry<M> {
    pub fn new() -> Result<Self> {
        let base_dir = std::env::var("BORG_DATA_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))
                    .map(|h| h.join(".borg"))
            })?
            .join(M::SUBDIR);

        let mut registry = Self {
            items: HashMap::new(),
            base_dir,
        };

        registry.scan()?;
        Ok(registry)
    }

    pub fn with_dir(dir: PathBuf) -> Result<Self> {
        let mut registry = Self {
            items: HashMap::new(),
            base_dir: dir,
        };

        registry.scan()?;
        Ok(registry)
    }

    pub fn scan(&mut self) -> Result<()> {
        self.items.clear();

        let scanned = scan_manifest_dir(
            &self.base_dir,
            M::MANIFEST_FILENAME,
            M::load,
            |m| m.item_name().to_string(),
            M::ITEM_TYPE,
        )?;

        for (name, (manifest, dir)) in scanned {
            self.items.insert(name, RegisteredItem { manifest, dir });
        }

        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&RegisteredItem<M>> {
        self.items.get(name)
    }

    pub fn items(&self) -> impl Iterator<Item = &RegisteredItem<M>> {
        self.items.values()
    }
}

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

/// Generic directory scanner for manifest-based registries.
fn scan_manifest_dir<M, F, N>(
    dir: &Path,
    manifest_filename: &str,
    load_fn: F,
    name_fn: N,
    item_type: &str,
) -> Result<HashMap<String, (M, PathBuf)>>
where
    F: Fn(&Path) -> Result<M>,
    N: Fn(&M) -> String,
{
    let mut items = HashMap::new();

    if !dir.exists() {
        debug!("{item_type} directory does not exist: {}", dir.display());
        return Ok(items);
    }

    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Skip symlinks to prevent registering items from unexpected locations
        if path.is_symlink() {
            warn!(
                "Skipping symlinked {item_type} directory: {}",
                path.display()
            );
            continue;
        }

        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join(manifest_filename);
        if !manifest_path.exists() {
            continue;
        }

        match load_fn(&manifest_path) {
            Ok(manifest) => {
                let name = name_fn(&manifest);
                debug!("Registered {item_type}: {name} from {}", path.display());
                items.insert(name, (manifest, path));
            }
            Err(e) => {
                warn!(
                    "Failed to load {item_type} manifest {}: {e}",
                    manifest_path.display()
                );
            }
        }
    }

    debug!("Loaded {} {item_type}(s)", items.len());
    Ok(items)
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
