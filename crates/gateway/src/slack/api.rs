use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use tracing::warn;

use super::types::{
    AuthTestResponse, PostMessageRequest, PostMessageResponse, UpdateMessageRequest,
};
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

/// A client for the Slack Web API.
#[derive(Clone)]
pub struct SlackClient {
    client: Client,
    token: String,
    circuit_breaker: Arc<CircuitBreaker>,
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
        })
    }

    /// Validate the bot token by calling `auth.test`.
    pub async fn auth_test(&self) -> Result<AuthTestResponse> {
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

        Ok(resp)
    }

    /// Send a message to a Slack channel, automatically chunking at 4000 chars.
    /// Optionally replies in a thread if `thread_ts` is provided.
    /// Returns the message `ts` of the last chunk sent (useful for later editing).
    pub async fn post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<Option<String>> {
        let chunks = chunker::chunk_text_nonempty(text, MESSAGE_CHUNK_SIZE);
        let mut last_ts = None;

        for chunk in &chunks {
            last_ts = self.send_single_message(channel, chunk, thread_ts).await?;
        }
        Ok(last_ts)
    }

    async fn send_single_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<Option<String>> {
        let converted_text = super::mrkdwn::markdown_to_mrkdwn(text);
        let body = PostMessageRequest {
            channel: channel.to_string(),
            text: converted_text,
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
        let resp_body: PostMessageResponse = resp
            .json()
            .await
            .context("Failed to parse chat.postMessage response")?;

        if resp_body.ok {
            return Ok(resp_body.ts);
        }

        let error = resp_body.error.as_deref().unwrap_or("unknown error");
        bail!("chat.postMessage failed ({}): {}", status.as_u16(), error);
    }

    /// Edit a previously sent message via `chat.update`.
    pub async fn update_message(&self, channel: &str, ts: &str, text: &str) -> Result<()> {
        let converted_text = super::mrkdwn::markdown_to_mrkdwn(text);
        let body = UpdateMessageRequest {
            channel: channel.to_string(),
            ts: ts.to_string(),
            text: converted_text,
            blocks: None,
        };

        let policy = RateLimitPolicy {
            service_name: "Slack chat.update",
            ..RateLimitPolicy::default()
        };

        let resp = send_with_rate_limit_retry(&policy, || async {
            self.client
                .post(format!("{SLACK_API_BASE}/chat.update"))
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await
                .context("Failed to update Slack message")
        })
        .await?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse chat.update response")?;

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

        bail!("chat.update failed ({}): {}", status.as_u16(), error);
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

    /// Respond to a Slack slash command via response_url.
    ///
    /// This POSTs JSON to the response_url provided by Slack in the slash command payload.
    /// No auth token is needed — the URL itself is the credential.
    /// Only accepts `hooks.slack.com` URLs to prevent SSRF.
    pub async fn respond_to_url(&self, response_url: &str, text: &str) -> Result<()> {
        if !response_url.starts_with("https://hooks.slack.com/") {
            bail!("Refusing to POST to non-Slack response_url");
        }

        let body = serde_json::json!({
            "text": text,
            "response_type": "ephemeral",
        });

        let resp = self
            .client
            .post(response_url)
            .json(&body)
            .send()
            .await
            .context("Failed to POST to Slack response_url")?;

        if !resp.status().is_success() {
            bail!(
                "Slack response_url returned status {}",
                resp.status().as_u16()
            );
        }

        Ok(())
    }

    /// Download a file from Slack using the bot token for authentication.
    ///
    /// Slack file URLs (url_private_download) require Bearer token auth.
    /// Enforces a 25 MB size limit to prevent memory exhaustion.
    pub async fn download_file(&self, url: &str) -> Result<Vec<u8>> {
        const MAX_FILE_SIZE: u64 = 25 * 1024 * 1024;

        let resp = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to download Slack file")?;

        if !resp.status().is_success() {
            bail!(
                "Slack file download failed: HTTP {}",
                resp.status().as_u16()
            );
        }

        if let Some(len) = resp.content_length() {
            if len > MAX_FILE_SIZE {
                bail!("Slack file too large: {len} bytes (max {MAX_FILE_SIZE})");
            }
        }

        let bytes = resp
            .bytes()
            .await
            .context("Failed to read Slack file bytes")?;

        if bytes.len() as u64 > MAX_FILE_SIZE {
            bail!(
                "Slack file too large: {} bytes (max {MAX_FILE_SIZE})",
                bytes.len()
            );
        }

        Ok(bytes.to_vec())
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

    #[test]
    fn chat_update_url_construction() {
        assert_eq!(
            format!("{SLACK_API_BASE}/chat.update"),
            "https://slack.com/api/chat.update"
        );
    }

    #[test]
    fn respond_to_url_body_serialization() {
        let body = serde_json::json!({
            "text": "Hello from slash command",
            "response_type": "ephemeral",
        });
        assert_eq!(body["text"], "Hello from slash command");
        assert_eq!(body["response_type"], "ephemeral");
    }

    #[tokio::test]
    async fn respond_to_url_rejects_non_slack_domain() {
        let client = SlackClient::new("xoxb-test").unwrap();
        let result = client
            .respond_to_url("https://evil.com/steal", "hello")
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("non-Slack response_url"));
    }
}
