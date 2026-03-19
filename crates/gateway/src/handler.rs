use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

use borg_core::agent::{Agent, AgentEvent};
use borg_core::config::Config;
use borg_core::db::{Database, NewDelivery};
use borg_core::sanitize::{
    scan_for_injection, wrap_untrusted, wrap_with_injection_warning, ThreatLevel,
};

use crate::chunker;
use crate::executor::ChannelExecutor;
use crate::health::ChannelHealthRegistry;
use crate::registry::RegisteredChannel;
use crate::retry::{self, RetryOutcome, RetryPolicy};

/// A media attachment on an inbound message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InboundAttachment {
    pub mime_type: String,
    /// Base64-encoded binary content.
    pub data: String,
    #[serde(default)]
    pub filename: Option<String>,
}

/// Normalized inbound message parsed from the channel's inbound script.
#[derive(Debug, serde::Deserialize)]
pub struct InboundMessage {
    pub sender_id: String,
    pub text: String,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub thread_ts: Option<String>,
    #[serde(default)]
    pub attachments: Vec<InboundAttachment>,
}

/// Process a webhook request for a channel end-to-end.
pub async fn handle_webhook(
    channel: &RegisteredChannel,
    headers_json: serde_json::Value,
    body: String,
    config: &Config,
    health: Option<&Arc<RwLock<ChannelHealthRegistry>>>,
) -> Result<String> {
    let executor = ChannelExecutor::new(&channel.manifest, &channel.dir);
    let blocked_paths = &config.security.blocked_paths;

    // Step 1: Verify webhook signature (if secret_env configured)
    if let Some(secret_env) = &channel.manifest.auth.secret_env {
        if channel.manifest.scripts.verify.is_none() {
            bail!(
                "Channel '{}' has secret_env configured but no verify script",
                channel.manifest.name
            );
        }

        let secret = config
            .resolve_credential_or_env(secret_env)
            .with_context(|| {
                format!(
                    "Verification credential '{secret_env}' not found for channel '{}'",
                    channel.manifest.name
                )
            })?;

        let verify_input = serde_json::json!({
            "headers": headers_json,
            "body": body,
            "secret": secret,
        });

        let verified = executor
            .verify(&verify_input.to_string(), blocked_paths)
            .await
            .context("Verification script failed")?;

        if !verified {
            bail!("Webhook verification failed");
        }
    }

    // Step 2: Parse inbound message
    let inbound_input = serde_json::json!({
        "headers": headers_json,
        "body": body,
    });

    let inbound_output = executor
        .parse_inbound(&inbound_input.to_string(), blocked_paths)
        .await
        .context("Inbound parsing failed")?;

    let parsed: serde_json::Value =
        serde_json::from_str(&inbound_output).context("Invalid inbound script output JSON")?;
    if parsed
        .get("skip")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return Ok("(skipped)".to_string());
    }
    let inbound: InboundMessage =
        serde_json::from_value(parsed).context("Invalid inbound message structure")?;

    // Steps 3-5: shared processing
    process_message(channel, inbound, config, health).await
}

/// Process a polled message that is already normalized (no verify/parse needed).
pub async fn handle_polled_message(
    channel: &RegisteredChannel,
    message: InboundMessage,
    config: &Config,
    health: Option<&Arc<RwLock<ChannelHealthRegistry>>>,
) -> Result<String> {
    process_message(channel, message, config, health).await
}

/// Build a RetryPolicy from channel settings, falling back to defaults.
fn build_retry_policy(channel: &RegisteredChannel) -> RetryPolicy {
    let mut policy = RetryPolicy::default();
    if let Some(max) = channel.manifest.settings.retry_max_attempts {
        policy.max_retries = max;
    }
    if let Some(delay) = channel.manifest.settings.retry_initial_delay_ms {
        policy.initial_delay_ms = delay;
    }
    policy
}

