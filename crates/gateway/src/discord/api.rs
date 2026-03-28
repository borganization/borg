use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use serde_json::json;
use tracing::warn;

use super::types::{CreateMessageRequest, CurrentUser, InteractionResponse};
use crate::chunker;
use crate::commands::{CommandDef, NativeCommandRegistration};
use crate::constants::GATEWAY_HTTP_TIMEOUT;
use crate::http_retry::{send_with_rate_limit_retry, RateLimitPolicy};

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const DISCORD_MESSAGE_CHUNK_SIZE: usize = 2000;

/// A client for the Discord REST API.
#[derive(Clone)]
pub struct DiscordClient {
    client: Client,
    token: String,
    application_id: Option<String>,
}

impl DiscordClient {
    pub fn new(token: &str) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(GATEWAY_HTTP_TIMEOUT)
                .build()
                .context("Failed to build Discord HTTP client")?,
            token: token.to_string(),
            application_id: None,
        })
    }

    /// Set the application ID (typically the bot user ID from get_current_user).
    pub fn set_application_id(&mut self, id: String) {
        self.application_id = Some(id);
    }

    /// Get the current bot user via GET /users/@me.
    pub async fn get_current_user(&self) -> Result<CurrentUser> {
        let resp = self
            .client
            .get(format!("{DISCORD_API_BASE}/users/@me"))
            .header("Authorization", format!("Bot {}", self.token))
            .send()
            .await
            .context("Failed to call GET /users/@me")?;

        let status = resp.status();
        if !status.is_success() {
            let body = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to read Discord error response body: {e}");
                    String::new()
                }
            };
            bail!("GET /users/@me failed ({status}): {body}");
        }

        resp.json()
            .await
            .context("Failed to parse /users/@me response")
    }

    /// Create an interaction response (initial reply to an interaction).
    ///
    /// This endpoint does NOT require a bot authorization header — it uses
    /// the interaction token in the URL path.
    pub async fn create_interaction_response(
        &self,
        interaction_id: &str,
        interaction_token: &str,
        response: &InteractionResponse,
    ) -> Result<()> {
        let url = format!(
            "{DISCORD_API_BASE}/interactions/{interaction_id}/{interaction_token}/callback"
        );
        let client = self.client.clone();
        let response = response.clone();

        self.send_with_retry(move || {
            let client = client.clone();
            let url = url.clone();
            let response = response.clone();
            async move {
                client
                    .post(&url)
                    .json(&response)
                    .send()
                    .await
                    .context("Discord API request failed")
            }
        })
        .await?;

        Ok(())
    }

    /// Edit the original interaction response (for deferred replies).
    ///
    /// Automatically chunks text at 2000 characters. For messages exceeding
    /// the limit, the first chunk is sent as an edit and subsequent chunks
    /// are sent as follow-up messages.
    pub async fn edit_original_response(
        &self,
        application_id: &str,
        interaction_token: &str,
        text: &str,
    ) -> Result<()> {
        let chunks = chunker::chunk_text_nonempty(text, DISCORD_MESSAGE_CHUNK_SIZE);

        // Edit the original response with the first chunk
        let url = format!(
            "{DISCORD_API_BASE}/webhooks/{application_id}/{interaction_token}/messages/@original"
        );
        let chunk0 = chunks[0].clone();
        let client = self.client.clone();
        self.send_with_retry(move || {
            let client = client.clone();
            let url = url.clone();
            let chunk0 = chunk0.clone();
            async move {
                client
                    .patch(&url)
                    .json(&json!({ "content": chunk0 }))
                    .send()
                    .await
                    .context("Discord API request failed")
            }
        })
        .await?;

        // Send remaining chunks as follow-up messages
        for chunk in &chunks[1..] {
            let followup_url =
                format!("{DISCORD_API_BASE}/webhooks/{application_id}/{interaction_token}");
            let chunk = chunk.clone();
            let client = self.client.clone();
            self.send_with_retry(move || {
                let client = client.clone();
                let url = followup_url.clone();
                let chunk = chunk.clone();
                async move {
                    client
                        .post(&url)
                        .json(&json!({ "content": chunk }))
                        .send()
                        .await
                        .context("Discord API request failed")
                }
            })
            .await?;
        }

        Ok(())
    }

    /// Send a message to a channel, chunking at 2000 characters.
    pub async fn send_message(&self, channel_id: &str, text: &str) -> Result<()> {
        let chunks = chunker::chunk_text_nonempty(text, DISCORD_MESSAGE_CHUNK_SIZE);

        for chunk in &chunks {
            let url = format!("{DISCORD_API_BASE}/channels/{channel_id}/messages");
            let body = CreateMessageRequest {
                content: chunk.clone(),
            };
            let token = self.token.clone();
            let client = self.client.clone();
            self.send_with_retry(move || {
                let client = client.clone();
                let url = url.clone();
                let body = body.clone();
                let token = token.clone();
                async move {
                    client
                        .post(&url)
                        .header("Authorization", format!("Bot {token}"))
                        .json(&body)
                        .send()
                        .await
                        .context("Discord API request failed")
                }
            })
            .await?;
        }

        Ok(())
    }

    /// Trigger a typing indicator in a channel.
    /// POST /channels/{channel_id}/typing
    pub async fn trigger_typing_indicator(&self, channel_id: &str) -> Result<()> {
        let url = format!("{DISCORD_API_BASE}/channels/{channel_id}/typing");
        let token = self.token.clone();
        let client = self.client.clone();

        self.send_with_retry(move || {
            let client = client.clone();
            let url = url.clone();
            let token = token.clone();
            async move {
                client
                    .post(&url)
                    .header("Authorization", format!("Bot {token}"))
                    .header("Content-Length", "0")
                    .send()
                    .await
                    .context("Discord API request failed")
            }
        })
        .await?;

        Ok(())
    }

    /// Add a reaction to a message.
    pub async fn add_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> Result<()> {
        let url = format!(
            "{DISCORD_API_BASE}/channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me"
        );
        let token = self.token.clone();
        let client = self.client.clone();

        self.send_with_retry(move || {
            let client = client.clone();
            let url = url.clone();
            let token = token.clone();
            async move {
                client
                    .put(&url)
                    .header("Authorization", format!("Bot {token}"))
                    .header("Content-Length", "0")
                    .send()
                    .await
                    .context("Discord API request failed")
            }
        })
        .await?;

        Ok(())
    }

    /// Register global application commands with Discord.
    /// Uses `PUT /applications/{app_id}/commands` to bulk overwrite all commands.
    pub async fn register_global_commands(
        &self,
        application_id: &str,
        commands: &[DiscordCommandPayload],
    ) -> Result<()> {
        let url = format!("{DISCORD_API_BASE}/applications/{application_id}/commands");
        let token = self.token.clone();
        let body = serde_json::to_value(commands).context("Failed to serialize commands")?;
        let client = self.client.clone();

        self.send_with_retry(move || {
            let client = client.clone();
            let url = url.clone();
            let token = token.clone();
            let body = body.clone();
            async move {
                client
                    .put(&url)
                    .header("Authorization", format!("Bot {token}"))
                    .json(&body)
                    .send()
                    .await
                    .context("Discord API request failed")
            }
        })
        .await?;

        Ok(())
    }

    /// Send a request with 429 rate-limit retry logic using the shared retry helper.
    async fn send_with_retry<F, Fut>(&self, make_request: F) -> Result<reqwest::Response>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<reqwest::Response>>,
    {
        let policy = RateLimitPolicy {
            service_name: "Discord",
            ..RateLimitPolicy::default()
        };

        let resp = send_with_rate_limit_retry(&policy, make_request).await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Discord API error ({status}): {body}");
        }

        Ok(resp)
    }
}

