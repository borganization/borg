use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use tamagotchi_core::config::Config;

use super::echo_cache::EchoCache;
use super::reflection_guard;
use super::sanitize::sanitize_outbound;
use super::self_chat_cache::SelfChatCache;
use super::send::send_imessage;
use super::types::{IMessagePayload, InboundDecision};
use crate::handler::{self, InboundMessage};
use crate::registry::RegisteredChannel;

const DEFAULT_POLL_INTERVAL_MS: u64 = 2000;
const STATE_FILE: &str = "state.json";

/// Start the iMessage monitor loop in a background task.
pub fn spawn_monitor(config: Config, shutdown: CancellationToken) -> Result<JoinHandle<()>> {
    let data_dir = Config::data_dir()?;
    let channel_dir = data_dir.join("channels/imessage");

    let handle = tokio::spawn(async move {
        if let Err(e) = monitor_loop(config, channel_dir, shutdown).await {
            tracing::error!("iMessage monitor exited with error: {e}");
        }
    });

    Ok(handle)
}

async fn monitor_loop(
    config: Config,
    channel_dir: PathBuf,
    shutdown: CancellationToken,
) -> Result<()> {
    let db_path = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("no home directory"))?
        .join("Library/Messages/chat.db");

    if !db_path.exists() {
        anyhow::bail!("chat.db not found at {}", db_path.display());
    }

    let db_uri = format!("file:{}?mode=ro", db_path.display());

    // Load persisted state (last_rowid)
    let state_path = channel_dir.join(STATE_FILE);
    let mut last_rowid = load_last_rowid(&state_path);

    // If no state, initialize to current max ROWID (only process future messages)
    if last_rowid == 0 {
        let conn = open_chatdb(&db_uri)?;
        last_rowid = conn
            .query_row("SELECT MAX(ROWID) FROM message", [], |row| {
                row.get::<_, Option<i64>>(0)
            })?
            .unwrap_or(0);
        save_last_rowid(&state_path, last_rowid);
    }

    let poll_interval = Duration::from_millis(DEFAULT_POLL_INTERVAL_MS);
    let mut echo_cache = EchoCache::new();
    let mut self_chat_cache = SelfChatCache::new();

    // Load channel manifest for handler reuse
    let manifest_path = channel_dir.join("channel.toml");
    let manifest = crate::manifest::ChannelManifest::load(&manifest_path)
        .context("Failed to load imessage channel.toml")?;
    let registered_channel = RegisteredChannel {
        manifest,
        dir: channel_dir.clone(),
    };

    info!(
        "iMessage monitor started (last_rowid={last_rowid}, poll={}ms)",
        poll_interval.as_millis()
    );

    let mut interval = tokio::time::interval(poll_interval);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                info!("iMessage monitor shutting down");
                break;
            }
            _ = interval.tick() => {
                match poll_new_messages(&db_uri, last_rowid) {
                    Ok(messages) => {
                        for payload in messages {
                            let rowid = payload.rowid;
                            let decision = evaluate_message(
                                &payload,
                                &mut echo_cache,
                                &mut self_chat_cache,
                            );

                            match decision {
                                InboundDecision::Dispatch { sender_id, text, channel_id } => {
                                    info!("iMessage from {sender_id}: {}", &text[..text.len().min(50)]);

                                    let inbound = InboundMessage {
                                        sender_id: sender_id.clone(),
                                        text,
                                        channel_id: Some(channel_id),
                                        thread_id: None,
                                        message_id: None,
                                        thread_ts: None,
                                    };

                                    match handler::handle_polled_message(
                                        &registered_channel,
                                        inbound,
                                        &config,
                                        None,
                                    ).await {
                                        Ok(response) => {
                                            let sanitized = sanitize_outbound(&response);
                                            if !sanitized.is_empty() && sanitized != "(no response)" {
                                                // Detect service from chat identifier
                                                let service = payload.chat_identifier
                                                    .as_deref()
                                                    .and_then(|ci| {
                                                        if ci.starts_with("SMS") { Some("SMS") } else { None }
                                                    })
                                                    .unwrap_or("iMessage");

                                                if let Err(e) = send_imessage(&sender_id, &sanitized, service).await {
                                                    warn!("Failed to send iMessage reply: {e}");
                                                } else {
                                                    echo_cache.remember(&sanitized, None);
                                                }
                                            }
                                        }
                                        Err(e) => warn!("Failed to process iMessage: {e}"),
                                    }
                                }
                                InboundDecision::Drop { reason } => {
                                    debug!("Dropped iMessage rowid={rowid}: {reason}");
                                }
                            }

                            if rowid > last_rowid {
                                last_rowid = rowid;
                                save_last_rowid(&state_path, last_rowid);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to poll chat.db: {e}");
                    }
                }
            }
        }
    }

    Ok(())
}

