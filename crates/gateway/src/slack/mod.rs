pub mod api;
pub mod dedup;
pub mod mrkdwn;
pub mod parse;
pub mod types;
pub mod typing;
pub mod verify;

use anyhow::Result;
use axum::http::HeaderMap;

use crate::handler::InboundMessage;
use dedup::EventDeduplicator;
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
/// If `dedup` is provided, duplicate events (by event_id) are skipped.
/// Slack retry headers (`X-Slack-Retry-Num` with reason `http_timeout`) are auto-skipped.
pub fn handle_slack_webhook(
    headers: &HeaderMap,
    body: &str,
    signing_secret: Option<&str>,
    mut dedup: Option<&mut EventDeduplicator>,
    bot_user_id: Option<&str>,
) -> Result<SlackWebhookResult> {
    // Skip Slack retries caused by slow responses — we already received the event
    if is_slack_http_timeout_retry(headers) {
        return Ok(SlackWebhookResult::Skip);
    }

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
            // Deduplicate by event_id if deduplicator is provided
            if let Some(ref mut dedup) = dedup {
                if let Some(ref event_id) = callback.event_id {
                    if dedup.is_duplicate(event_id) {
                        return Ok(SlackWebhookResult::Skip);
                    }
                }
            }

            match parse::parse_event(&callback, bot_user_id) {
                Some(msg) => Ok(SlackWebhookResult::Message(msg)),
                None => Ok(SlackWebhookResult::Skip),
            }
        }
    }
}

/// Result of processing a Slack slash command.
pub enum SlashCommandResult {
    /// A parsed inbound message with optional response_url for async reply.
    Command(InboundMessage, Option<String>),
}

/// Handle a Slack slash command request (application/x-www-form-urlencoded).
///
/// Slack slash commands must receive a 200 within 3 seconds. The actual response
/// should be sent asynchronously via `response_url`.
pub fn handle_slash_command(
    headers: &HeaderMap,
    body: &str,
    signing_secret: Option<&str>,
) -> Result<SlashCommandResult> {
    // Verify signature before processing
    if let Some(secret) = signing_secret {
        verify::verify_slack_signature(headers, body, secret)?;
    }

    let payload: types::SlashCommandPayload = serde_urlencoded::from_str(body)
        .map_err(|e| anyhow::anyhow!("Invalid slash command form data: {e}"))?;

    let text = match &payload.text {
        Some(t) if !t.is_empty() => format!("{} {}", payload.command, t),
        _ => payload.command.clone(),
    };

    let msg = InboundMessage {
        sender_id: payload.user_id.clone(),
        text,
        channel_id: Some(payload.channel_id.clone()),
        thread_id: None,
        message_id: None,
        thread_ts: None,
        attachments: Vec::new(),
        reaction: None,
        metadata: serde_json::Value::Null,
    };

    Ok(SlashCommandResult::Command(msg, payload.response_url))
}

