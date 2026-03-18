use super::types::Update;
use crate::handler::InboundMessage;

/// Extract an `InboundMessage` from a Telegram update.
///
/// Handles text messages, media messages (photo, document, video, audio, voice, sticker),
/// edited messages, callback queries, and forum topic routing.
/// Returns `None` for service messages (forum_topic_created, etc.) and empty updates.
pub fn parse_update(update: &Update) -> Option<InboundMessage> {
    // Try regular message first, then edited message
    if let Some(msg) = update.message.as_ref().or(update.edited_message.as_ref()) {
        // Skip service messages
        if msg.forum_topic_created.is_some() {
            return None;
        }

        // Try text first, then caption, then generate placeholder for media
        let text = if let Some(ref t) = msg.text {
            if t.is_empty() {
                return None;
            }
            t.clone()
        } else if let Some(ref caption) = msg.caption {
            caption.clone()
        } else if msg.photo.is_some() {
            "[Photo]".to_string()
        } else if let Some(ref doc) = msg.document {
            match &doc.file_name {
                Some(name) => format!("[Document: {name}]"),
                None => "[Document]".to_string(),
            }
        } else if msg.video.is_some() {
            "[Video]".to_string()
        } else if msg.audio.is_some() {
            "[Audio]".to_string()
        } else if msg.voice.is_some() {
            "[Voice message]".to_string()
        } else if let Some(ref sticker) = msg.sticker {
            match &sticker.emoji {
                Some(emoji) => format!("[Sticker: {emoji}]"),
                None => "[Sticker]".to_string(),
            }
        } else {
            return None;
        };

        let sender_id = msg
            .from
            .as_ref()
            .map(|u| u.id.to_string())
            .unwrap_or_else(|| msg.chat.id.to_string());

        let thread_id = msg.message_thread_id.map(|t| t.to_string());
        let message_id = Some(msg.message_id.to_string());

        return Some(InboundMessage {
            sender_id,
            text,
            channel_id: Some(msg.chat.id.to_string()),
            thread_id,
            message_id,
            thread_ts: None,
            attachments: Vec::new(),
        });
    }

    // Try callback query
    if let Some(cb) = &update.callback_query {
        let data = cb.data.as_deref()?;
        if data.is_empty() {
            return None;
        }

        let chat_id = cb.message.as_ref().map(|m| m.chat.id.to_string());
        let thread_id = cb
            .message
            .as_ref()
            .and_then(|m| m.message_thread_id)
            .map(|t| t.to_string());
        let message_id = cb.message.as_ref().map(|m| m.message_id.to_string());

        return Some(InboundMessage {
            sender_id: cb.from.id.to_string(),
            text: data.to_string(),
            channel_id: chat_id,
            thread_id,
            message_id,
            thread_ts: None,
            attachments: Vec::new(),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_update(text: &str) -> Update {
        serde_json::from_str(&format!(
            r#"{{
                "update_id": 1,
                "message": {{
                    "message_id": 1,
                    "from": {{ "id": 42, "first_name": "Alice", "is_bot": false }},
                    "chat": {{ "id": 42, "type": "private" }},
                    "date": 1700000000,
                    "text": {text_json}
                }}
            }}"#,
            text_json = serde_json::to_string(text).unwrap()
        ))
        .unwrap()
    }

    #[test]
    fn parse_text_message() {
        let update = text_update("Hello bot");
        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.sender_id, "42");
        assert_eq!(msg.text, "Hello bot");
        assert_eq!(msg.channel_id.as_deref(), Some("42"));
        assert_eq!(msg.message_id.as_deref(), Some("1"));
    }

    #[test]
    fn parse_edited_message() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 2,
                "edited_message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000,
                    "edit_date": 1700000060,
                    "text": "edited text"
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.text, "edited text");
    }

    #[test]
    fn parse_callback_query() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 3,
                "callback_query": {
                    "id": "cb1",
                    "from": { "id": 99, "first_name": "Bob", "is_bot": false },
                    "message": {
                        "message_id": 5,
                        "chat": { "id": 42, "type": "private" },
                        "date": 1700000000
                    },
                    "data": "btn_click"
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.sender_id, "99");
        assert_eq!(msg.text, "btn_click");
        assert_eq!(msg.channel_id.as_deref(), Some("42"));
    }

    #[test]
    fn photo_with_caption() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 4,
                "message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000,
                    "photo": [{ "file_id": "abc", "file_unique_id": "u1", "width": 100, "height": 100 }],
                    "caption": "Look at this!"
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.text, "Look at this!");
    }

    #[test]
    fn photo_without_caption_generates_placeholder() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 5,
                "message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000,
                    "photo": [{ "file_id": "abc", "file_unique_id": "u1", "width": 100, "height": 100 }]
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.text, "[Photo]");
    }

    #[test]
    fn document_with_filename() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 6,
                "message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000,
                    "document": { "file_id": "abc", "file_unique_id": "u1", "file_name": "report.pdf" }
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.text, "[Document: report.pdf]");
    }

    #[test]
    fn voice_message_placeholder() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 7,
                "message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000,
                    "voice": { "file_id": "abc", "file_unique_id": "u1", "duration": 5 }
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.text, "[Voice message]");
    }

    #[test]
    fn sticker_with_emoji() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 8,
                "message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000,
                    "sticker": { "file_id": "abc", "file_unique_id": "u1", "width": 512, "height": 512, "emoji": "\ud83d\ude00" }
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert!(msg.text.starts_with("[Sticker:"));
    }

    #[test]
    fn forum_topic_created_skipped() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 9,
                "message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": -100123, "type": "supergroup" },
                    "date": 1700000000,
                    "forum_topic_created": { "name": "General", "icon_color": 0 }
                }
            }"#,
        )
        .unwrap();

        assert!(parse_update(&update).is_none());
    }

    #[test]
    fn message_thread_id_extracted() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 10,
                "message": {
                    "message_id": 50,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": -100123, "type": "supergroup" },
                    "date": 1700000000,
                    "message_thread_id": 99,
                    "text": "forum message"
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.thread_id.as_deref(), Some("99"));
        assert_eq!(msg.text, "forum message");
    }

    #[test]
    fn empty_text_returns_none() {
        let update = text_update("");
        assert!(parse_update(&update).is_none());
    }

    #[test]
    fn minimal_update_no_message_returns_none() {
        let update: Update = serde_json::from_str(r#"{ "update_id": 5 }"#).unwrap();
        assert!(parse_update(&update).is_none());
    }

    #[test]
    fn callback_without_data_returns_none() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 6,
                "callback_query": {
                    "id": "cb1",
                    "from": { "id": 99, "first_name": "Bob", "is_bot": false }
                }
            }"#,
        )
        .unwrap();

        assert!(parse_update(&update).is_none());
    }

    #[test]
    fn callback_with_data_but_no_message() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 8,
                "callback_query": {
                    "id": "cb2",
                    "from": { "id": 99, "first_name": "Bob", "is_bot": false },
                    "data": "inline_click"
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.sender_id, "99");
        assert_eq!(msg.text, "inline_click");
        assert!(msg.channel_id.is_none());
    }

    #[test]
    fn group_message_uses_sender_not_chat() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 7,
                "message": {
                    "message_id": 10,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": -100123, "type": "supergroup" },
                    "date": 1700000000,
                    "text": "group msg"
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.sender_id, "42");
        assert_eq!(msg.channel_id.as_deref(), Some("-100123"));
    }

    #[test]
    fn video_placeholder() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 11,
                "message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000,
                    "video": { "file_id": "abc", "file_unique_id": "u1", "width": 1920, "height": 1080, "duration": 30 }
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.text, "[Video]");
    }

    #[test]
    fn audio_placeholder() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 12,
                "message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000,
                    "audio": { "file_id": "abc", "file_unique_id": "u1", "duration": 180 }
                }
            }"#,
        )
        .unwrap();

        let msg = parse_update(&update).unwrap();
        assert_eq!(msg.text, "[Audio]");
    }
}
