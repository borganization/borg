pub mod api;
pub mod dedup;
pub mod echo;
pub mod format;
pub mod parse;
pub mod types;
pub mod typing;
pub mod verify;

use anyhow::Result;
use axum::http::HeaderMap;
use tokio::sync::Mutex;

use crate::handler::InboundMessage;
use dedup::EventDeduplicator;
use echo::EchoCache;
use types::SlackEnvelope;

/// Process an incoming Slack webhook request.
///
/// Flow: check for url_verification → verify signature → dedup → parse event → return.
/// Returns `Ok(SlackWebhookResult::Challenge(..))` for URL verification handshake.
/// Returns `Ok(SlackWebhookResult::Message(..))` for valid inbound messages.
/// Returns `Ok(SlackWebhookResult::Skip)` for bot messages, non-text events, etc.
pub enum SlackWebhookResult {
    /// URL verification challenge — return this string as the HTTP response.
    Challenge(String),
    /// A parsed inbound message ready for agent processing.
    Message(InboundMessage),
    /// Event was recognized but should be skipped (bot message, non-text, etc.).
    Skip,
}

/// Handle a Slack webhook request.
///
/// If `signing_secret` is provided, the request signature is verified on all requests.
/// If `dedup` is provided, duplicate events are detected and skipped.
/// If `bot_user_id` and `echo_cache` are provided, self-messages are filtered.
pub async fn handle_slack_webhook(
    headers: &HeaderMap,
    body: &str,
    signing_secret: Option<&str>,
    dedup: Option<&Mutex<EventDeduplicator>>,
    bot_user_id: Option<&str>,
    echo_cache: Option<&Mutex<EchoCache>>,
) -> Result<SlackWebhookResult> {
    // Verify signature before processing any payload
    if let Some(secret) = signing_secret {
        verify::verify_slack_signature(headers, body, secret)?;
    }

    let envelope: SlackEnvelope =
        serde_json::from_str(body).map_err(|e| anyhow::anyhow!("Invalid Slack event JSON: {e}"))?;

    match envelope {
        SlackEnvelope::UrlVerification { challenge } => {
            Ok(SlackWebhookResult::Challenge(challenge))
        }
        SlackEnvelope::EventCallback(callback) => {
            // Deduplicate events by event_id
            if let (Some(dedup), Some(event_id)) = (dedup, callback.event_id.as_deref()) {
                if dedup.lock().await.is_duplicate(event_id) {
                    return Ok(SlackWebhookResult::Skip);
                }
            }

            match parse::parse_event(&callback, bot_user_id, echo_cache).await {
                Some(msg) => Ok(SlackWebhookResult::Message(msg)),
                None => Ok(SlackWebhookResult::Skip),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::sync::Arc;

    type HmacSha256 = Hmac<Sha256>;

    fn compute_signature(secret: &str, timestamp: &str, body: &str) -> String {
        let sig_basestring = format!("v0:{timestamp}:{body}");
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key size");
        mac.update(sig_basestring.as_bytes());
        let result = mac.finalize();
        format!("v0={}", hex::encode(result.into_bytes()))
    }

    fn make_signed_headers(secret: &str, body: &str) -> HeaderMap {
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let sig = compute_signature(secret, &timestamp, body);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-slack-request-timestamp",
            HeaderValue::from_str(&timestamp).unwrap(),
        );
        headers.insert("x-slack-signature", HeaderValue::from_str(&sig).unwrap());
        headers
    }

    #[tokio::test]
    async fn url_verification_returns_challenge() {
        let body = r#"{"type":"url_verification","challenge":"abc123","token":"xyz"}"#;
        let headers = HeaderMap::new();

        let result = handle_slack_webhook(&headers, body, None, None, None, None)
            .await
            .unwrap();
        match result {
            SlackWebhookResult::Challenge(c) => assert_eq!(c, "abc123"),
            _ => panic!("expected Challenge"),
        }
    }

    #[tokio::test]
    async fn url_verification_requires_signature_when_secret_set() {
        let body = r#"{"type":"url_verification","challenge":"abc123","token":"xyz"}"#;
        let secret = "test-signing-secret";
        let headers = make_signed_headers(secret, body);

        let result = handle_slack_webhook(&headers, body, Some(secret), None, None, None)
            .await
            .unwrap();
        match result {
            SlackWebhookResult::Challenge(c) => assert_eq!(c, "abc123"),
            _ => panic!("expected Challenge"),
        }
    }

    #[tokio::test]
    async fn url_verification_fails_with_bad_signature() {
        let body = r#"{"type":"url_verification","challenge":"abc123","token":"xyz"}"#;
        let headers = HeaderMap::new(); // no signature headers

        let result = handle_slack_webhook(&headers, body, Some("secret"), None, None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn event_callback_without_secret() {
        let body = r#"{
            "type": "event_callback",
            "token": "tok",
            "team_id": "T123",
            "event_id": "Ev123",
            "event": {
                "type": "message",
                "user": "U456",
                "text": "hello",
                "ts": "1234567890.123456",
                "channel": "C789"
            }
        }"#;
        let headers = HeaderMap::new();

        let result = handle_slack_webhook(&headers, body, None, None, None, None)
            .await
            .unwrap();
        match result {
            SlackWebhookResult::Message(msg) => {
                assert_eq!(msg.sender_id, "U456");
                assert_eq!(msg.text, "hello");
                assert_eq!(msg.channel_id.as_deref(), Some("C789"));
            }
            _ => panic!("expected Message"),
        }
    }

    #[tokio::test]
    async fn event_callback_with_valid_signature() {
        let body = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev123","event":{"type":"message","user":"U456","text":"hello","ts":"1234567890.123456","channel":"C789"}}"#;
        let secret = "test-signing-secret";
        let headers = make_signed_headers(secret, body);

        let result = handle_slack_webhook(&headers, body, Some(secret), None, None, None)
            .await
            .unwrap();
        match result {
            SlackWebhookResult::Message(msg) => {
                assert_eq!(msg.text, "hello");
            }
            _ => panic!("expected Message"),
        }
    }

    #[tokio::test]
    async fn event_callback_with_invalid_signature_fails() {
        let body = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev123","event":{"type":"message","user":"U456","text":"hello","ts":"1234567890.123456","channel":"C789"}}"#;
        let headers = make_signed_headers("wrong-secret", body);

        let result =
            handle_slack_webhook(&headers, body, Some("correct-secret"), None, None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn bot_message_returns_skip() {
        let body = r#"{
            "type": "event_callback",
            "token": "tok",
            "team_id": "T123",
            "event_id": "Ev123",
            "event": {
                "type": "message",
                "text": "bot says hi",
                "ts": "1234567890.123456",
                "channel": "C789",
                "bot_id": "B123"
            }
        }"#;
        let headers = HeaderMap::new();

        let result = handle_slack_webhook(&headers, body, None, None, None, None)
            .await
            .unwrap();
        assert!(matches!(result, SlackWebhookResult::Skip));
    }

    #[tokio::test]
    async fn invalid_json_returns_error() {
        let headers = HeaderMap::new();
        let result = handle_slack_webhook(&headers, "not json", None, None, None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn duplicate_event_id_returns_skip() {
        let body = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev123","event":{"type":"message","user":"U456","text":"hello","ts":"1234567890.123456","channel":"C789"}}"#;
        let headers = HeaderMap::new();
        let dedup = Arc::new(Mutex::new(EventDeduplicator::new()));

        // First call should parse the message
        let result = handle_slack_webhook(&headers, body, None, Some(&dedup), None, None)
            .await
            .unwrap();
        assert!(matches!(result, SlackWebhookResult::Message(_)));

        // Second call with same event_id should be skipped
        let result = handle_slack_webhook(&headers, body, None, Some(&dedup), None, None)
            .await
            .unwrap();
        assert!(matches!(result, SlackWebhookResult::Skip));
    }
}
