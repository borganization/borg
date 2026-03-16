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
    pub media_group_id: Option<String>,
    pub message_thread_id: Option<i64>,
    pub photo: Option<Vec<PhotoSize>>,
    pub caption: Option<String>,
    pub document: Option<Document>,
    pub video: Option<Video>,
    pub audio: Option<Audio>,
    pub voice: Option<Voice>,
    pub sticker: Option<Sticker>,
    pub reply_to_message: Option<Box<TelegramMessage>>,
    pub forward_date: Option<i64>,
    pub forum_topic_created: Option<serde_json::Value>,
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

/// A Telegram photo size variant.
#[derive(Debug, Clone, Deserialize)]
pub struct PhotoSize {
    pub file_id: String,
    pub file_unique_id: String,
    pub width: i32,
    pub height: i32,
    pub file_size: Option<i64>,
}

/// A Telegram document (file attachment).
#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    pub file_id: String,
    pub file_unique_id: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
}

/// A Telegram video.
#[derive(Debug, Clone, Deserialize)]
pub struct Video {
    pub file_id: String,
    pub file_unique_id: String,
    pub width: i32,
    pub height: i32,
    pub duration: i32,
    pub file_size: Option<i64>,
}

/// A Telegram audio file.
#[derive(Debug, Clone, Deserialize)]
pub struct Audio {
    pub file_id: String,
    pub file_unique_id: String,
    pub duration: i32,
    pub title: Option<String>,
    pub file_size: Option<i64>,
}

/// A Telegram voice message.
#[derive(Debug, Clone, Deserialize)]
pub struct Voice {
    pub file_id: String,
    pub file_unique_id: String,
    pub duration: i32,
    pub file_size: Option<i64>,
}

/// A Telegram sticker.
#[derive(Debug, Clone, Deserialize)]
pub struct Sticker {
    pub file_id: String,
    pub file_unique_id: String,
    pub width: i32,
    pub height: i32,
    pub emoji: Option<String>,
    pub file_size: Option<i64>,
}

/// Telegram file info returned by getFile.
#[derive(Debug, Clone, Deserialize)]
pub struct FileInfo {
    pub file_id: String,
    pub file_unique_id: String,
    pub file_size: Option<i64>,
    pub file_path: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_thread_id: Option<i64>,
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

    #[test]
    fn deserialize_photo_message() {
        let json = r#"{
            "update_id": 100,
            "message": {
                "message_id": 1,
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "chat": { "id": 42, "type": "private" },
                "date": 1700000000,
                "photo": [
                    { "file_id": "small", "file_unique_id": "u1", "width": 90, "height": 90, "file_size": 1024 },
                    { "file_id": "large", "file_unique_id": "u2", "width": 800, "height": 600, "file_size": 50000 }
                ],
                "caption": "My photo"
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        let msg = update.message.unwrap();
        let photos = msg.photo.unwrap();
        assert_eq!(photos.len(), 2);
        assert_eq!(photos[0].file_id, "small");
        assert_eq!(photos[1].width, 800);
        assert_eq!(msg.caption.as_deref(), Some("My photo"));
    }

    #[test]
    fn deserialize_document_message() {
        let json = r#"{
            "update_id": 101,
            "message": {
                "message_id": 2,
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "chat": { "id": 42, "type": "private" },
                "date": 1700000000,
                "document": {
                    "file_id": "doc1",
                    "file_unique_id": "ud1",
                    "file_name": "report.pdf",
                    "mime_type": "application/pdf",
                    "file_size": 123456
                }
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        let doc = update.message.unwrap().document.unwrap();
        assert_eq!(doc.file_name.as_deref(), Some("report.pdf"));
        assert_eq!(doc.mime_type.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn deserialize_forum_message() {
        let json = r#"{
            "update_id": 102,
            "message": {
                "message_id": 50,
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "chat": { "id": -100123, "type": "supergroup" },
                "date": 1700000000,
                "message_thread_id": 99,
                "text": "Forum topic message"
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        let msg = update.message.unwrap();
        assert_eq!(msg.message_thread_id, Some(99));
        assert_eq!(msg.text.as_deref(), Some("Forum topic message"));
    }

    #[test]
    fn deserialize_voice_message() {
        let json = r#"{
            "update_id": 103,
            "message": {
                "message_id": 3,
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "chat": { "id": 42, "type": "private" },
                "date": 1700000000,
                "voice": { "file_id": "v1", "file_unique_id": "uv1", "duration": 5, "file_size": 8000 }
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        let voice = update.message.unwrap().voice.unwrap();
        assert_eq!(voice.duration, 5);
    }

    #[test]
    fn deserialize_sticker_message() {
        let json = r#"{
            "update_id": 104,
            "message": {
                "message_id": 4,
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "chat": { "id": 42, "type": "private" },
                "date": 1700000000,
                "sticker": { "file_id": "s1", "file_unique_id": "us1", "width": 512, "height": 512, "emoji": "\ud83d\ude00" }
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        let sticker = update.message.unwrap().sticker.unwrap();
        assert_eq!(sticker.width, 512);
        assert!(sticker.emoji.is_some());
    }

    #[test]
    fn deserialize_reply_to_message() {
        let json = r#"{
            "update_id": 105,
            "message": {
                "message_id": 10,
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "chat": { "id": 42, "type": "private" },
                "date": 1700000000,
                "text": "Reply text",
                "reply_to_message": {
                    "message_id": 5,
                    "chat": { "id": 42, "type": "private" },
                    "date": 1699999000,
                    "text": "Original"
                }
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        let msg = update.message.unwrap();
        let reply = msg.reply_to_message.unwrap();
        assert_eq!(reply.message_id, 5);
        assert_eq!(reply.text.as_deref(), Some("Original"));
    }

    #[test]
    fn deserialize_forwarded_message() {
        let json = r#"{
            "update_id": 106,
            "message": {
                "message_id": 11,
                "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                "chat": { "id": 42, "type": "private" },
                "date": 1700000000,
                "forward_date": 1699000000,
                "text": "Forwarded text"
            }
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        let msg = update.message.unwrap();
        assert_eq!(msg.forward_date, Some(1699000000));
    }

    #[test]
    fn deserialize_send_message_with_thread() {
        let req = SendMessageRequest {
            chat_id: 42,
            text: "hello".into(),
            parse_mode: Some("HTML".into()),
            reply_to_message_id: Some(10),
            message_thread_id: Some(99),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["message_thread_id"], 99);
        assert_eq!(json["reply_to_message_id"], 10);
    }

    #[test]
    fn deserialize_file_info() {
        let json = r#"{
            "file_id": "abc123",
            "file_unique_id": "unique1",
            "file_size": 12345,
            "file_path": "photos/file_1.jpg"
        }"#;

        let info: FileInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.file_id, "abc123");
        assert_eq!(info.file_path.as_deref(), Some("photos/file_1.jpg"));
    }
}
