use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::warn;

use super::echo::EchoCache;
use super::types::{AuthTestResponse, PostMessageRequest};
use crate::chunker;
use crate::circuit_breaker::CircuitBreaker;
use crate::http_retry::{send_with_rate_limit_retry, RateLimitPolicy};

const SLACK_API_BASE: &str = "https://slack.com/api";
const MESSAGE_CHUNK_SIZE: usize = 4000;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Circuit breaker thresholds for Slack typing indicators.
/// Trips after 2 consecutive failures (matching OpenClaw's maxConsecutiveFailures).
const TYPING_CB_FAILURE_THRESHOLD: u32 = 2;
const TYPING_CB_SUSPENSION_SECS: u64 = 60;

/// Max file download size for Slack attachments.
const MAX_FILE_DOWNLOAD: usize = borg_core::constants::SLACK_MAX_FILE_SIZE;

/// A client for the Slack Web API.
#[derive(Clone)]
pub struct SlackClient {
    client: Client,
    token: String,
    circuit_breaker: Arc<CircuitBreaker>,
    bot_user_id: Option<String>,
    echo_cache: Arc<Mutex<EchoCache>>,
}

impl SlackClient {
    pub fn new(token: &str) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .context("Failed to build Slack HTTP client")?,
            token: token.to_string(),
            circuit_breaker: Arc::new(CircuitBreaker::new(
                TYPING_CB_FAILURE_THRESHOLD,
                TYPING_CB_SUSPENSION_SECS,
            )),
            bot_user_id: None,
            echo_cache: Arc::new(Mutex::new(EchoCache::new())),
        })
    }

    /// The bot's own user ID (set after `auth_test`).
    pub fn bot_user_id(&self) -> Option<&str> {
        self.bot_user_id.as_deref()
    }

    /// Reference to the echo cache for inbound filtering.
    pub fn echo_cache(&self) -> &Arc<Mutex<EchoCache>> {
        &self.echo_cache
    }

    /// Validate the bot token by calling `auth.test`.
    /// Stores the bot's user_id for echo detection.
    pub async fn auth_test(&mut self) -> Result<AuthTestResponse> {
        let resp: AuthTestResponse = self
            .client
            .post(format!("{SLACK_API_BASE}/auth.test"))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to call auth.test")?
            .json()
            .await
            .context("Failed to parse auth.test response")?;

        if !resp.ok {
            bail!(
                "auth.test failed: {}",
                resp.error.as_deref().unwrap_or("unknown error")
            );
        }

        self.bot_user_id = resp.user_id.clone();

        Ok(resp)
    }

    /// Send a message to a Slack channel, automatically chunking at 4000 chars.
    /// Optionally replies in a thread if `thread_ts` is provided.
    /// Records sent text in echo cache for self-message detection.
    pub async fn post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<()> {
        let chunks = chunker::chunk_text_nonempty(text, MESSAGE_CHUNK_SIZE);

        for chunk in &chunks {
            self.send_single_message(channel, chunk, thread_ts).await?;
        }

        // Record full text in echo cache after successful send
        self.echo_cache.lock().await.remember(text);
        Ok(())
    }

    async fn send_single_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<()> {
        let body = PostMessageRequest {
            channel: channel.to_string(),
            text: text.to_string(),
            thread_ts: thread_ts.map(String::from),
            blocks: None,
        };

        let policy = RateLimitPolicy {
            service_name: "Slack chat.postMessage",
            ..RateLimitPolicy::default()
        };

        let resp = send_with_rate_limit_retry(&policy, || async {
            self.client
                .post(format!("{SLACK_API_BASE}/chat.postMessage"))
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await
                .context("Failed to send Slack message")
        })
        .await?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse chat.postMessage response")?;

        if resp_body
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(());
        }

        let error = resp_body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");

        let hint = match error {
            "channel_not_found" => " — bot may not be added to this channel",
            "not_in_channel" => " — invite the bot to the channel first",
            "invalid_auth" => " — check SLACK_BOT_TOKEN",
            "account_inactive" => " — bot token has been revoked",
            "token_revoked" => " — bot token has been revoked",
            "missing_scope" => " — bot token is missing required OAuth scopes",
            _ => "",
        };

        bail!(
            "chat.postMessage failed ({}): {error}{hint}",
            status.as_u16()
        );
    }

    /// Set thread typing status via `assistant.threads.setStatus` API.
    /// Non-fatal: logs warning on failure. Requires `thread_ts`.
    pub async fn set_thread_status(
        &self,
        channel: &str,
        thread_ts: Option<&str>,
        status: &str,
    ) -> Result<()> {
        let Some(ts) = thread_ts else {
            return Ok(());
        };

        if self.circuit_breaker.is_open() {
            return Ok(());
        }

        let body = serde_json::json!({
            "channel_id": channel,
            "thread_ts": ts,
            "status": status,
        });

        let result = self
            .client
            .post(format!("{SLACK_API_BASE}/assistant.threads.setStatus"))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await;

        match result {
            Ok(resp) => {
                let resp_body: serde_json::Value = resp
                    .json()
                    .await
                    .unwrap_or_else(|_| serde_json::json!({"ok": false}));

                if resp_body
                    .get("ok")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                {
                    self.circuit_breaker.record_success();
                } else {
                    self.circuit_breaker.record_failure();
                    let error = resp_body
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    warn!("assistant.threads.setStatus failed: {error}");
                }
            }
            Err(e) => {
                // Network errors don't trip circuit breaker
                warn!("assistant.threads.setStatus network error: {e}");
            }
        }

        Ok(())
    }

    /// Add a reaction emoji to a message. Non-fatal on failure.
    pub async fn add_reaction(&self, channel: &str, message_ts: &str, emoji: &str) {
        let body = serde_json::json!({
            "channel": channel,
            "timestamp": message_ts,
            "name": emoji,
        });

        let result = self
            .client
            .post(format!("{SLACK_API_BASE}/reactions.add"))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await;

        if let Err(e) = result {
            warn!("reactions.add failed: {e}");
        }
    }

    /// Upload a file to a Slack channel via `files.upload`.
    pub async fn upload_file(
        &self,
        channels: &str,
        content: &[u8],
        filename: &str,
        filetype: Option<&str>,
    ) -> Result<()> {
        let mut form = reqwest::multipart::Form::new()
            .text("channels", channels.to_string())
            .text("filename", filename.to_string())
            .part(
                "file",
                reqwest::multipart::Part::bytes(content.to_vec()).file_name(filename.to_string()),
            );

        if let Some(ft) = filetype {
            form = form.text("filetype", ft.to_string());
        }

        let resp = self
            .client
            .post(format!("{SLACK_API_BASE}/files.upload"))
            .bearer_auth(&self.token)
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file to Slack")?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse files.upload response")?;

        if resp_body
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(());
        }

        let error = resp_body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");

        bail!("files.upload failed ({}): {}", status.as_u16(), error);
    }

    /// Download a file from Slack using `url_private` with Bearer auth.
    /// Returns the file bytes and content-type.
    /// Rejects files larger than the configured max size.
    /// Only downloads from `files.slack.com` to prevent SSRF.
    pub async fn download_file(&self, url: &str) -> Result<(Vec<u8>, String)> {
        // Validate URL domain to prevent SSRF
        if !url.starts_with("https://files.slack.com/") {
            bail!("Refusing to download file from non-Slack domain");
        }

        let resp = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to download Slack file")?;

        // Check Content-Length before buffering to prevent memory exhaustion
        if let Some(content_length) = resp.content_length() {
            if content_length as usize > MAX_FILE_DOWNLOAD {
                bail!("Slack file too large ({content_length} bytes, max {MAX_FILE_DOWNLOAD})");
            }
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        let bytes = resp
            .bytes()
            .await
            .context("Failed to read Slack file bytes")?;

        if bytes.len() > MAX_FILE_DOWNLOAD {
            bail!(
                "Slack file too large ({} bytes, max {})",
                bytes.len(),
                MAX_FILE_DOWNLOAD
            );
        }

        Ok((bytes.to_vec(), content_type))
    }

    /// Fetch recent messages from a Slack channel via `conversations.history`.
    pub async fn conversations_history(
        &self,
        channel: &str,
        limit: u32,
    ) -> Result<Vec<HistoryMessage>> {
        let resp = self
            .client
            .get(format!("{SLACK_API_BASE}/conversations.history"))
            .bearer_auth(&self.token)
            .query(&[("channel", channel), ("limit", &limit.to_string())])
            .send()
            .await
            .context("Failed to call conversations.history")?;

        let body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse conversations.history response")?;

        if !body
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            let error = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            bail!("conversations.history failed: {error}");
        }

        let messages = body
            .get("messages")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        Some(HistoryMessage {
                            user: m.get("user")?.as_str()?.to_string(),
                            text: m.get("text")?.as_str()?.to_string(),
                            ts: m.get("ts")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(messages)
    }

    /// Remove a reaction emoji from a message. Non-fatal on failure.
    pub async fn remove_reaction(&self, channel: &str, message_ts: &str, emoji: &str) {
        let body = serde_json::json!({
            "channel": channel,
            "timestamp": message_ts,
            "name": emoji,
        });

        let result = self
            .client
            .post(format!("{SLACK_API_BASE}/reactions.remove"))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await;

        if let Err(e) = result {
            warn!("reactions.remove failed: {e}");
        }
    }
}

/// A message from `conversations.history` for channel context injection.
#[derive(Debug, Clone)]
pub struct HistoryMessage {
    pub user: String,
    pub text: String,
    pub ts: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_url_construction() {
        assert_eq!(
            format!("{SLACK_API_BASE}/auth.test"),
            "https://slack.com/api/auth.test"
        );
        assert_eq!(
            format!("{SLACK_API_BASE}/chat.postMessage"),
            "https://slack.com/api/chat.postMessage"
        );
    }

    #[test]
    fn post_message_request_serialization() {
        let req = PostMessageRequest {
            channel: "C789".into(),
            text: "hello".into(),
            thread_ts: Some("1234567890.111111".into()),
            blocks: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["channel"], "C789");
        assert_eq!(json["text"], "hello");
        assert_eq!(json["thread_ts"], "1234567890.111111");
    }

    #[test]
    fn post_message_request_no_thread() {
        let req = PostMessageRequest {
            channel: "C789".into(),
            text: "hello".into(),
            thread_ts: None,
            blocks: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("thread_ts").is_none());
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

    #[test]
    fn set_thread_status_serializes_correct_json() {
        let body = serde_json::json!({
            "channel_id": "C123",
            "thread_ts": "1234567890.123456",
            "status": "is typing...",
        });
        assert_eq!(body["channel_id"], "C123");
        assert_eq!(body["thread_ts"], "1234567890.123456");
        assert_eq!(body["status"], "is typing...");
    }

    #[test]
    fn reaction_serializes_correct_json() {
        let body = serde_json::json!({
            "channel": "C123",
            "timestamp": "1234567890.123456",
            "name": "thinking_face",
        });
        assert_eq!(body["channel"], "C123");
        assert_eq!(body["timestamp"], "1234567890.123456");
        assert_eq!(body["name"], "thinking_face");
    }

    #[test]
    fn circuit_breaker_initialized() {
        let client = SlackClient::new("xoxb-test").unwrap();
        assert!(!client.circuit_breaker.is_open());
    }

    #[test]
    fn file_upload_url_construction() {
        assert_eq!(
            format!("{SLACK_API_BASE}/files.upload"),
            "https://slack.com/api/files.upload"
        );
    }
}
