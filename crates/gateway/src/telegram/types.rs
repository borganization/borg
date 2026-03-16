use serde::{Deserialize, Serialize};

/// Telegram Bot API Update object.
#[derive(Debug, Clone, Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<TelegramMessage>,
    pub edited_message: Option<TelegramMessage>,
    pub callback_query: Option<CallbackQuery>,
}

/// A Telegram message.
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramMessage {
    pub message_id: i64,
    pub from: Option<User>,
    pub chat: Chat,
    pub date: i64,
    pub text: Option<String>,
    pub edit_date: Option<i64>,
}

/// A Telegram chat.
#[derive(Debug, Clone, Deserialize)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: String,
}

/// A Telegram user.
#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: i64,
    pub first_name: String,
    pub username: Option<String>,
    #[serde(default)]
    pub is_bot: bool,
}

/// A Telegram callback query (inline button press).
#[derive(Debug, Clone, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    pub message: Option<TelegramMessage>,
    pub data: Option<String>,
}

/// Generic Telegram Bot API response wrapper.
#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub ok: bool,
    pub result: Option<T>,
    pub description: Option<String>,
    pub error_code: Option<u16>,
    #[serde(default)]
    pub retry_after: Option<u64>,
}

/// Request body for sendMessage.
#[derive(Debug, Serialize)]
pub struct SendMessageRequest {
    pub chat_id: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_mode: Option<String>,
}

/// Webhook info returned by getWebhookInfo.
#[derive(Debug, Deserialize)]
pub struct WebhookInfo {
    pub url: String,
    #[serde(default)]
    pub has_custom_certificate: bool,
    #[serde(default)]
    pub pending_update_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_text_message_update() {
        let json = r#"{
            "update_id": 123456,
            "message": {
                "message_id": 1,
                "from": {
                    "id": 42,
                    "first_name": "Alice",
                    "username": "alice",
                    "is_bot": false
                },
                "chat": { "id": 42, "type": "private" },
                "date": 1700000000,
                "text": "Hello bot"
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        assert_eq!(update.update_id, 123456);
        let msg = update.message.unwrap();
        assert_eq!(msg.text.as_deref(), Some("Hello bot"));
        assert_eq!(msg.chat.id, 42);
        assert_eq!(msg.chat.chat_type, "private");
        let from = msg.from.unwrap();
        assert_eq!(from.id, 42);
        assert_eq!(from.username.as_deref(), Some("alice"));
        assert!(!from.is_bot);
    }

    #[test]
    fn deserialize_edited_message_update() {
        let json = r#"{
            "update_id": 123457,
            "edited_message": {
                "message_id": 1,
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "chat": { "id": 42, "type": "private" },
                "date": 1700000000,
                "edit_date": 1700000060,
                "text": "Hello bot (edited)"
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        assert!(update.message.is_none());
        let edited = update.edited_message.unwrap();
        assert_eq!(edited.text.as_deref(), Some("Hello bot (edited)"));
        assert_eq!(edited.edit_date, Some(1700000060));
    }

    #[test]
    fn deserialize_callback_query_update() {
        let json = r#"{
            "update_id": 123458,
            "callback_query": {
                "id": "cb_1",
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "message": {
                    "message_id": 5,
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000
                },
                "data": "button_click"
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        let cb = update.callback_query.unwrap();
        assert_eq!(cb.id, "cb_1");
        assert_eq!(cb.data.as_deref(), Some("button_click"));
        assert_eq!(cb.from.id, 42);
    }

    #[test]
    fn deserialize_api_response_success() {
        let json = r#"{
            "ok": true,
            "result": { "id": 123, "first_name": "TestBot", "is_bot": true }
        }"#;

        let resp: ApiResponse<User> = serde_json::from_str(json).unwrap();
        assert!(resp.ok);
        let user = resp.result.unwrap();
        assert_eq!(user.id, 123);
        assert!(user.is_bot);
    }

    #[test]
    fn deserialize_api_response_error() {
        let json = r#"{
            "ok": false,
            "error_code": 429,
            "description": "Too Many Requests",
            "retry_after": 30
        }"#;

        let resp: ApiResponse<()> = serde_json::from_str(json).unwrap();
        assert!(!resp.ok);
        assert_eq!(resp.error_code, Some(429));
        assert_eq!(resp.retry_after, Some(30));
    }

    #[test]
    fn deserialize_webhook_info() {
        let json = r#"{
            "url": "https://example.com/webhook/telegram",
            "has_custom_certificate": false,
            "pending_update_count": 3
        }"#;

        let info: WebhookInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.url, "https://example.com/webhook/telegram");
        assert_eq!(info.pending_update_count, 3);
    }

    #[test]
    fn deserialize_minimal_update() {
        let json = r#"{ "update_id": 999 }"#;
        let update: Update = serde_json::from_str(json).unwrap();
        assert_eq!(update.update_id, 999);
        assert!(update.message.is_none());
        assert!(update.edited_message.is_none());
        assert!(update.callback_query.is_none());
    }
}
