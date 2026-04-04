//! `NativeChannel` implementation for Discord.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::http::{HeaderMap, StatusCode};
use tokio::sync::RwLock;

use crate::channel_trait::{NativeChannel, TypingHandle, WebhookContext, WebhookOutcome};
use crate::health::ChannelHealthRegistry;

use super::api::DiscordClient;
use super::DiscordWebhookResult;

/// Discord-specific response context stored in the delivery queue.
#[derive(serde::Serialize, serde::Deserialize)]
struct DiscordResponseContext {
    channel_id: String,
    interaction_id: String,
    interaction_token: String,
    application_id: Option<String>,
}

/// Discord native channel implementation.
pub struct DiscordChannel {
    pub client: Arc<DiscordClient>,
    pub public_key: Option<String>,
}

#[async_trait]
impl NativeChannel for DiscordChannel {
    fn names(&self) -> Vec<&str> {
        vec!["discord"]
    }

    async fn handle_webhook(
        &self,
        headers: &HeaderMap,
        body: &str,
        _ctx: &WebhookContext<'_>,
    ) -> Result<WebhookOutcome> {
        let webhook_result =
            super::handle_discord_webhook(headers, body, self.public_key.as_deref())?;

        match webhook_result {
            DiscordWebhookResult::Pong(response) => Ok(WebhookOutcome::ProtocolResponse((
                StatusCode::OK,
                axum::Json(serde_json::to_value(response).unwrap_or_default()),
            ))),
            DiscordWebhookResult::Skip => Ok(WebhookOutcome::Skip),
            DiscordWebhookResult::Message(inbound, interaction) => {
                // Send deferred response immediately so Discord doesn't time out
                if let Err(e) = self
                    .client
                    .create_interaction_response(
                        &interaction.id,
                        &interaction.token,
                        &super::types::InteractionResponse::deferred(),
                    )
                    .await
                {
                    tracing::warn!("Failed to send Discord deferred response: {e}");
                }

                let session_key =
                    inbound.session_key("discord", inbound.channel_id.as_deref().unwrap_or(""));

                let response_context = serde_json::to_value(DiscordResponseContext {
                    channel_id: inbound.channel_id.clone().unwrap_or_default(),
                    interaction_id: interaction.id.clone(),
                    interaction_token: interaction.token.clone(),
                    application_id: interaction.application_id.clone(),
                })?;

                Ok(WebhookOutcome::Message {
                    inbound,
                    session_key,
                    response_context,
                })
            }
        }
    }

    async fn send_response(
        &self,
        response_text: &str,
        response_context: &serde_json::Value,
        health: &Arc<RwLock<ChannelHealthRegistry>>,
    ) -> Result<()> {
        let ctx: DiscordResponseContext = serde_json::from_value(response_context.clone())?;

        if let Some(ref app_id) = ctx.application_id {
            if let Err(e) = self
                .client
                .edit_original_response(app_id, &ctx.interaction_token, response_text)
                .await
            {
                tracing::warn!("Failed to edit Discord interaction response: {e}");
                // Fallback: send as channel message
                if let Err(e2) = self
                    .client
                    .send_message(&ctx.channel_id, response_text)
                    .await
                {
                    health
                        .write()
                        .await
                        .record_error("discord", &e2.to_string());
                    return Err(e2);
                }
            }
        } else if let Err(e) = self
            .client
            .send_message(&ctx.channel_id, response_text)
            .await
        {
            health.write().await.record_error("discord", &e.to_string());
            return Err(e);
        }

        health.write().await.record_outbound("discord");
        Ok(())
    }

    fn start_typing(&self, response_context: &serde_json::Value) -> Option<TypingHandle> {
        let ctx: DiscordResponseContext = serde_json::from_value(response_context.clone()).ok()?;
        let client = self.client.clone();

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let join = tokio::spawn(async move {
            let config = crate::typing_keepalive::TypingKeepaliveConfig {
                keepalive_interval: std::time::Duration::from_secs(8),
                label: "discord",
            };
            crate::typing_keepalive::run_keepalive(config, stop_rx, || {
                let client = client.clone();
                let ch = ctx.channel_id.clone();
                async move { client.trigger_typing_indicator(&ch).await }
            })
            .await;
        });

        Some(TypingHandle::new(stop_tx, join))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_context_roundtrip() {
        let ctx = DiscordResponseContext {
            channel_id: "ch1".to_string(),
            interaction_id: "int1".to_string(),
            interaction_token: "tok1".to_string(),
            application_id: Some("app1".to_string()),
        };
        let json = serde_json::to_value(&ctx).unwrap();
        let back: DiscordResponseContext = serde_json::from_value(json).unwrap();
        assert_eq!(back.channel_id, "ch1");
        assert_eq!(back.application_id.as_deref(), Some("app1"));
    }
}
