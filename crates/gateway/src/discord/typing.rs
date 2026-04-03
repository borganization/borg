use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::api::DiscordClient;
use crate::typing_keepalive::{self, TypingKeepaliveConfig};

/// Keepalive interval for re-sending typing indicator (Discord typing lasts ~10s).
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(8);

/// Discord API endpoint path for triggering typing indicator.
#[cfg(test)]
fn typing_url(channel_id: &str) -> String {
    format!("https://discord.com/api/v10/channels/{channel_id}/typing")
}

/// Handle to a running typing indicator. Call `stop()` to clean up.
pub struct TypingIndicator {
    stop_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl TypingIndicator {
    /// Start typing indicator with keepalive loop.
    ///
    /// Spawns a background task that posts to Discord's typing endpoint
    /// every 8 seconds to keep the indicator visible (it expires after ~10s).
    /// Auto-stops after TTL.
    pub fn start(client: Arc<DiscordClient>, channel_id: String) -> Self {
        let (stop_tx, stop_rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            let config = TypingKeepaliveConfig {
                keepalive_interval: KEEPALIVE_INTERVAL,
                label: "discord",
            };
            typing_keepalive::run_keepalive(config, stop_rx, || {
                let client = client.clone();
                let channel_id = channel_id.clone();
                async move { client.trigger_typing_indicator(&channel_id).await }
            })
            .await;
        });

        Self {
            stop_tx: Some(stop_tx),
            handle: Some(handle),
        }
    }

    /// Stop the keepalive loop and wait for the background task to finish.
    pub async fn stop(mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

/// Ensure the background task is killed if the indicator is dropped without `stop()`.
impl Drop for TypingIndicator {
    fn drop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_expected() {
        assert_eq!(KEEPALIVE_INTERVAL, Duration::from_secs(8));
    }

    #[test]
    fn typing_url_construction() {
        assert_eq!(
            typing_url("123456"),
            "https://discord.com/api/v10/channels/123456/typing"
        );
    }
}
