use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::warn;

use super::api::TelegramClient;

/// Keepalive interval for re-sending typing action (Telegram typing expires ~5s).
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(4);

/// Maximum duration before auto-stopping typing indicator.
const MAX_TTL: Duration = Duration::from_secs(60);

/// Handle to a running typing indicator. Call `stop()` to clean up.
pub struct TypingIndicator {
    stop_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl TypingIndicator {
    /// Start typing indicator with keepalive loop.
    ///
    /// Spawns a background task that calls Telegram's sendChatAction("typing")
    /// every 4 seconds to keep the indicator visible (it expires after ~5s).
    /// Auto-stops after 60 seconds TTL.
    pub fn start(client: Arc<TelegramClient>, chat_id: i64) -> Self {
        let (stop_tx, stop_rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            typing_keepalive(client, chat_id, stop_rx).await;
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

async fn typing_keepalive(
    client: Arc<TelegramClient>,
    chat_id: i64,
    mut stop_rx: oneshot::Receiver<()>,
) {
    // Initial typing trigger
    if let Err(e) = client.send_typing(chat_id).await {
        warn!("[telegram typing] Initial trigger failed: {e}");
    }

    let mut keepalive_interval = tokio::time::interval(KEEPALIVE_INTERVAL);
    keepalive_interval.tick().await; // consume first immediate tick
    let ttl_deadline = tokio::time::sleep(MAX_TTL);
    tokio::pin!(ttl_deadline);

    let mut consecutive_failures: u32 = 0;

    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                break;
            }
            _ = keepalive_interval.tick() => {
                let result = client.send_typing(chat_id).await;
                if result.is_err() {
                    consecutive_failures += 1;
                    if consecutive_failures >= 2 {
                        warn!("[telegram typing] 2 consecutive failures, stopping keepalive");
                        break;
                    }
                } else {
                    consecutive_failures = 0;
                }
            }
            _ = &mut ttl_deadline => {
                warn!("[telegram typing] TTL exceeded (60s), auto-stopping");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_expected() {
        assert_eq!(KEEPALIVE_INTERVAL, Duration::from_secs(4));
        assert_eq!(MAX_TTL, Duration::from_secs(60));
    }
}