/// Evaluate a message through all decision gates.
fn evaluate_message(
    payload: &IMessagePayload,
    echo_cache: &mut EchoCache,
    self_chat_cache: &mut SelfChatCache,
) -> InboundDecision {
    let text = match &payload.text {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => {
            return InboundDecision::Drop {
                reason: "empty text".to_string(),
            }
        }
    };

    let chat_id = payload
        .chat_identifier
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    // Gate 1: is_from_me — record in self-chat cache, then drop
    if payload.is_from_me {
        self_chat_cache.remember(&text, &chat_id);
        return InboundDecision::Drop {
            reason: "is_from_me".to_string(),
        };
    }

    // Gate 2: echo detection — did we recently send this exact text?
    if echo_cache.is_echo(&text, None) {
        return InboundDecision::Drop {
            reason: "echo of sent message".to_string(),
        };
    }

    // Gate 3: self-chat reflection — was this recently sent by us in the same chat?
    if self_chat_cache.is_self_echo(&text, &chat_id) {
        return InboundDecision::Drop {
            reason: "self-chat reflection".to_string(),
        };
    }

    // Gate 4: reflection guard — contains internal markers?
    if let Some(marker) = reflection_guard::detect_reflected_content(&text) {
        return InboundDecision::Drop {
            reason: format!("reflected content: {marker}"),
        };
    }

    // Determine sender
    let sender_id = payload
        .sender
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    InboundDecision::Dispatch {
        sender_id,
        text,
        channel_id: chat_id,
    }
}

fn poll_new_messages(db_uri: &str, last_rowid: i64) -> Result<Vec<IMessagePayload>> {
    let conn = open_chatdb(db_uri)?;

    let mut stmt = conn.prepare(
        "SELECT m.ROWID, m.text, m.date, h.id, m.is_from_me, m.cache_roomnames, \
         c.chat_identifier, c.guid \
         FROM message m \
         LEFT JOIN handle h ON m.handle_id = h.ROWID \
         LEFT JOIN chat_message_join cmj ON cmj.message_id = m.ROWID \
         LEFT JOIN chat c ON c.ROWID = cmj.chat_id \
         WHERE m.ROWID > ?1 AND m.text IS NOT NULL \
         ORDER BY m.ROWID ASC \
         LIMIT 50",
    )?;

    let rows = stmt.query_map([last_rowid], |row| {
        let cache_roomnames: Option<String> = row.get(5)?;
        let is_group = cache_roomnames.as_deref().is_some_and(|r| !r.is_empty());

        Ok(IMessagePayload {
            rowid: row.get(0)?,
            text: row.get(1)?,
            sender: row.get(3)?,
            is_from_me: row.get::<_, i32>(4)? != 0,
            chat_identifier: row.get(6)?,
            is_group,
        })
    })?;

    let mut messages = Vec::new();
    for row in rows {
        match row {
            Ok(msg) => messages.push(msg),
            Err(e) => warn!("Failed to parse message row: {e}"),
        }
    }

    Ok(messages)
}

fn open_chatdb(db_uri: &str) -> Result<rusqlite::Connection> {
    rusqlite::Connection::open_with_flags(
        db_uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .context("Failed to open chat.db (check Full Disk Access)")
}

fn load_last_rowid(state_path: &std::path::Path) -> i64 {
    std::fs::read_to_string(state_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("last_rowid")?.as_i64())
        .unwrap_or(0)
}

fn save_last_rowid(state_path: &std::path::Path, rowid: i64) {
    let state = format!("{{\"last_rowid\": {rowid}}}");
    if let Err(e) = std::fs::write(state_path, &state) {
        warn!("Failed to save iMessage state: {e}");
    }
}