/// Check if the request is a Slack retry due to HTTP timeout.
/// Slack sets `X-Slack-Retry-Num` and `X-Slack-Retry-Reason` headers on retries.
fn is_slack_http_timeout_retry(headers: &HeaderMap) -> bool {
    if headers.get("x-slack-retry-num").is_none() {
        return false;
    }
    headers
        .get("x-slack-retry-reason")
        .and_then(|v| v.to_str().ok())
        .map(|reason| reason == "http_timeout")
        .unwrap_or(false)
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

        let result = handle_slack_webhook(&headers, body, None, None, None).unwrap();
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

        let result = handle_slack_webhook(&headers, body, Some(secret), None, None).unwrap();
        match result {
            SlackWebhookResult::Challenge(c) => assert_eq!(c, "abc123"),
            _ => panic!("expected Challenge"),
        }
    }

    #[test]
    fn url_verification_fails_with_bad_signature() {
        let body = r#"{"type":"url_verification","challenge":"abc123","token":"xyz"}"#;
        let headers = HeaderMap::new(); // no signature headers

        let result = handle_slack_webhook(&headers, body, Some("secret"), None, None);
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

        let result = handle_slack_webhook(&headers, body, None, None, None).unwrap();
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

        let result = handle_slack_webhook(&headers, body, Some(secret), None, None).unwrap();
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

        let result = handle_slack_webhook(&headers, body, Some("correct-secret"), None, None);
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

        let result = handle_slack_webhook(&headers, body, None, None, None).unwrap();
        assert!(matches!(result, SlackWebhookResult::Skip));
    }

    #[test]
    fn invalid_json_returns_error() {
        let headers = HeaderMap::new();
        let result = handle_slack_webhook(&headers, "not json", None, None, None);
        assert!(result.is_err());
    }

    // ── Dedup tests ───────────────────────────────────────────────────

    #[test]
    fn retry_header_skips_http_timeout() {
        let body = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev123","event":{"type":"message","user":"U456","text":"hello","ts":"1234567890.123456","channel":"C789"}}"#;
        let mut headers = HeaderMap::new();
        headers.insert("x-slack-retry-num", HeaderValue::from_static("1"));
        headers.insert(
            "x-slack-retry-reason",
            HeaderValue::from_static("http_timeout"),
        );

        let result = handle_slack_webhook(&headers, body, None, None, None).unwrap();
        assert!(matches!(result, SlackWebhookResult::Skip));
    }

    #[test]
    fn retry_other_reason_not_skipped() {
        let body = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev123","event":{"type":"message","user":"U456","text":"hello","ts":"1234567890.123456","channel":"C789"}}"#;
        let mut headers = HeaderMap::new();
        headers.insert("x-slack-retry-num", HeaderValue::from_static("1"));
        headers.insert(
            "x-slack-retry-reason",
            HeaderValue::from_static("some_other_reason"),
        );

        let result = handle_slack_webhook(&headers, body, None, None, None).unwrap();
        assert!(matches!(result, SlackWebhookResult::Message(_)));
    }

    #[test]
    fn duplicate_event_id_skips() {
        let body = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev999","event":{"type":"message","user":"U456","text":"hello","ts":"1234567890.123456","channel":"C789"}}"#;
        let headers = HeaderMap::new();
        let mut dedup = EventDeduplicator::new();

        // First call — not duplicate
        let result = handle_slack_webhook(&headers, body, None, Some(&mut dedup), None).unwrap();
        assert!(matches!(result, SlackWebhookResult::Message(_)));

        // Second call — duplicate
        let result = handle_slack_webhook(&headers, body, None, Some(&mut dedup), None).unwrap();
        assert!(matches!(result, SlackWebhookResult::Skip));
    }

    #[test]
    fn non_duplicate_event_id_passes() {
        let body1 = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev001","event":{"type":"message","user":"U456","text":"hello","ts":"1234567890.123456","channel":"C789"}}"#;
        let body2 = r#"{"type":"event_callback","token":"tok","team_id":"T123","event_id":"Ev002","event":{"type":"message","user":"U456","text":"world","ts":"1234567890.654321","channel":"C789"}}"#;
        let headers = HeaderMap::new();
        let mut dedup = EventDeduplicator::new();

        let result = handle_slack_webhook(&headers, body1, None, Some(&mut dedup), None).unwrap();
        assert!(matches!(result, SlackWebhookResult::Message(_)));

        let result = handle_slack_webhook(&headers, body2, None, Some(&mut dedup), None).unwrap();
        assert!(matches!(result, SlackWebhookResult::Message(_)));
    }

    // ── Slash command tests ───────────────────────────────────────────

    #[test]
    fn slash_command_parses_into_inbound() {
        let body = "command=%2Fborg&text=hello+world&user_id=U123&channel_id=C456";
        let headers = HeaderMap::new();

        let result = handle_slash_command(&headers, body, None).unwrap();
        let SlashCommandResult::Command(msg, response_url) = result;
        assert_eq!(msg.sender_id, "U123");
        assert_eq!(msg.text, "/borg hello world");
        assert_eq!(msg.channel_id.as_deref(), Some("C456"));
        assert!(response_url.is_none());
    }

    #[test]
    fn slash_command_with_response_url() {
        let body = "command=%2Fborg&text=deploy&user_id=U123&channel_id=C456\
            &response_url=https%3A%2F%2Fhooks.slack.com%2Fcommands%2Fxyz";
        let headers = HeaderMap::new();

        let result = handle_slash_command(&headers, body, None).unwrap();
        let SlashCommandResult::Command(_, response_url) = result;
        assert_eq!(
            response_url.as_deref(),
            Some("https://hooks.slack.com/commands/xyz")
        );
    }

    #[test]
    fn slash_command_empty_text() {
        let body = "command=%2Fborg&user_id=U123&channel_id=C456";
        let headers = HeaderMap::new();

        let result = handle_slash_command(&headers, body, None).unwrap();
        let SlashCommandResult::Command(msg, _) = result;
        assert_eq!(msg.text, "/borg");
    }

    #[test]
    fn slash_command_verifies_signature() {
        let body = "command=%2Fborg&text=hello&user_id=U123&channel_id=C456";
        let secret = "test-signing-secret";
        let headers = make_signed_headers(secret, body);

        let result = handle_slash_command(&headers, body, Some(secret)).unwrap();
        let SlashCommandResult::Command(msg, _) = result;
        assert_eq!(msg.text, "/borg hello");
    }

    #[test]
    fn slash_command_fails_with_bad_signature() {
        let body = "command=%2Fborg&text=hello&user_id=U123&channel_id=C456";
        let headers = HeaderMap::new();

        let result = handle_slash_command(&headers, body, Some("secret"));
        assert!(result.is_err());
    }

    #[test]
    fn slash_command_invalid_form_data() {
        let headers = HeaderMap::new();
        // Missing required fields
        let result = handle_slash_command(&headers, "invalid=data", None);
        assert!(result.is_err());
    }
}
