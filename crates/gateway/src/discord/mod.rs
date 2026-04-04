/// Discord Bot API HTTP client.
pub mod api;
/// Native channel implementation for Discord.
pub mod channel;
/// Discord interaction parsing into inbound messages.
pub mod parse;
/// Discord API type definitions.
pub mod types;
/// Typing indicator keepalive for Discord channels.
pub mod typing;
/// Ed25519 signature verification for interaction webhooks.
pub mod verify;

use anyhow::Result;
use axum::http::HeaderMap;

use crate::handler::InboundMessage;
use types::{Interaction, InteractionResponse, InteractionType};

/// Result of processing a Discord interaction webhook.
pub enum DiscordWebhookResult {
    /// Ping interaction — return the pong response as the HTTP body.
    Pong(InteractionResponse),
    /// A parsed inbound message with the original interaction (needed for responding).
    Message(InboundMessage, Box<Interaction>),
    /// Interaction was recognized but should be skipped (bot, autocomplete, etc.).
    Skip,
}

/// Handle a Discord interaction webhook request.
///
/// If `public_key_hex` is provided, the request signature is verified using Ed25519.
/// Returns `Pong` for ping interactions, `Message` for actionable interactions,
/// and `Skip` for everything else (bots, autocomplete, unknown types).
pub fn handle_discord_webhook(
    headers: &HeaderMap,
    body: &str,
    public_key_hex: Option<&str>,
) -> Result<DiscordWebhookResult> {
    // Verify signature if public key is configured
    if let Some(key) = public_key_hex {
        verify::verify_discord_signature(headers, body, key)?;
    }

    let interaction: Interaction = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("Invalid Discord interaction JSON: {e}"))?;

    // Handle Ping → Pong
    if interaction.interaction_type == InteractionType::Ping {
        return Ok(DiscordWebhookResult::Pong(InteractionResponse::pong()));
    }

    // Try to parse into an InboundMessage
    match parse::parse_interaction(&interaction) {
        Some(msg) => Ok(DiscordWebhookResult::Message(msg, Box::new(interaction))),
        None => Ok(DiscordWebhookResult::Skip),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use ed25519_dalek::{Signer, SigningKey};

    fn make_signed_headers(signing_key: &SigningKey, body: &str) -> (HeaderMap, String) {
        let timestamp = "1234567890";
        let message = format!("{timestamp}{body}");
        let signature = signing_key.sign(message.as_bytes());
        let sig_hex = hex::encode(signature.to_bytes());

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-signature-ed25519",
            HeaderValue::from_str(&sig_hex).unwrap(),
        );
        headers.insert(
            "x-signature-timestamp",
            HeaderValue::from_str(timestamp).unwrap(),
        );

        let public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
        (headers, public_key_hex)
    }

    #[test]
    fn ping_returns_pong() {
        let body = r#"{"id":"1","type":1,"token":"tok"}"#;
        let headers = HeaderMap::new();

        let result = handle_discord_webhook(&headers, body, None).unwrap();
        match result {
            DiscordWebhookResult::Pong(resp) => {
                assert_eq!(resp.response_type, 1);
            }
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn ping_with_valid_signature() {
        let body = r#"{"id":"1","type":1,"token":"tok"}"#;
        let signing_key = SigningKey::from_bytes(&[3u8; 32]);
        let (headers, pub_hex) = make_signed_headers(&signing_key, body);

        let result = handle_discord_webhook(&headers, body, Some(&pub_hex)).unwrap();
        match result {
            DiscordWebhookResult::Pong(resp) => {
                assert_eq!(resp.response_type, 1);
            }
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn slash_command_returns_message() {
        let body = r#"{
            "id": "2",
            "type": 2,
            "token": "tok",
            "data": { "id": "cmd1", "name": "ask", "options": [{"name":"q","value":"hello"}] },
            "member": { "user": { "id": "u1", "username": "alice" } },
            "channel_id": "ch1"
        }"#;
        let headers = HeaderMap::new();

        let result = handle_discord_webhook(&headers, body, None).unwrap();
        match result {
            DiscordWebhookResult::Message(msg, interaction) => {
                assert_eq!(msg.sender_id, "u1");
                assert_eq!(msg.text, "hello");
                assert_eq!(msg.channel_id.as_deref(), Some("ch1"));
                assert_eq!(interaction.token, "tok");
            }
            _ => panic!("expected Message"),
        }
    }

    #[test]
    fn invalid_json_returns_error() {
        let headers = HeaderMap::new();
        let result = handle_discord_webhook(&headers, "not json", None);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_signature_fails() {
        let body = r#"{"id":"1","type":1,"token":"tok"}"#;
        let signing_key = SigningKey::from_bytes(&[3u8; 32]);
        let wrong_key = SigningKey::from_bytes(&[4u8; 32]);
        let (headers, _) = make_signed_headers(&signing_key, body);
        let wrong_pub_hex = hex::encode(wrong_key.verifying_key().to_bytes());

        let result = handle_discord_webhook(&headers, body, Some(&wrong_pub_hex));
        assert!(result.is_err());
    }

    #[test]
    fn bot_user_returns_skip() {
        let body = r#"{
            "id": "3",
            "type": 2,
            "token": "tok",
            "data": { "name": "ask" },
            "user": { "id": "bot1", "username": "mybot", "bot": true }
        }"#;
        let headers = HeaderMap::new();

        let result = handle_discord_webhook(&headers, body, None).unwrap();
        assert!(matches!(result, DiscordWebhookResult::Skip));
    }

    #[test]
    fn autocomplete_returns_skip() {
        let body = r#"{
            "id": "4",
            "type": 4,
            "token": "tok",
            "data": { "name": "search" },
            "member": { "user": { "id": "u1", "username": "alice" } }
        }"#;
        let headers = HeaderMap::new();

        let result = handle_discord_webhook(&headers, body, None).unwrap();
        assert!(matches!(result, DiscordWebhookResult::Skip));
    }

    #[test]
    fn unknown_type_returns_skip() {
        let body = r#"{
            "id": "5",
            "type": 99,
            "token": "tok",
            "member": { "user": { "id": "u1", "username": "alice" } }
        }"#;
        let headers = HeaderMap::new();

        let result = handle_discord_webhook(&headers, body, None).unwrap();
        assert!(matches!(result, DiscordWebhookResult::Skip));
    }

    #[test]
    fn message_component_returns_message() {
        let body = r#"{
            "id": "6",
            "type": 3,
            "token": "tok",
            "data": { "custom_id": "btn_yes" },
            "user": { "id": "u2", "username": "bob" },
            "channel_id": "ch2"
        }"#;
        let headers = HeaderMap::new();

        let result = handle_discord_webhook(&headers, body, None).unwrap();
        match result {
            DiscordWebhookResult::Message(msg, _) => {
                assert_eq!(msg.text, "btn_yes");
                assert_eq!(msg.sender_id, "u2");
            }
            _ => panic!("expected Message"),
        }
    }
}
