use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Generic directory scanner for manifest-based registries.
///
/// Scans `dir` for subdirectories containing `manifest_filename`,
/// loads each manifest using `load_fn`, and returns a map of name to (manifest, dir).
pub fn scan_manifest_dir<M, F, N>(
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
    fn scan_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = scan_manifest_dir::<String, _, _>(
            dir.path(),
            "manifest.toml",
            |_| Ok("test".to_string()),
            |m| m.clone(),
            "test",
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn scan_nonexistent_dir() {
        let result = scan_manifest_dir::<String, _, _>(
            Path::new("/tmp/nonexistent_scan_dir_xyz"),
            "manifest.toml",
            |_| Ok("test".to_string()),
            |m| m.clone(),
            "test",
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn scan_finds_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("item1");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("manifest.toml"), "name = \"item1\"").unwrap();

        let result = scan_manifest_dir(
            dir.path(),
            "manifest.toml",
            |path| {
                let content = std::fs::read_to_string(path)?;
                Ok(content)
            },
            |content| content.lines().next().unwrap_or("unknown").to_string(),
            "test",
        )
        .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn scan_skips_files_and_missing_manifests() {
        let dir = tempfile::tempdir().unwrap();
        // A file, not a directory
        std::fs::write(dir.path().join("not_a_dir.txt"), "data").unwrap();
        // A directory without a manifest
        std::fs::create_dir_all(dir.path().join("no-manifest")).unwrap();

        let result = scan_manifest_dir::<String, _, _>(
            dir.path(),
            "manifest.toml",
            |_| Ok("test".to_string()),
            |m| m.clone(),
            "test",
        )
        .unwrap();
        assert!(result.is_empty());
    }
}
