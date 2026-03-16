pub mod echo_cache;
pub mod monitor;
pub mod probe;
pub mod reflection_guard;
pub mod sanitize;
pub mod self_chat_cache;
pub mod send;
pub mod types;

use anyhow::Result;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use tamagotchi_core::config::Config;

/// Start the native iMessage monitor. Reads `~/Library/Messages/chat.db`
/// directly via rusqlite, processes inbound messages through echo detection
/// and reflection guards, dispatches to the agent, and sends replies via
/// osascript.
///
/// Returns a `JoinHandle` for the monitor task.
pub async fn start_imessage_monitor(
    config: Config,
    shutdown: CancellationToken,
) -> Result<JoinHandle<()>> {
    monitor::spawn_monitor(config, shutdown)
}