/// Invoke the agent with an inbound message and return the response text and session ID.
///
/// This is the shared core: session resolution, agent creation, message dispatch, response collection.
/// Used by both script-based channels and native integrations (e.g. Telegram).
///
/// Applies gateway routing to select per-channel/sender agent configuration overrides.
pub async fn invoke_agent(
    channel_name: &str,
    inbound: &InboundMessage,
    config: &Config,
    health: Option<&Arc<RwLock<ChannelHealthRegistry>>>,
) -> Result<(String, String)> {
    // Resolve gateway routing (per-channel/sender config overrides)
    let route = crate::routing::resolve_route(config, channel_name, &inbound.sender_id, None);
    let config = &route.config;

    info!(
        "Channel '{}' received message from '{}' (route: {})",
        channel_name, inbound.sender_id, route.matched_by
    );

    if let Some(h) = health {
        h.write().await.record_inbound(channel_name);
    }

    // Resolve session — include thread_id and binding_id in the key for isolation
    let db = Database::open().context("Failed to open database")?;
    let base_key = match &inbound.thread_id {
        Some(tid) => format!("{}:{}", inbound.sender_id, tid),
        None => inbound.sender_id.clone(),
    };
    let session_key = if route.binding_id != "default" {
        format!("{}:{}", route.binding_id, base_key)
    } else {
        base_key
    };
    let session_id = db
        .resolve_channel_session(channel_name, &session_key)
        .context("Failed to resolve channel session")?;

    // Log inbound message
    if let Err(e) = db.log_channel_message(
        channel_name,
        &inbound.sender_id,
        "inbound",
        Some(&inbound.text),
        None,
        Some(&session_id),
    ) {
        warn!("Failed to log inbound message for channel '{channel_name}': {e}");
    }

    // Check for slash commands before creating agent
    if let Some(response) = crate::commands::try_handle_command(
        &inbound.text,
        &db,
        config,
        channel_name,
        &session_key,
        &session_id,
    ) {
        if let Err(e) = db.log_channel_message(
            channel_name,
            &inbound.sender_id,
            "outbound",
            Some(&response),
            None,
            Some(&session_id),
        ) {
            warn!("Failed to log command response: {e}");
        }
        return Ok((response, session_id));
    }

    // Create Agent, load session, send message
    let mut agent = Agent::new(config.clone(), borg_core::telemetry::BorgMetrics::noop())
        .context("Failed to create agent")?;

    if let Err(e) = agent.load_session(&session_id) {
        warn!(
            "Could not load session '{session_id}' for channel '{}': {e}",
            channel_name
        );
    }

    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(256);

    // Apply injection scanning and source attribution
    let message_text = {
        let base = format!(
            "[Channel: {}, Sender: {}]\n{}",
            channel_name, inbound.sender_id, inbound.text
        );
        match scan_for_injection(&inbound.text) {
            ThreatLevel::HighRisk { .. } => wrap_with_injection_warning(channel_name, &base),
            ThreatLevel::Flagged { .. } => wrap_untrusted(channel_name, &base),
            ThreatLevel::Clean => base,
        }
    };

    // Check for image attachments and build multimodal message if present
    let has_image_attachments = inbound
        .attachments
        .iter()
        .any(|a| a.mime_type.starts_with("image/"));

    let agent_handle = if has_image_attachments {
        let mut parts = vec![borg_core::types::ContentPart::Text(message_text)];
        for att in &inbound.attachments {
            if att.mime_type.starts_with("image/") {
                parts.push(borg_core::types::ContentPart::ImageBase64 {
                    media: borg_core::types::MediaData {
                        mime_type: att.mime_type.clone(),
                        data: att.data.clone(),
                        filename: att.filename.clone(),
                    },
                });
            }
        }
        // Compress images before sending to agent
        if config.media.compression_enabled {
            borg_core::media::compress_content_parts(&mut parts, config.media.max_image_bytes);
        }
        let msg = borg_core::types::Message::user_multimodal(parts);
        tokio::spawn(async move {
            agent
                .send_message_raw(msg, event_tx, tokio_util::sync::CancellationToken::new())
                .await
        })
    } else {
        tokio::spawn(async move {
            agent
                .send_message_with_cancel(
                    &message_text,
                    event_tx,
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
        })
    };

    // Collect the full response text
    let mut response_text = String::new();
    while let Some(event) = event_rx.recv().await {
        match event {
            AgentEvent::TextDelta(delta) => response_text.push_str(&delta),
            AgentEvent::Error(e) => {
                warn!("Agent error on channel '{}': {e}", channel_name)
            }
            AgentEvent::ToolConfirmation {
                respond,
                tool_name,
                reason,
            } => {
                warn!("Auto-denying tool confirmation in gateway mode: {tool_name} ({reason})");
                response_text.push_str(&format!(
                    "\n[Operation denied: {tool_name} requires confirmation — {reason}]"
                ));
                let _ = respond.send(false);
            }
            AgentEvent::ShellConfirmation { respond, command } => {
                warn!("Auto-denying shell confirmation in gateway mode: {command}");
                response_text.push_str("\n[Operation denied: shell command requires confirmation]");
                let _ = respond.send(false);
            }
            _ => {}
        }
    }

    // Wait for agent to finish
    match agent_handle.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!("Agent error: {e}"),
        Err(e) => warn!("Agent task panicked: {e}"),
    }

    if response_text.is_empty() {
        response_text = "(no response)".to_string();
    }

    // Log outbound message
    if let Err(e) = db.log_channel_message(
        channel_name,
        &inbound.sender_id,
        "outbound",
        Some(&response_text),
        None,
        Some(&session_id),
    ) {
        warn!("Failed to log outbound message for channel '{channel_name}': {e}");
    }

    Ok((response_text, session_id))
}

