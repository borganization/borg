//! Watches memory directories for .md file changes and triggers re-indexing.
//!
//! Modeled on `config_watcher.rs` but watches multiple directories (global, local, extra)
//! and re-embeds changed files instead of reloading config.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use crate::config::Config;

/// Debounce interval before re-indexing (longer than config watcher since embedding is expensive).
const DEBOUNCE_MS: u64 = 1500;

/// Watches memory directories for .md file changes and spawns re-embedding tasks.
pub struct MemoryWatcher {
    _watchers: Vec<RecommendedWatcher>,
    stop_tx: Option<oneshot::Sender<()>>,
}

impl MemoryWatcher {
    /// Start watching memory directories. Spawns a background tokio task.
    ///
    /// Watches:
    /// - `~/.borg/memory/` (global)
    /// - `$CWD/.borg/memory/` (local, if exists)
    /// - Any `extra_paths` from config
    pub fn start(config: Config) -> Result<Self> {
        let (stop_tx, stop_rx) = oneshot::channel();
        let (event_tx, mut event_rx) = mpsc::channel::<PathBuf>(64);

        let mut watchers = Vec::new();
        let mut watch_dirs = Vec::new();

        // Global memory dir
        if let Ok(mem_dir) = crate::memory::memory_dir() {
            if mem_dir.exists() {
                watch_dirs.push(mem_dir);
            }
        }

        // Local project memory
        if let Ok(cwd) = std::env::current_dir() {
            let local = cwd.join(".borg").join("memory");
            if local.exists() {
                watch_dirs.push(local);
            }
        }

        // Extra paths
        for raw in &config.memory.extra_paths {
            let expanded = shellexpand::tilde(raw).to_string();
            let p = PathBuf::from(expanded);
            if p.is_dir() {
                watch_dirs.push(p);
            }
        }

        for dir in &watch_dirs {
            let tx = event_tx.clone();
            match notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) => {
                            for path in event.paths {
                                if path.extension().is_some_and(|e| e == "md") {
                                    let _ = tx.try_send(path);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }) {
                Ok(mut w) => {
                    if let Err(e) = w.watch(dir, RecursiveMode::Recursive) {
                        warn!("Failed to watch {}: {e}", dir.display());
                    } else {
                        debug!("Memory watcher watching {}", dir.display());
                        watchers.push(w);
                    }
                }
                Err(e) => {
                    warn!("Failed to create watcher for {}: {e}", dir.display());
                }
            }
        }

        if watchers.is_empty() {
            debug!("No memory directories to watch");
        }

        let config_clone = config;
        tokio::spawn(async move {
            reindex_loop(config_clone, &mut event_rx, stop_rx).await;
        });

        Ok(Self {
            _watchers: watchers,
            stop_tx: Some(stop_tx),
        })
    }

    /// Stop watching.
    pub fn stop(mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Determine scope and relative filename from an absolute path.
pub fn resolve_scope_and_filename(path: &Path, config: &Config) -> (String, String) {
    // Check global memory dir
    if let Ok(global_dir) = crate::memory::memory_dir() {
        if let Ok(rel) = path.strip_prefix(&global_dir) {
            return ("global".to_string(), rel.to_string_lossy().to_string());
        }
    }

    // Check local memory dir
    if let Ok(cwd) = std::env::current_dir() {
        let local_dir = cwd.join(".borg").join("memory");
        if let Ok(rel) = path.strip_prefix(&local_dir) {
            return ("local".to_string(), rel.to_string_lossy().to_string());
        }
    }

    // Check extra paths
    for raw in &config.memory.extra_paths {
        let expanded = shellexpand::tilde(raw).to_string();
        let extra_dir = PathBuf::from(&expanded);
        if let Ok(rel) = path.strip_prefix(&extra_dir) {
            let dir_name = extra_dir.file_name().unwrap_or_default().to_string_lossy();
            return (
                "extra".to_string(),
                format!("extra/{dir_name}/{}", rel.to_string_lossy()),
            );
        }
    }

    // Fallback: use filename only under global scope
    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    tracing::debug!(
        "Could not determine scope for path {}, falling back to global",
        path.display()
    );
    ("global".to_string(), filename)
}

/// Background loop: debounce file change events, then re-embed changed files.
async fn reindex_loop(
    config: Config,
    event_rx: &mut mpsc::Receiver<PathBuf>,
    mut stop_rx: oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                info!("Memory watcher stopped");
                return;
            }
            Some(changed_path) = event_rx.recv() => {
                // Debounce: wait, then drain additional events
                tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
                let mut paths = vec![changed_path];
                while let Ok(p) = event_rx.try_recv() {
                    paths.push(p);
                }
                paths.sort();
                paths.dedup();

                for path in paths {
                    if path.extension().is_none_or(|e| e != "md") {
                        continue;
                    }
                    let (scope, filename) = resolve_scope_and_filename(&path, &config);
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let c = config.clone();
                        tokio::spawn(async move {
                            if let Err(e) = crate::embeddings::embed_memory_file_chunked(
                                &c, &filename, &content, &scope,
                            ).await {
                                debug!("Memory watcher re-embed failed for {filename}: {e}");
                            } else {
                                debug!("Memory watcher re-indexed {scope}/{filename}");
                            }
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_scope_global() {
        let config = Config::default();
        if let Ok(global_dir) = crate::memory::memory_dir() {
            let path = global_dir.join("test.md");
            let (scope, filename) = resolve_scope_and_filename(&path, &config);
            assert_eq!(scope, "global");
            assert_eq!(filename, "test.md");
        }
    }

    #[test]
    fn resolve_scope_extra() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Config {
            memory: crate::config::MemoryConfig {
                extra_paths: vec![tmp.path().to_string_lossy().to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        let path = tmp.path().join("notes.md");
        let (scope, filename) = resolve_scope_and_filename(&path, &config);
        assert_eq!(scope, "extra");
        assert!(filename.contains("notes.md"));
    }

    #[test]
    fn resolve_scope_fallback() {
        let config = Config::default();
        let path = PathBuf::from("/tmp/random/unknown.md");
        let (scope, filename) = resolve_scope_and_filename(&path, &config);
        assert_eq!(scope, "global");
        assert_eq!(filename, "unknown.md");
    }

    #[tokio::test]
    async fn watcher_starts_and_stops() {
        let config = Config::default();
        let watcher = MemoryWatcher::start(config);
        assert!(watcher.is_ok());
        if let Ok(w) = watcher {
            w.stop();
        }
    }

    #[test]
    fn resolve_scope_unknown_path_returns_global() {
        let config = Config::default();
        let path = std::path::PathBuf::from("/some/completely/unknown/path/file.md");
        let (scope, filename) = resolve_scope_and_filename(&path, &config);
        assert_eq!(scope, "global");
        assert_eq!(filename, "file.md");
    }

    #[tokio::test]
    async fn watcher_nonexistent_directory_graceful() {
        // MemoryWatcher::start should not fail even if memory dirs don't exist
        let config = Config {
            memory: crate::config::MemoryConfig {
                extra_paths: vec!["/nonexistent/path/xyz123".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        // Should start without error (non-existent paths are skipped)
        let result = MemoryWatcher::start(config);
        assert!(result.is_ok());
        if let Ok(w) = result {
            w.stop();
        }
    }
}
