use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::json;
use tracing::warn;

use super::types::{CreateMessageRequest, CurrentUser, InteractionResponse};
use crate::chunker;
use crate::http_retry::RateLimitPolicy;

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const MESSAGE_CHUNK_SIZE: usize = 2000;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// A client for the Discord REST API.
#[derive(Clone)]
pub struct DiscordClient {
    client: Client,
    token: String,
}

impl DiscordClient {
    pub fn new(token: &str) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .context("Failed to build Discord HTTP client")?,
            token: token.to_string(),
        })
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
        let request = self.client.post(&url).json(response);

        self.send_with_retry(request).await?;
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
        let chunks = chunker::chunk_text_nonempty(text, MESSAGE_CHUNK_SIZE);

        // Edit the original response with the first chunk
        let url = format!(
            "{DISCORD_API_BASE}/webhooks/{application_id}/{interaction_token}/messages/@original"
        );
        let request = self
            .client
            .patch(&url)
            .json(&json!({ "content": chunks[0] }));
        self.send_with_retry(request).await?;

        // Send remaining chunks as follow-up messages
        for chunk in &chunks[1..] {
            let followup_url =
                format!("{DISCORD_API_BASE}/webhooks/{application_id}/{interaction_token}");
            let request = self
                .client
                .post(&followup_url)
                .json(&json!({ "content": chunk }));
            self.send_with_retry(request).await?;
        }

        Ok(())
    }

    /// Send a message to a channel, chunking at 2000 characters.
    pub async fn send_message(&self, channel_id: &str, text: &str) -> Result<()> {
        let chunks = chunker::chunk_text_nonempty(text, MESSAGE_CHUNK_SIZE);

        for chunk in &chunks {
            let url = format!("{DISCORD_API_BASE}/channels/{channel_id}/messages");
            let body = CreateMessageRequest {
                content: chunk.clone(),
            };
            let request = self
                .client
                .post(&url)
                .header("Authorization", format!("Bot {}", self.token))
                .json(&body);
            self.send_with_retry(request).await?;
        }

        Ok(())
    }

    /// Send a request with 429 rate-limit retry logic.
    async fn send_with_retry(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        // We need to clone the request for retries. reqwest::RequestBuilder
        // supports try_clone() for requests with cloneable bodies.
        let mut current_request = request;

        let policy = RateLimitPolicy {
            service_name: "Discord",
            ..RateLimitPolicy::default()
        };

        // Discord needs special handling because we must clone the RequestBuilder
        // before sending, since it's consumed on send.
        let mut attempts = 0u32;
        loop {
            let cloned = current_request
                .try_clone()
                .ok_or_else(|| anyhow::anyhow!("Request body is not cloneable for retry"))?;

            let resp = current_request
                .send()
                .await
                .context("Discord API request failed")?;

            let status = resp.status();
            if status.as_u16() == 429 {
                attempts += 1;
                if attempts > policy.max_retries {
                    bail!("Discord rate limited after {} retries", policy.max_retries);
                }
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1);
                let capped = retry_after.min(policy.max_retry_after_secs);
                warn!(
                    "Discord rate limited, retry after {capped}s (attempt {attempts}/{})",
                    policy.max_retries
                );
                tokio::time::sleep(Duration::from_secs(capped)).await;
                current_request = cloned;
                continue;
            }

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                bail!("Discord API error ({status}): {body}");
            }

            return Ok(resp);
        }
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
        let chunks = chunker::chunk_text(&long_text, MESSAGE_CHUNK_SIZE);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 2000);
        assert_eq!(chunks[1].len(), 2000);
        assert_eq!(chunks[2].len(), 500);
    }

    #[test]
    fn short_text_single_chunk() {
        let text = "short message";
        let chunks = chunker::chunk_text(text, MESSAGE_CHUNK_SIZE);
        assert_eq!(chunks, vec!["short message"]);
    }

    #[test]
    fn client_construction() {
        let client = DiscordClient::new("test-token").unwrap();
        assert_eq!(client.token, "test-token");
    }
}
