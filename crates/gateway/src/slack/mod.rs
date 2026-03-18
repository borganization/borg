pub mod api;
pub mod parse;
pub mod types;
pub mod typing;
pub mod verify;

use anyhow::Result;
use axum::http::HeaderMap;

use crate::handler::InboundMessage;
use types::SlackEnvelope;

/// Process an incoming Slack webhook request.
///
/// Flow: check for url_verification → verify signature → parse event → return.
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
pub fn handle_slack_webhook(
    headers: &HeaderMap,
    body: &str,
    signing_secret: Option<&str>,
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
        SlackEnvelope::EventCallback(callback) => match parse::parse_event(&callback) {
            Some(msg) => Ok(SlackWebhookResult::Message(msg)),
            None => Ok(SlackWebhookResult::Skip),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

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

    #[test]
    fn url_verification_returns_challenge() {
        let body = r#"{"type":"url_verification","challenge":"abc123","token":"xyz"}"#;
        let headers = HeaderMap::new();

        let result = handle_slack_webhook(&headers, body, None).unwrap();
        match result {
            SlackWebhookResult::Challenge(c) => assert_eq!(c, "abc123"),
            _ => panic!("expected Challenge"),
        }
    }

    #[test]
    fn url_verification_requires_signature_when_secret_set() {
        let body = r#"{"type":"url_verification","challenge":"abc123","token":"xyz"}"#;
        let secret = "test-signing-secret";
        let headers = make_signed_headers(secret, body);

        let result = handle_slack_webhook(&headers, body, Some(secret)).unwrap();
        match result {
            SlackWebhookResult::Challenge(c) => assert_eq!(c, "abc123"),
            _ => panic!("expected Challenge"),
        }
    }

    #[test]
    fn url_verification_fails_with_bad_signature() {
        let body = r#"{"type":"url_verification","challenge":"abc123","token":"xyz"}"#;
        let headers = HeaderMap::new(); // no signature headers

        let result = handle_slack_webhook(&headers, body, Some("secret"));
        assert!(result.is_err());
    }

    #[test]
    fn event_callback_without_secret() {
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

        let result = handle_slack_webhook(&headers, body, None).unwrap();
        match result {
            SlackWebhookResult::Message(msg) => {
                assert_eq!(msg.sender_id, "U456");
                assert_eq!(msg.text, "hello");
                assert_eq!(msg.channel_id.as_deref(), Some("C789"));
            }
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn event_callback_with_valid_signature() {
        let body = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev123","event":{"type":"message","user":"U456","text":"hello","ts":"1234567890.123456","channel":"C789"}}"#;
        let secret = "test-signing-secret";
        let headers = make_signed_headers(secret, body);

        let result = handle_slack_webhook(&headers, body, Some(secret)).unwrap();
        match result {
            SlackWebhookResult::Message(msg) => {
                assert_eq!(msg.text, "hello");
            }
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn event_callback_with_invalid_signature_fails() {
        let body = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev123","event":{"type":"message","user":"U456","text":"hello","ts":"1234567890.123456","channel":"C789"}}"#;
        let headers = make_signed_headers("wrong-secret", body);

        let result = handle_slack_webhook(&headers, body, Some("correct-secret"));
        assert!(result.is_err());
    }

    #[test]
    fn bot_message_returns_skip() {
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

        let result = handle_slack_webhook(&headers, body, None).unwrap();
        assert!(matches!(result, SlackWebhookResult::Skip));
    }

    #[test]
    fn invalid_json_returns_error() {
        let headers = HeaderMap::new();
        let result = handle_slack_webhook(&headers, "not json", None);
        assert!(result.is_err());
    }
}
