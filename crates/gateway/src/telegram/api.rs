use anyhow::{bail, Context, Result};
use reqwest::Client;
use tracing::warn;

use super::types::{ApiResponse, SendMessageRequest, User};
use crate::chunker;

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";

/// A client for the Telegram Bot API.
#[derive(Clone)]
pub struct TelegramClient {
    client: Client,
    token: String,
}

impl TelegramClient {
    pub fn new(token: &str) -> Self {
        Self {
            client: Client::new(),
            token: token.to_string(),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{TELEGRAM_API_BASE}/bot{}/{method}", self.token)
    }

    /// Validate the bot token by calling getMe.
    pub async fn get_me(&self) -> Result<User> {
        let resp: ApiResponse<User> = self
            .client
            .get(self.api_url("getMe"))
            .send()
            .await
            .context("Failed to call getMe")?
            .json()
            .await
            .context("Failed to parse getMe response")?;

        match resp.result {
            Some(user) if resp.ok => Ok(user),
            _ => bail!(
                "getMe failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            ),
        }
    }

    /// Send a text message, automatically chunking if it exceeds 4000 chars.
    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<()> {
        let chunks = chunker::chunk_text(text, 4000);
        let chunks = if chunks.is_empty() {
            vec![text.to_string()]
        } else {
            chunks
        };

        for chunk in &chunks {
            self.send_single_message(chat_id, chunk, parse_mode).await?;
        }
        Ok(())
    }

    async fn send_single_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<()> {
        let body = SendMessageRequest {
            chat_id,
            text: text.to_string(),
            parse_mode: parse_mode.map(String::from),
        };

        const MAX_RETRIES: u32 = 5;
        const MAX_RETRY_AFTER_SECS: u64 = 300;
        let mut attempts = 0u32;

        loop {
            let resp = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&body)
                .send()
                .await
                .context("Failed to send message")?;

            let status = resp.status();
            let resp_body: ApiResponse<serde_json::Value> = resp
                .json()
                .await
                .context("Failed to parse sendMessage response")?;

            if resp_body.ok {
                return Ok(());
            }

            // Handle 429 rate limiting — wait and retry (bounded)
            if status.as_u16() == 429 {
                attempts += 1;
                if attempts > MAX_RETRIES {
                    bail!("sendMessage rate limited after {MAX_RETRIES} retries");
                }
                if let Some(retry_after) = resp_body.retry_after {
                    let capped = retry_after.min(MAX_RETRY_AFTER_SECS);
                    warn!("Telegram rate limited, retry after {capped}s (attempt {attempts}/{MAX_RETRIES})");
                    tokio::time::sleep(std::time::Duration::from_secs(capped)).await;
                    continue;
                }
            }

            bail!(
                "sendMessage failed ({}): {}",
                status.as_u16(),
                resp_body
                    .description
                    .unwrap_or_else(|| "unknown error".into())
            );
        }
    }

    /// Send a "typing" chat action.
    pub async fn send_typing(&self, chat_id: i64) -> Result<()> {
        let body = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing",
        });

        let _ = self
            .client
            .post(self.api_url("sendChatAction"))
            .json(&body)
            .send()
            .await;

        Ok(())
    }

    /// Register a webhook URL with Telegram.
    pub async fn set_webhook(&self, url: &str, secret_token: Option<&str>) -> Result<()> {
        let mut body = serde_json::json!({ "url": url });
        if let Some(secret) = secret_token {
            body["secret_token"] = serde_json::json!(secret);
        }

        let resp: ApiResponse<bool> = self
            .client
            .post(self.api_url("setWebhook"))
            .json(&body)
            .send()
            .await
            .context("Failed to call setWebhook")?
            .json()
            .await
            .context("Failed to parse setWebhook response")?;

        if !resp.ok {
            bail!(
                "setWebhook failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        Ok(())
    }

    /// Remove the webhook.
    pub async fn delete_webhook(&self) -> Result<()> {
        let resp: ApiResponse<bool> = self
            .client
            .post(self.api_url("deleteWebhook"))
            .send()
            .await
            .context("Failed to call deleteWebhook")?
            .json()
            .await
            .context("Failed to parse deleteWebhook response")?;

        if !resp.ok {
            bail!(
                "deleteWebhook failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_url_construction() {
        let client = TelegramClient::new("123:ABC");
        assert_eq!(
            client.api_url("getMe"),
            "https://api.telegram.org/bot123:ABC/getMe"
        );
        assert_eq!(
            client.api_url("sendMessage"),
            "https://api.telegram.org/bot123:ABC/sendMessage"
        );
    }

    #[test]
    fn send_message_request_serialization() {
        let req = SendMessageRequest {
            chat_id: 42,
            text: "hello".into(),
            parse_mode: Some("Markdown".into()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["chat_id"], 42);
        assert_eq!(json["text"], "hello");
        assert_eq!(json["parse_mode"], "Markdown");
    }

    #[test]
    fn send_message_request_no_parse_mode() {
        let req = SendMessageRequest {
            chat_id: 42,
            text: "hello".into(),
            parse_mode: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("parse_mode").is_none());
    }

    #[test]
    fn chunking_integration() {
        // Verify that a long message would be chunked at 4000 chars
        let long_text: String = "a".repeat(8500);
        let chunks = chunker::chunk_text(&long_text, 4000);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 4000);
        assert_eq!(chunks[1].len(), 4000);
        assert_eq!(chunks[2].len(), 500);
    }
}
