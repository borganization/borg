//! Shared typing indicator keepalive loop used by Telegram, Slack, and Discord.
//!
//! Each platform has different keepalive intervals and send functions, but the
//! core loop structure (initial trigger, periodic keepalive, TTL deadline,
//! consecutive failure tracking) is identical.

use std::future::Future;
use std::time::Duration;

use borg_core::constants::{TYPING_MAX_CONSECUTIVE_FAILURES, TYPING_MAX_TTL_SECS};
use tokio::sync::oneshot;
use tracing::warn;

/// Platform-specific configuration for the keepalive loop.
pub struct TypingKeepaliveConfig {
    /// How often to re-send the typing action.
    pub keepalive_interval: Duration,
    /// Log prefix for this platform (e.g., "telegram", "slack", "discord").
    pub label: &'static str,
}

/// Run the keepalive loop: initial trigger, then periodic re-sends until
/// stopped, TTL exceeded, or too many consecutive failures.
pub async fn run_keepalive<F, Fut>(
    config: TypingKeepaliveConfig,
    mut stop_rx: oneshot::Receiver<()>,
    send_typing: F,
) where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<(), anyhow::Error>>,
{
    let max_ttl = Duration::from_secs(TYPING_MAX_TTL_SECS);

    // Initial typing trigger
    if let Err(e) = send_typing().await {
        warn!("[{} typing] Initial trigger failed: {e}", config.label);
    }

    let mut keepalive_interval = tokio::time::interval(config.keepalive_interval);
    keepalive_interval.tick().await; // consume first immediate tick
    let ttl_deadline = tokio::time::sleep(max_ttl);
    tokio::pin!(ttl_deadline);

    let mut consecutive_failures: u32 = 0;

    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                break;
            }
            _ = keepalive_interval.tick() => {
                let result = send_typing().await;
                if result.is_err() {
                    consecutive_failures += 1;
                    if consecutive_failures >= TYPING_MAX_CONSECUTIVE_FAILURES {
                        warn!(
                            "[{} typing] {} consecutive failures, stopping keepalive",
                            config.label, TYPING_MAX_CONSECUTIVE_FAILURES
                        );
                        break;
                    }
                } else {
                    consecutive_failures = 0;
                }
            }
            _ = &mut ttl_deadline => {
                warn!(
                    "[{} typing] TTL exceeded ({}s), auto-stopping",
                    config.label, TYPING_MAX_TTL_SECS
                );
                break;
            }
        }
    }
}
