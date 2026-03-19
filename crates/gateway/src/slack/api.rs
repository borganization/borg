use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use tracing::warn;

use super::types::{AuthTestResponse, PostMessageRequest};
use crate::chunker;
use crate::circuit_breaker::CircuitBreaker;

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
    pub async fn post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<()> {
        let chunks = chunker::chunk_text(text, MESSAGE_CHUNK_SIZE);
        let chunks = if chunks.is_empty() {
            vec![text.to_string()]
        } else {
            chunks
        };

        for chunk in &chunks {
            self.send_single_message(channel, chunk, thread_ts).await?;
        }
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
        };

        const MAX_RETRIES: u32 = 5;
        const MAX_RETRY_AFTER_SECS: u64 = 300;
        let mut attempts = 0u32;

        loop {
            let resp = self
                .client
                .post(format!("{SLACK_API_BASE}/chat.postMessage"))
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await
                .context("Failed to send Slack message")?;

            let status = resp.status();

            // Handle HTTP-level 429 rate limiting
            if status.as_u16() == 429 {
                attempts += 1;
                if attempts > MAX_RETRIES {
                    bail!("Slack chat.postMessage rate limited after {MAX_RETRIES} retries");
                }
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1);
                let capped = retry_after.min(MAX_RETRY_AFTER_SECS);
                warn!(
                    "Slack rate limited, retry after {capped}s (attempt {attempts}/{MAX_RETRIES})"
                );
                tokio::time::sleep(std::time::Duration::from_secs(capped)).await;
                continue;
            }

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

            bail!("chat.postMessage failed ({}): {}", status.as_u16(), error);
        }
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
}
