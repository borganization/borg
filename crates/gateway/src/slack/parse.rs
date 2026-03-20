use super::types::EventCallback;
use crate::handler::InboundMessage;

/// Parse a Slack `EventCallback` into an `InboundMessage`.
///
/// Returns `None` for:
/// - Bot messages (to avoid self-reply loops)
/// - Non-text message subtypes (edits, deletes, joins, etc.)
/// - Events without text
/// - Unrecognized event types (not `message` or `app_mention`)
pub fn parse_event(callback: &EventCallback) -> Option<InboundMessage> {
    let event = &callback.event;

    // Skip bot messages to prevent loops
    if event.bot_id.is_some() {
        return None;
    }

    // Only handle message and app_mention events
    if event.event_type != "message" && event.event_type != "app_mention" {
        return None;
    }

    // Skip non-standard message subtypes
    if let Some(ref subtype) = event.subtype {
        match subtype.as_str() {
            "bot_message" | "message_changed" | "message_deleted" | "channel_join"
            | "channel_leave" => return None,
            _ => {}
        }
    }

    let text = event.text.as_deref()?;
    if text.is_empty() {
        return None;
    }

    let sender_id = event.user.as_deref()?;

    Some(InboundMessage {
        sender_id: sender_id.to_string(),
        text: text.to_string(),
        channel_id: event.channel.clone(),
        thread_id: None,
        message_id: None,
        thread_ts: event.thread_ts.clone(),
        attachments: Vec::new(),
        reaction: None,
        metadata: serde_json::Value::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::super::types::SlackEvent;
    use super::*;

    fn make_event(event: SlackEvent) -> EventCallback {
        EventCallback {
            token: Some("tok".into()),
            team_id: Some("T123".into()),
            event_id: Some("Ev123".into()),
            event,
        }
    }

    fn make_slack_event(event_type: &str) -> SlackEvent {
        SlackEvent {
            event_type: event_type.to_string(),
            subtype: None,
            user: Some("U456".to_string()),
            text: Some("hello".to_string()),
            ts: Some("1234567890.123456".to_string()),
            thread_ts: None,
            channel: Some("C789".to_string()),
            channel_type: Some("channel".to_string()),
            bot_id: None,
        }
    }

    #[test]
    fn parse_message_event() {
        let cb = make_event(make_slack_event("message"));
        let msg = parse_event(&cb).unwrap();
        assert_eq!(msg.sender_id, "U456");
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.channel_id.as_deref(), Some("C789"));
        // Non-threaded messages have no thread_ts — replies go in-channel
        assert!(msg.thread_ts.is_none());
    }

    #[test]
    fn parse_app_mention_event() {
        let cb = make_event(make_slack_event("app_mention"));
        let msg = parse_event(&cb).unwrap();
        assert_eq!(msg.sender_id, "U456");
        assert_eq!(msg.text, "hello");
    }

    #[test]
    fn parse_threaded_message() {
        let mut event = make_slack_event("message");
        event.thread_ts = Some("1234567890.000001".to_string());
        let cb = make_event(event);
        let msg = parse_event(&cb).unwrap();
        assert_eq!(msg.thread_ts.as_deref(), Some("1234567890.000001"));
    }

    #[test]
    fn bot_message_returns_none() {
        let mut event = make_slack_event("message");
        event.bot_id = Some("B123".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb).is_none());
    }

    #[test]
    fn no_text_returns_none() {
        let mut event = make_slack_event("message");
        event.text = None;
        let cb = make_event(event);
        assert!(parse_event(&cb).is_none());
    }

    #[test]
    fn empty_text_returns_none() {
        let mut event = make_slack_event("message");
        event.text = Some(String::new());
        let cb = make_event(event);
        assert!(parse_event(&cb).is_none());
    }

    #[test]
    fn no_user_returns_none() {
        let mut event = make_slack_event("message");
        event.user = None;
        let cb = make_event(event);
        assert!(parse_event(&cb).is_none());
    }

    #[test]
    fn unknown_event_type_returns_none() {
        let cb = make_event(make_slack_event("reaction_added"));
        assert!(parse_event(&cb).is_none());
    }

    #[test]
    fn message_changed_subtype_returns_none() {
        let mut event = make_slack_event("message");
        event.subtype = Some("message_changed".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb).is_none());
    }

    #[test]
    fn bot_message_subtype_returns_none() {
        let mut event = make_slack_event("message");
        event.subtype = Some("bot_message".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb).is_none());
    }

    #[test]
    fn channel_join_subtype_returns_none() {
        let mut event = make_slack_event("message");
        event.subtype = Some("channel_join".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb).is_none());
    }

    #[test]
    fn missing_channel_still_parses() {
        let mut event = make_slack_event("message");
        event.channel = None;
        let cb = make_event(event);
        let msg = parse_event(&cb).unwrap();
        assert!(msg.channel_id.is_none());
    }
}
