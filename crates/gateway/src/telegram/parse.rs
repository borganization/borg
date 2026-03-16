use super::types::Update;
use crate::handler::InboundMessage;

/// Extract an `InboundMessage` from a Telegram update.
///
/// Returns `None` for non-text updates (e.g. photo-only, stickers).
pub fn parse_update(update: &Update) -> Option<InboundMessage> {
    // Try regular message first, then edited message
    if let Some(msg) = update.message.as_ref().or(update.edited_message.as_ref()) {
        let text = msg.text.as_deref()?.to_string();
        if text.is_empty() {
            return None;
        }

        let sender_id = msg
            .from
            .as_ref()
            .map(|u| u.id.to_string())
            .unwrap_or_else(|| msg.chat.id.to_string());

        return Some(InboundMessage {
            sender_id,
            text,
            channel_id: Some(msg.chat.id.to_string()),
        });
    }

    // Try callback query
    if let Some(cb) = &update.callback_query {
        let data = cb.data.as_deref()?;
        if data.is_empty() {
            return None;
        }

        let chat_id = cb.message.as_ref().map(|m| m.chat.id.to_string());

        return Some(InboundMessage {
            sender_id: cb.from.id.to_string(),
            text: data.to_string(),
            channel_id: chat_id,
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
    fn photo_only_returns_none() {
        let update: Update = serde_json::from_str(
            r#"{
                "update_id": 4,
                "message": {
                    "message_id": 1,
                    "from": { "id": 42, "first_name": "Alice", "is_bot": false },
                    "chat": { "id": 42, "type": "private" },
                    "date": 1700000000
                }
            }"#,
        )
        .unwrap();

        assert!(parse_update(&update).is_none());
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
}
