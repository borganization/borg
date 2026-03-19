use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{oneshot, watch};
use tracing::{info, warn};

use crate::config::Config;

/// Debounce interval before applying config changes (matches OpenClaw's 300ms).
const DEBOUNCE_MS: u64 = 300;

/// Watches `config.toml` for changes and broadcasts validated updates.
/// Modeled after OpenClaw's config-reload.ts state machine.
pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    rx: watch::Receiver<Config>,
    stop_tx: Option<oneshot::Sender<()>>,
}

impl ConfigWatcher {
    /// Start watching a config file. Spawns a background tokio task.
    pub fn start(config_path: PathBuf, initial_config: Config) -> Result<Self> {
        let (config_tx, config_rx) = watch::channel(initial_config);
        let (stop_tx, stop_rx) = oneshot::channel();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<()>(16);

        let target_filename = config_path.file_name().unwrap_or_default().to_os_string();

        // Watch parent directory (more reliable than single-file watch)
        let watch_dir = config_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("config path has no parent directory"))?
            .to_path_buf();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                match event.kind {
                    EventKind::Modify(_) | EventKind::Create(_) => {
                        // Check if event is for our config file
                        let is_config = event
                            .paths
                            .iter()
                            .any(|p| p.file_name().map(|f| f == target_filename).unwrap_or(false));
                        if is_config {
                            let _ = event_tx.try_send(());
                        }
                    }
                    _ => {}
                }
            }
        })?;

        watcher.watch(&watch_dir, RecursiveMode::NonRecursive)?;

        let reload_path = config_path;
        tokio::spawn(async move {
            reload_loop(reload_path, config_tx, &mut event_rx, stop_rx).await;
        });

        Ok(Self {
            _watcher: watcher,
            rx: config_rx,
            stop_tx: Some(stop_tx),
        })
    }

    /// Get a clone of the watch receiver for sharing with agents/gateway.
    pub fn subscribe(&self) -> watch::Receiver<Config> {
        self.rx.clone()
    }

    /// Stop watching (called on shutdown).
    pub fn stop(mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Background reload loop with debounce and validate-before-apply.
async fn reload_loop(
    config_path: PathBuf,
    config_tx: watch::Sender<Config>,
    event_rx: &mut tokio::sync::mpsc::Receiver<()>,
    mut stop_rx: oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                info!("Config watcher stopped");
                return;
            }
            event = event_rx.recv() => {
                if event.is_none() {
                    return;
                }

                // Debounce: wait 300ms, consuming any additional events during that window
                tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
                // Drain any events that arrived during debounce
                while event_rx.try_recv().is_ok() {}

                // Validate and apply
                match Config::load_from(&config_path) {
                    Ok(new_config) => {
                        info!("Config reloaded from disk");
                        let _ = config_tx.send(new_config);
                    }
                    Err(e) => {
                        warn!("Config reload skipped (invalid config): {e}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn reload_on_file_change() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let config_path = dir.path().join("config.toml");

        // Write initial config
        let mut f = std::fs::File::create(&config_path).unwrap_or_else(|e| panic!("create: {e}"));
        writeln!(f, "[llm]\ntemperature = 0.7").unwrap_or_else(|e| panic!("write: {e}"));
        drop(f);

        let initial = Config::load_from(&config_path).unwrap_or_else(|e| panic!("load: {e}"));
        let watcher = ConfigWatcher::start(config_path.clone(), initial)
            .unwrap_or_else(|e| panic!("start: {e}"));
        let mut rx = watcher.subscribe();

        // Modify config
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut f = std::fs::File::create(&config_path).unwrap_or_else(|e| panic!("create2: {e}"));
        writeln!(f, "[llm]\ntemperature = 0.5").unwrap_or_else(|e| panic!("write2: {e}"));
        drop(f);

        // Wait for reload (debounce 300ms + some margin)
        let result = tokio::time::timeout(Duration::from_secs(2), rx.changed()).await;
        assert!(result.is_ok(), "should receive config update");

        let new_config = rx.borrow();
        assert!((new_config.llm.temperature - 0.5).abs() < f32::EPSILON);

        watcher.stop();
    }

    #[tokio::test]
    async fn invalid_config_preserves_old() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let config_path = dir.path().join("config.toml");

        // Write valid config
        let mut f = std::fs::File::create(&config_path).unwrap_or_else(|e| panic!("create: {e}"));
        writeln!(f, "[llm]\ntemperature = 0.7").unwrap_or_else(|e| panic!("write: {e}"));
        drop(f);

        let initial = Config::load_from(&config_path).unwrap_or_else(|e| panic!("load: {e}"));
        let watcher = ConfigWatcher::start(config_path.clone(), initial)
            .unwrap_or_else(|e| panic!("start: {e}"));
        let rx = watcher.subscribe();

        // Write invalid TOML
        tokio::time::sleep(Duration::from_millis(100)).await;
        std::fs::write(&config_path, "invalid [[ toml !!!")
            .unwrap_or_else(|e| panic!("write2: {e}"));

        // Wait for debounce + processing
        tokio::time::sleep(Duration::from_millis(600)).await;

        // Should still have original config
        let config = rx.borrow();
        assert!((config.llm.temperature - 0.7).abs() < f32::EPSILON);

        watcher.stop();
    }
}