/// Payload for registering a Discord global application command.
#[derive(Serialize, Clone)]
pub struct DiscordCommandPayload {
    pub name: String,
    pub description: String,
    /// Command type: 1 = CHAT_INPUT (slash command)
    #[serde(rename = "type")]
    pub command_type: u8,
}

#[async_trait]
impl NativeCommandRegistration for DiscordClient {
    async fn register_commands(&self, commands: &[CommandDef]) -> Result<()> {
        let app_id = self
            .application_id
            .as_deref()
            .context("Discord application_id not set")?;

        let payloads: Vec<DiscordCommandPayload> = commands
            .iter()
            .map(|c| DiscordCommandPayload {
                name: c.name.to_string(),
                description: c.description.to_string(),
                command_type: 1,
            })
            .collect();

        self.register_global_commands(app_id, &payloads).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_url_construction() {
        assert_eq!(
            format!("{DISCORD_API_BASE}/users/@me"),
            "https://discord.com/api/v10/users/@me"
        );
        assert_eq!(
            format!("{DISCORD_API_BASE}/channels/123/messages"),
            "https://discord.com/api/v10/channels/123/messages"
        );
        assert_eq!(
            format!("{DISCORD_API_BASE}/interactions/int1/tok/callback"),
            "https://discord.com/api/v10/interactions/int1/tok/callback"
        );
        assert_eq!(
            format!("{DISCORD_API_BASE}/webhooks/app1/tok/messages/@original"),
            "https://discord.com/api/v10/webhooks/app1/tok/messages/@original"
        );
        assert_eq!(
            format!("{DISCORD_API_BASE}/webhooks/app1/tok"),
            "https://discord.com/api/v10/webhooks/app1/tok"
        );
    }

    #[test]
    fn create_message_request_serialization() {
        let req = CreateMessageRequest {
            content: "hello discord".into(),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["content"], "hello discord");
    }

    #[test]
    fn interaction_response_serialization() {
        let resp = InteractionResponse::deferred();
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["type"], 5);
    }

    #[test]
    fn chunking_at_2000() {
        let long_text: String = "a".repeat(4500);
        let chunks = chunker::chunk_text(&long_text, DISCORD_MESSAGE_CHUNK_SIZE);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 2000);
        assert_eq!(chunks[1].len(), 2000);
        assert_eq!(chunks[2].len(), 500);
    }

    #[test]
    fn short_text_single_chunk() {
        let text = "short message";
        let chunks = chunker::chunk_text(text, DISCORD_MESSAGE_CHUNK_SIZE);
        assert_eq!(chunks, vec!["short message"]);
    }

    #[test]
    fn client_construction() {
        let client = DiscordClient::new("test-token").unwrap();
        assert_eq!(client.token, "test-token");
    }

    #[test]
    fn reaction_url_construction() {
        let channel_id = "123";
        let message_id = "456";
        let emoji = "%F0%9F%91%8D"; // URL-encoded thumbs up
        let url = format!(
            "{DISCORD_API_BASE}/channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me"
        );
        assert_eq!(
            url,
            "https://discord.com/api/v10/channels/123/messages/456/reactions/%F0%9F%91%8D/@me"
        );
    }
}