/// Shared message processing: session resolution, agent invocation, outbound with retry + chunking.
async fn process_message(
    channel: &RegisteredChannel,
    inbound: InboundMessage,
    config: &Config,
    health: Option<&Arc<RwLock<ChannelHealthRegistry>>>,
) -> Result<String> {
    let channel_name = &channel.manifest.name;

    let (response_text, _session_id) = invoke_agent(channel_name, &inbound, config, health).await?;

    // Prepare auth tokens (resolve from credential store, falling back to env vars)
    let token = channel
        .manifest
        .auth
        .token_env
        .as_deref()
        .and_then(|env| config.resolve_credential_or_env(env))
        .unwrap_or_default();

    let secret = channel
        .manifest
        .auth
        .secret_env
        .as_deref()
        .and_then(|env| config.resolve_credential_or_env(env))
        .unwrap_or_default();

    let executor = ChannelExecutor::new(&channel.manifest, &channel.dir);
    let blocked_paths = &config.security.blocked_paths;
    let retry_policy = build_retry_policy(channel);

    // Chunk text if max_message_chars is configured
    let chunks = match channel.manifest.settings.max_message_chars {
        Some(max) if max > 0 => chunker::chunk_text(&response_text, max),
        _ => vec![response_text.clone()],
    };
    let total_chunks = chunks.len();

    let db = Database::open().context("Failed to open database")?;

    for (i, chunk) in chunks.iter().enumerate() {
        // Build payload without secrets for persistence
        let mut outbound = serde_json::json!({
            "text": chunk,
            "sender_id": inbound.sender_id,
            "channel_id": inbound.channel_id,
        });

        if total_chunks > 1 {
            outbound["chunk_index"] = serde_json::json!(i);
            outbound["total_chunks"] = serde_json::json!(total_chunks);
        }

        let persist_str = outbound.to_string();

        // Add secrets for runtime sending only
        outbound["token"] = serde_json::json!(token);
        outbound["secret"] = serde_json::json!(secret);
        let outbound_str = outbound.to_string();

        // Enqueue to delivery queue for persistence (without secrets)
        let delivery_id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = db.enqueue_delivery(&NewDelivery {
            id: &delivery_id,
            channel_name,
            sender_id: &inbound.sender_id,
            channel_id: inbound.channel_id.as_deref(),
            session_id: Some(&_session_id),
            payload_json: &persist_str,
            max_retries: retry_policy.max_retries as i32,
        }) {
            warn!("Failed to enqueue delivery for channel '{channel_name}': {e}");
        }

        // Send with retry
        match retry::send_with_retry(&executor, &outbound_str, blocked_paths, &retry_policy).await {
            RetryOutcome::Success(_) => {
                info!(
                    "Outbound sent for channel '{}' (chunk {}/{})",
                    channel_name,
                    i + 1,
                    total_chunks
                );
                if let Err(e) = db.mark_delivered(&delivery_id) {
                    warn!("Failed to mark delivery '{delivery_id}' as delivered: {e}");
                }
                if let Some(h) = health {
                    h.write().await.record_outbound(channel_name);
                }
            }
            RetryOutcome::PermanentFailure(e) | RetryOutcome::Exhausted(e) => {
                warn!("Outbound failure for channel '{}': {e}", channel_name);
                record_delivery_failure_sync(&db, &delivery_id, &e);
                if let Some(h) = health {
                    h.write().await.record_error(channel_name, &e);
                }
            }
        }
    }

    Ok(response_text)
}

fn record_delivery_failure_sync(db: &Database, delivery_id: &str, error: &str) {
    if let Err(db_err) = db.mark_failed(delivery_id, error, None) {
        warn!("Failed to mark delivery '{delivery_id}' as failed: {db_err}");
    }
}
