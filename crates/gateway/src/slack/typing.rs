use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::warn;

use super::api::SlackClient;

/// Keepalive interval for re-sending typing status (matches OpenClaw).
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(3);

/// Maximum duration before auto-stopping typing indicator (matches OpenClaw).
const MAX_TTL: Duration = Duration::from_secs(60);

/// Reaction emoji added to the user's message while typing.
const TYPING_REACTION: &str = "thinking_face";

/// Handle to a running typing indicator. Call `stop()` to clean up.
pub struct TypingIndicator {
    stop_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
    channel: String,
    thread_ts: Option<String>,
    message_ts: Option<String>,
    client: Arc<SlackClient>,
}

impl TypingIndicator {
    /// Start typing indicator with keepalive loop.
    ///
    /// Spawns a background task that:
    /// 1. Sets thread status to "is typing..." via `assistant.threads.setStatus`
    /// 2. Adds `:thinking_face:` reaction to the user's message
    /// 3. Every 3s, re-sends setStatus to keep it visible (keepalive)
    /// 4. After 60s TTL, auto-stops with warning log
    pub fn start(
        client: Arc<SlackClient>,
        channel: String,
        thread_ts: Option<String>,
        message_ts: Option<String>,
    ) -> Self {
        let (stop_tx, stop_rx) = oneshot::channel();

        let bg_client = client.clone();
        let bg_channel = channel.clone();
        let bg_thread_ts = thread_ts.clone();
        let bg_message_ts = message_ts.clone();

        let handle = tokio::spawn(async move {
            typing_keepalive(bg_client, bg_channel, bg_thread_ts, bg_message_ts, stop_rx).await;
        });

        Self {
            stop_tx: Some(stop_tx),
            handle: Some(handle),
            channel,
            thread_ts,
            message_ts,
            client,
        }
    }

    /// Stop the keepalive loop, clear status, remove reaction.
    pub async fn stop(mut self) {
        // Signal the background task to stop
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }

        // Wait for background task to finish
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }

        // Clear thread status (empty string = clear, matching OpenClaw)
        if let Err(e) = self
            .client
            .set_thread_status(&self.channel, self.thread_ts.as_deref(), "")
            .await
        {
            warn!("[typing] Failed to clear thread status: {e}");
        }

        // Remove reaction
        if let Some(ref ts) = self.message_ts {
            self.client
                .remove_reaction(&self.channel, ts, TYPING_REACTION)
                .await;
        }
    }
}

async fn typing_keepalive(
    client: Arc<SlackClient>,
    channel: String,
    thread_ts: Option<String>,
    message_ts: Option<String>,
    mut stop_rx: oneshot::Receiver<()>,
) {
    // Initial status set
    let _ = client
        .set_thread_status(&channel, thread_ts.as_deref(), "is typing...")
        .await;

    // Add reaction to user's message
    if let Some(ref ts) = message_ts {
        client.add_reaction(&channel, ts, TYPING_REACTION).await;
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
                let result = client
                    .set_thread_status(&channel, thread_ts.as_deref(), "is typing...")
                    .await;
                if result.is_err() {
                    consecutive_failures += 1;
                    if consecutive_failures >= 2 {
                        warn!("[typing] 2 consecutive failures, stopping keepalive");
                        break;
                    }
                } else {
                    consecutive_failures = 0;
                }
            }
            _ = &mut ttl_deadline => {
                warn!("[typing] TTL exceeded (60s), auto-stopping");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_openclaw() {
        assert_eq!(KEEPALIVE_INTERVAL, Duration::from_secs(3));
        assert_eq!(MAX_TTL, Duration::from_secs(60));
        assert_eq!(TYPING_REACTION, "thinking_face");
    }
}
