pub mod api;
pub mod channel;
pub mod parse;
pub mod types;
pub mod verify;

use anyhow::Result;
use axum::http::HeaderMap;

use crate::handler::InboundMessage;
use types::Activity;

/// Process an incoming Microsoft Teams webhook request.
///
/// Flow: verify HMAC signature (if secret provided) -> parse Activity -> convert to InboundMessage.
/// Returns `Ok(Some((InboundMessage, Activity)))` for valid message activities.
/// The `Activity` is returned alongside the message so the caller can extract `service_url`
/// and other fields needed to send a response.
/// Returns `Ok(None)` for non-message activities (conversationUpdate, typing, etc.).
pub fn handle_teams_webhook(
    headers: &HeaderMap,
    body: &str,
    app_secret: Option<&str>,
) -> Result<Option<(InboundMessage, Activity)>> {
    // Verify HMAC signature if secret is provided
    if let Some(secret) = app_secret {
        verify::verify_teams_signature(headers, body.as_bytes(), secret)?;
    }

    let activity: Activity = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("Invalid Teams activity JSON: {e}"))?;

    match parse::parse_activity(&activity) {
        Some(msg) => Ok(Some((msg, activity))),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    fn make_secret() -> String {
        base64::engine::general_purpose::STANDARD.encode(b"test-secret-key-32bytes-long!!")
    }

    fn compute_signature(secret_b64: &str, body: &[u8]) -> String {
        let secret_bytes = base64::engine::general_purpose::STANDARD
            .decode(secret_b64)
            .unwrap();
        let mut mac = HmacSha256::new_from_slice(&secret_bytes).expect("HMAC accepts any key size");
        mac.update(body);
        let result = mac.finalize().into_bytes();
        base64::engine::general_purpose::STANDARD.encode(result)
    }

    fn make_signed_headers(secret_b64: &str, body: &str) -> HeaderMap {
        let sig = compute_signature(secret_b64, body.as_bytes());
        let mut headers = HeaderMap::new();
        let value = format!("HMAC {sig}");
        headers.insert("authorization", HeaderValue::from_str(&value).unwrap());
        headers
    }

    fn message_body() -> String {
        serde_json::json!({
            "id": "act-1",
            "type": "message",
            "text": "hello bot",
            "from": {"id": "user-1", "name": "Alice"},
            "conversation": {"id": "conv-1"},
            "recipient": {"id": "bot-1", "name": "MyBot"},
            "serviceUrl": "https://smba.trafficmanager.net/teams/"
        })
        .to_string()
    }

    #[test]
    fn full_flow_without_signature() {
        let body = message_body();
        let headers = HeaderMap::new();

        let result = handle_teams_webhook(&headers, &body, None).unwrap();
        let (msg, activity) = result.unwrap();
        assert_eq!(msg.sender_id, "user-1");
        assert_eq!(msg.text, "hello bot");
        assert_eq!(msg.channel_id.as_deref(), Some("conv-1"));
        assert_eq!(
            activity.service_url.as_deref(),
            Some("https://smba.trafficmanager.net/teams/")
        );
    }

    #[test]
    fn full_flow_with_valid_signature() {
        let body = message_body();
        let secret = make_secret();
        let headers = make_signed_headers(&secret, &body);

        let result = handle_teams_webhook(&headers, &body, Some(&secret)).unwrap();
        let (msg, _) = result.unwrap();
        assert_eq!(msg.sender_id, "user-1");
        assert_eq!(msg.text, "hello bot");
    }

    #[test]
    fn invalid_signature_fails() {
        let body = message_body();
        let secret = make_secret();
        let wrong_secret =
            base64::engine::general_purpose::STANDARD.encode(b"wrong-key!!!!!!!!!!!!!!!!!!!!!!!");
        let headers = make_signed_headers(&wrong_secret, &body);

        let result = handle_teams_webhook(&headers, &body, Some(&secret));
        assert!(result.is_err());
    }

    #[test]
    fn non_message_activity_returns_none() {
        let body = serde_json::json!({
            "id": "act-2",
            "type": "conversationUpdate",
            "from": {"id": "user-1"},
            "conversation": {"id": "conv-1"},
            "serviceUrl": "https://smba.trafficmanager.net/teams/"
        })
        .to_string();
        let headers = HeaderMap::new();

        let result = handle_teams_webhook(&headers, &body, None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn invalid_json_returns_error() {
        let headers = HeaderMap::new();
        let result = handle_teams_webhook(&headers, "not json", None);
        assert!(result.is_err());
    }

    #[test]
    fn bot_self_message_returns_none() {
        let body = serde_json::json!({
            "id": "act-3",
            "type": "message",
            "text": "I am the bot",
            "from": {"id": "bot-1", "name": "MyBot"},
            "conversation": {"id": "conv-1"},
            "recipient": {"id": "bot-1", "name": "MyBot"},
            "serviceUrl": "https://smba.trafficmanager.net/teams/"
        })
        .to_string();
        let headers = HeaderMap::new();

        let result = handle_teams_webhook(&headers, &body, None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn missing_auth_header_with_secret_fails() {
        let body = message_body();
        let secret = make_secret();
        let headers = HeaderMap::new();

        let result = handle_teams_webhook(&headers, &body, Some(&secret));
        assert!(result.is_err());
    }
}
