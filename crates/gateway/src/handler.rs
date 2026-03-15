use anyhow::{bail, Context, Result};
use tokio::sync::mpsc;
use tracing::{info, warn};

use tamagotchi_core::agent::{Agent, AgentEvent};
use tamagotchi_core::config::Config;
use tamagotchi_core::db::Database;

use crate::executor::ChannelExecutor;
use crate::registry::RegisteredChannel;

/// Normalized inbound message parsed from the channel's inbound script.
#[derive(Debug, serde::Deserialize)]
pub struct InboundMessage {
    pub sender_id: String,
    pub text: String,
    #[serde(default)]
    pub channel_id: Option<String>,
}

/// Process a webhook request for a channel end-to-end.
pub async fn handle_webhook(
    channel: &RegisteredChannel,
    headers_json: serde_json::Value,
    body: String,
    config: &Config,
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

        let secret = std::env::var(secret_env).with_context(|| {
            format!(
                "Verification env var '{secret_env}' not set for channel '{}'",
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

    let inbound: InboundMessage =
        serde_json::from_str(&inbound_output).context("Invalid inbound script output JSON")?;

    info!(
        "Channel '{}' received message from '{}'",
        channel.manifest.name, inbound.sender_id
    );

    // Step 3: Resolve session
    let db = Database::open().context("Failed to open database")?;
    let session_id = db
        .resolve_channel_session(&channel.manifest.name, &inbound.sender_id)
        .context("Failed to resolve channel session")?;

    // Log inbound message
    let _ = db.log_channel_message(
        &channel.manifest.name,
        &inbound.sender_id,
        "inbound",
        Some(&inbound.text),
        None,
        Some(&session_id),
    );

    // Step 4: Create Agent, load session, send message
    let mut agent = Agent::new(config.clone()).context("Failed to create agent")?;

    if let Err(e) = agent.load_session(&session_id) {
        warn!(
            "Could not load session '{session_id}' for channel '{}': {e}",
            channel.manifest.name
        );
    }

    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(256);
    let message_text = inbound.text.clone();

    let agent_handle = tokio::spawn(async move {
        agent
            .send_message_with_cancel(
                &message_text,
                event_tx,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
    });

    // Collect the full response text
    let mut response_text = String::new();
    while let Some(event) = event_rx.recv().await {
        match event {
            AgentEvent::TextDelta(delta) => response_text.push_str(&delta),
            AgentEvent::Error(e) => {
                warn!("Agent error on channel '{}': {e}", channel.manifest.name)
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
    let _ = db.log_channel_message(
        &channel.manifest.name,
        &inbound.sender_id,
        "outbound",
        Some(&response_text),
        None,
        Some(&session_id),
    );

    // Step 5: Send outbound response via channel script
    let token = channel
        .manifest
        .auth
        .token_env
        .as_deref()
        .and_then(|env| std::env::var(env).ok())
        .unwrap_or_default();

    let outbound_input = serde_json::json!({
        "text": response_text,
        "sender_id": inbound.sender_id,
        "channel_id": inbound.channel_id,
        "token": token,
    });

    match executor
        .send_outbound(&outbound_input.to_string(), blocked_paths)
        .await
    {
        Ok(_) => info!("Outbound sent for channel '{}'", channel.manifest.name),
        Err(e) => warn!(
            "Failed to send outbound for channel '{}': {e}",
            channel.manifest.name
        ),
    }

    Ok(response_text)
}
