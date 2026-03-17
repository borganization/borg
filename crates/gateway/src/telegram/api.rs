use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use tracing::warn;

use super::circuit_breaker::CircuitBreaker;
use super::types::{ApiResponse, FileInfo, SendMessageRequest, Update, User};
use crate::chunker;

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";

/// A client for the Telegram Bot API.
#[derive(Clone)]
pub struct TelegramClient {
    client: Client,
    token: String,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl TelegramClient {
    pub fn new(token: &str) -> Self {
        Self {
            client: Client::new(),
            token: token.to_string(),
            circuit_breaker: Arc::new(CircuitBreaker::new()),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{TELEGRAM_API_BASE}/bot{}/{method}", self.token)
    }

    fn file_url(&self, file_path: &str) -> String {
        format!("{TELEGRAM_API_BASE}/file/bot{}/{file_path}", self.token)
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
        message_thread_id: Option<i64>,
        reply_to_message_id: Option<i64>,
    ) -> Result<()> {
        let chunks = chunker::chunk_text(text, 4000);
        let chunks = if chunks.is_empty() {
            vec![text.to_string()]
        } else {
            chunks
        };

        for (i, chunk) in chunks.iter().enumerate() {
            // Only reply to the original message on the first chunk
            let reply_id = if i == 0 { reply_to_message_id } else { None };
            self.send_single_message(chat_id, chunk, parse_mode, message_thread_id, reply_id)
                .await?;
        }
        Ok(())
    }

    async fn send_single_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
        message_thread_id: Option<i64>,
        reply_to_message_id: Option<i64>,
    ) -> Result<()> {
        let body = SendMessageRequest {
            chat_id,
            text: text.to_string(),
            parse_mode: parse_mode.map(String::from),
            reply_to_message_id,
            message_thread_id,
        };

        const MAX_RETRIES: u32 = 5;
        const MAX_RETRY_AFTER_SECS: u64 = 300;
        let mut attempts = 0u32;

        loop {
            let send_result = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&body)
                .send()
                .await;

            let resp = match send_result {
                Ok(r) => r,
                Err(e) => {
                    if is_safe_to_retry(&e) {
                        attempts += 1;
                        if attempts > MAX_RETRIES {
                            bail!("sendMessage failed after {MAX_RETRIES} retries: {e}");
                        }
                        let backoff = Duration::from_millis(500 * 2u64.pow(attempts - 1));
                        warn!("sendMessage connection error, retrying in {backoff:?} (attempt {attempts}/{MAX_RETRIES}): {e}");
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    return Err(e).context("Failed to send message");
                }
            };

            let status = resp.status();
            let resp_body: ApiResponse<serde_json::Value> = resp
                .json()
                .await
                .context("Failed to parse sendMessage response")?;

            if resp_body.ok {
                return Ok(());
            }

            // Handle 429 rate limiting
            if status.as_u16() == 429 {
                attempts += 1;
                if attempts > MAX_RETRIES {
                    bail!("sendMessage rate limited after {MAX_RETRIES} retries");
                }
                if let Some(retry_after) = resp_body.retry_after {
                    let capped = retry_after.min(MAX_RETRY_AFTER_SECS);
                    warn!("Telegram rate limited, retry after {capped}s (attempt {attempts}/{MAX_RETRIES})");
                    tokio::time::sleep(Duration::from_secs(capped)).await;
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
    /// Uses circuit breaker to prevent infinite 401 loops.
    pub async fn send_typing(&self, chat_id: i64) -> Result<()> {
        if self.circuit_breaker.is_open() {
            return Ok(());
        }

        let body = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing",
        });

        let result = self
            .client
            .post(self.api_url("sendChatAction"))
            .json(&body)
            .send()
            .await;

        match result {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 401 {
                    self.circuit_breaker.record_failure(401);
                } else {
                    self.circuit_breaker.record_success();
                }
            }
            Err(_) => {
                // Network errors don't contribute to circuit breaker
            }
        }

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

    /// Get file info by file_id (for future media download support).
    pub async fn get_file(&self, file_id: &str) -> Result<FileInfo> {
        let body = serde_json::json!({ "file_id": file_id });
        let resp: ApiResponse<FileInfo> = self
            .client
            .post(self.api_url("getFile"))
            .json(&body)
            .send()
            .await
            .context("Failed to call getFile")?
            .json()
            .await
            .context("Failed to parse getFile response")?;

        match resp.result {
            Some(info) if resp.ok => Ok(info),
            _ => bail!(
                "getFile failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            ),
        }
    }

    /// Download a file by its file_path (obtained from get_file).
    pub async fn download_file(&self, file_path: &str) -> Result<Vec<u8>> {
        let bytes = self
            .client
            .get(self.file_url(file_path))
            .send()
            .await
            .context("Failed to download file")?
            .bytes()
            .await
            .context("Failed to read file bytes")?;

        Ok(bytes.to_vec())
    }

    /// Long-poll for updates (used when webhook is not configured).
    pub async fn get_updates(&self, offset: Option<i64>, timeout: u64) -> Result<Vec<Update>> {
        let mut body = serde_json::json!({
            "timeout": timeout,
            "allowed_updates": ["message", "edited_message", "callback_query"],
        });
        if let Some(off) = offset {
            body["offset"] = serde_json::json!(off);
        }

        let resp: ApiResponse<Vec<Update>> = self
            .client
            .post(self.api_url("getUpdates"))
            .json(&body)
            .timeout(Duration::from_secs(timeout + 10))
            .send()
            .await
            .context("Failed to call getUpdates")?
            .json()
            .await
            .context("Failed to parse getUpdates response")?;

        match resp.result {
            Some(updates) if resp.ok => Ok(updates),
            _ => bail!(
                "getUpdates failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            ),
        }
    }
}

/// Determine if a network error is safe to retry (no data was sent/received).
fn is_safe_to_retry(err: &reqwest::Error) -> bool {
    err.is_connect() || (err.is_timeout() && err.status().is_none())
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
    fn file_url_construction() {
        let client = TelegramClient::new("123:ABC");
        assert_eq!(
            client.file_url("photos/file_1.jpg"),
            "https://api.telegram.org/file/bot123:ABC/photos/file_1.jpg"
        );
    }

    #[test]
    fn send_message_request_serialization() {
        let req = SendMessageRequest {
            chat_id: 42,
            text: "hello".into(),
            parse_mode: Some("Markdown".into()),
            reply_to_message_id: None,
            message_thread_id: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["chat_id"], 42);
        assert_eq!(json["text"], "hello");
        assert_eq!(json["parse_mode"], "Markdown");
        assert!(json.get("reply_to_message_id").is_none());
        assert!(json.get("message_thread_id").is_none());
    }

    #[test]
    fn send_message_request_with_thread_and_reply() {
        let req = SendMessageRequest {
            chat_id: 42,
            text: "hello".into(),
            parse_mode: None,
            reply_to_message_id: Some(10),
            message_thread_id: Some(99),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["reply_to_message_id"], 10);
        assert_eq!(json["message_thread_id"], 99);
    }

    #[test]
    fn send_message_request_no_parse_mode() {
        let req = SendMessageRequest {
            chat_id: 42,
            text: "hello".into(),
            parse_mode: None,
            reply_to_message_id: None,
            message_thread_id: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("parse_mode").is_none());
    }

    #[test]
    fn chunking_integration() {
        let long_text: String = "a".repeat(8500);
        let chunks = chunker::chunk_text(&long_text, 4000);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 4000);
        assert_eq!(chunks[1].len(), 4000);
        assert_eq!(chunks[2].len(), 500);
    }
}
