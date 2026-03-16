pub mod api;
pub mod dedup;
pub mod parse;
pub mod types;
pub mod verify;

use anyhow::Result;
use axum::http::HeaderMap;
use tokio::sync::Mutex;

use crate::handler::InboundMessage;
use dedup::UpdateDeduplicator;

/// Process an incoming Telegram webhook request.
///
/// Flow: validate body → verify secret → dedup → parse → return.
/// Returns `Ok(None)` for non-text updates or duplicates.
pub async fn handle_telegram_webhook(
    headers: &HeaderMap,
    body: &str,
    secret: Option<&str>,
    dedup: &Mutex<UpdateDeduplicator>,
) -> Result<Option<InboundMessage>> {
    // Validate and parse the update JSON
    let update = verify::validate_update(body)?;

    // Verify secret token if configured
    if let Some(secret) = secret {
        if !verify::verify_secret_token(headers, secret) {
            anyhow::bail!("Telegram webhook secret verification failed");
        }
    }

    // Deduplicate
    {
        let mut guard = dedup.lock().await;
        if guard.is_duplicate(update.update_id) {
            return Ok(None);
        }
    }

    // Parse into InboundMessage
    Ok(parse::parse_update(&update))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn make_headers(secret: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(s) = secret {
            headers.insert(
                "x-telegram-bot-api-secret-token",
                HeaderValue::from_str(s).unwrap(),
            );
        }
        headers
    }

    fn text_body(update_id: i64, text: &str) -> String {
        format!(
            r#"{{
                "update_id": {update_id},
                "message": {{
                    "message_id": 1,
                    "from": {{ "id": 42, "first_name": "Alice", "is_bot": false }},
                    "chat": {{ "id": 42, "type": "private" }},
                    "date": 1700000000,
                    "text": {text_json}
                }}
            }}"#,
            text_json = serde_json::to_string(text).unwrap()
        )
    }

    #[tokio::test]
    async fn full_flow_no_secret() {
        let dedup = Mutex::new(UpdateDeduplicator::new());
        let headers = HeaderMap::new();
        let body = text_body(1, "hello");

        let result = handle_telegram_webhook(&headers, &body, None, &dedup)
            .await
            .unwrap();
        let msg = result.unwrap();
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.sender_id, "42");
    }

    #[tokio::test]
    async fn full_flow_with_secret() {
        let dedup = Mutex::new(UpdateDeduplicator::new());
        let headers = make_headers(Some("secret123"));
        let body = text_body(2, "hi");

        let result = handle_telegram_webhook(&headers, &body, Some("secret123"), &dedup)
            .await
            .unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn wrong_secret_rejected() {
        let dedup = Mutex::new(UpdateDeduplicator::new());
        let headers = make_headers(Some("wrong"));
        let body = text_body(3, "hi");

        let result = handle_telegram_webhook(&headers, &body, Some("secret123"), &dedup).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn duplicate_update_returns_none() {
        let dedup = Mutex::new(UpdateDeduplicator::new());
        let headers = HeaderMap::new();
        let body = text_body(10, "hi");

        let first = handle_telegram_webhook(&headers, &body, None, &dedup)
            .await
            .unwrap();
        assert!(first.is_some());

        let second = handle_telegram_webhook(&headers, &body, None, &dedup)
            .await
            .unwrap();
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn non_text_update_returns_none() {
        let dedup = Mutex::new(UpdateDeduplicator::new());
        let headers = HeaderMap::new();
        // Photo-only message (no text field)
        let body = r#"{
            "update_id": 20,
            "message": {
                "message_id": 1,
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "chat": { "id": 42, "type": "private" },
                "date": 1700000000
            }
        }"#;

        let result = handle_telegram_webhook(&headers, body, None, &dedup)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn invalid_body_returns_error() {
        let dedup = Mutex::new(UpdateDeduplicator::new());
        let headers = HeaderMap::new();

        let result = handle_telegram_webhook(&headers, "not json", None, &dedup).await;
        assert!(result.is_err());
    }
}
