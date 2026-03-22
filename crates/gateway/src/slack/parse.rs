use tokio::sync::Mutex;

use super::echo::EchoCache;
use super::types::EventCallback;
use crate::handler::{InboundAttachment, InboundMessage};

/// Parse a Slack `EventCallback` into an `InboundMessage`.
///
/// Returns `None` for:
/// - Bot messages (to avoid self-reply loops)
/// - Messages from the bot's own user ID (echo detection)
/// - Messages matching recently sent text (echo cache)
/// - Non-text message subtypes (edits, deletes, joins, etc.)
/// - Events without text
/// - Unrecognized event types (not `message` or `app_mention`)
pub async fn parse_event(
    callback: &EventCallback,
    bot_user_id: Option<&str>,
    echo_cache: Option<&Mutex<EchoCache>>,
) -> Option<InboundMessage> {
    let event = &callback.event;

    // Skip bot messages to prevent loops
    if event.bot_id.is_some() {
        return None;
    }

    // Skip messages from the bot's own user ID (defense-in-depth)
    if let (Some(sender), Some(bot_id)) = (event.user.as_deref(), bot_user_id) {
        if sender == bot_id {
            return None;
        }
    }

    // Only handle message and app_mention events
    if event.event_type != "message" && event.event_type != "app_mention" {
        return None;
    }

    // Skip non-standard message subtypes
    if let Some(ref subtype) = event.subtype {
        match subtype.as_str() {
            "bot_message" | "message_changed" | "message_deleted" | "channel_join"
            | "channel_leave" | "file_share" => return None,
            _ => {}
        }
    }

    let text = event.text.as_deref()?;
    if text.is_empty() {
        return None;
    }

    // Check echo cache — skip if this text matches a recently sent outbound message
    if let Some(cache) = echo_cache {
        if cache.lock().await.is_echo(text) {
            return None;
        }
    }

    let sender_id = event.user.as_deref()?;

    // Extract file metadata as placeholder attachments (URLs only, downloaded later)
    let attachments = event
        .files
        .as_ref()
        .map(|files| {
            files
                .iter()
                .map(|f| InboundAttachment {
                    mime_type: f
                        .mimetype
                        .clone()
                        .unwrap_or_else(|| "application/octet-stream".to_string()),
                    data: f.url_private.clone().unwrap_or_default(),
                    filename: f.name.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let peer_kind = match event.channel_type.as_deref() {
        Some("im") => Some("direct".to_string()),
        Some("channel") | Some("group") | Some("mpim") => Some("group".to_string()),
        _ => None,
    };

    Some(InboundMessage {
        sender_id: sender_id.to_string(),
        text: text.to_string(),
        channel_id: event.channel.clone(),
        thread_id: None,
        message_id: event.ts.clone(),
        thread_ts: event.thread_ts.clone(),
        attachments,
        reaction: None,
        metadata: serde_json::json!({
            "event_type": event.event_type,
        }),
        peer_kind,
    })
}

#[cfg(test)]
mod tests {
    use super::super::types::SlackEvent;
    use super::*;
    use std::sync::Arc;

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
            files: None,
        }
    }

    #[tokio::test]
    async fn parse_message_event() {
        let cb = make_event(make_slack_event("message"));
        let msg = parse_event(&cb, None, None).await.unwrap();
        assert_eq!(msg.sender_id, "U456");
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.channel_id.as_deref(), Some("C789"));
        assert_eq!(msg.message_id.as_deref(), Some("1234567890.123456"));
        assert!(msg.thread_ts.is_none());
    }

    #[tokio::test]
    async fn parse_app_mention_event() {
        let cb = make_event(make_slack_event("app_mention"));
        let msg = parse_event(&cb, None, None).await.unwrap();
        assert_eq!(msg.sender_id, "U456");
        assert_eq!(msg.text, "hello");
    }

    #[tokio::test]
    async fn parse_threaded_message() {
        let mut event = make_slack_event("message");
        event.thread_ts = Some("1234567890.000001".to_string());
        let cb = make_event(event);
        let msg = parse_event(&cb, None, None).await.unwrap();
        assert_eq!(msg.thread_ts.as_deref(), Some("1234567890.000001"));
    }

    #[tokio::test]
    async fn bot_message_returns_none() {
        let mut event = make_slack_event("message");
        event.bot_id = Some("B123".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb, None, None).await.is_none());
    }

    #[tokio::test]
    async fn bot_user_id_returns_none() {
        let cb = make_event(make_slack_event("message"));
        // sender U456 matches bot_user_id
        assert!(parse_event(&cb, Some("U456"), None).await.is_none());
    }

    #[tokio::test]
    async fn different_bot_user_id_parses() {
        let cb = make_event(make_slack_event("message"));
        let msg = parse_event(&cb, Some("U999"), None).await.unwrap();
        assert_eq!(msg.sender_id, "U456");
    }

    #[tokio::test]
    async fn echo_cache_filters_echo() {
        let cache = Arc::new(Mutex::new(EchoCache::new()));
        cache.lock().await.remember("hello");

        let cb = make_event(make_slack_event("message"));
        assert!(parse_event(&cb, None, Some(&cache)).await.is_none());
    }

    #[tokio::test]
    async fn echo_cache_allows_non_echo() {
        let cache = Arc::new(Mutex::new(EchoCache::new()));
        cache.lock().await.remember("something else");

        let cb = make_event(make_slack_event("message"));
        let msg = parse_event(&cb, None, Some(&cache)).await.unwrap();
        assert_eq!(msg.text, "hello");
    }

    #[tokio::test]
    async fn no_text_returns_none() {
        let mut event = make_slack_event("message");
        event.text = None;
        let cb = make_event(event);
        assert!(parse_event(&cb, None, None).await.is_none());
    }

    #[tokio::test]
    async fn empty_text_returns_none() {
        let mut event = make_slack_event("message");
        event.text = Some(String::new());
        let cb = make_event(event);
        assert!(parse_event(&cb, None, None).await.is_none());
    }

    #[tokio::test]
    async fn no_user_returns_none() {
        let mut event = make_slack_event("message");
        event.user = None;
        let cb = make_event(event);
        assert!(parse_event(&cb, None, None).await.is_none());
    }

    #[tokio::test]
    async fn unknown_event_type_returns_none() {
        let cb = make_event(make_slack_event("reaction_added"));
        assert!(parse_event(&cb, None, None).await.is_none());
    }

    #[tokio::test]
    async fn message_changed_subtype_returns_none() {
        let mut event = make_slack_event("message");
        event.subtype = Some("message_changed".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb, None, None).await.is_none());
    }

    #[tokio::test]
    async fn bot_message_subtype_returns_none() {
        let mut event = make_slack_event("message");
        event.subtype = Some("bot_message".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb, None, None).await.is_none());
    }

    #[tokio::test]
    async fn channel_join_subtype_returns_none() {
        let mut event = make_slack_event("message");
        event.subtype = Some("channel_join".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb, None, None).await.is_none());
    }

    #[tokio::test]
    async fn channel_type_channel_is_group_peer_kind() {
        let cb = make_event(make_slack_event("message"));
        let msg = parse_event(&cb, None, None).await.unwrap();
        assert_eq!(msg.peer_kind.as_deref(), Some("group"));
    }

    #[tokio::test]
    async fn channel_type_im_is_direct_peer_kind() {
        let mut event = make_slack_event("message");
        event.channel_type = Some("im".to_string());
        let cb = make_event(event);
        let msg = parse_event(&cb, None, None).await.unwrap();
        assert_eq!(msg.peer_kind.as_deref(), Some("direct"));
    }

    #[tokio::test]
    async fn app_mention_stores_event_type_metadata() {
        let cb = make_event(make_slack_event("app_mention"));
        let msg = parse_event(&cb, None, None).await.unwrap();
        assert_eq!(msg.metadata["event_type"], "app_mention");
    }

    #[tokio::test]
    async fn missing_channel_still_parses() {
        let mut event = make_slack_event("message");
        event.channel = None;
        let cb = make_event(event);
        let msg = parse_event(&cb, None, None).await.unwrap();
        assert!(msg.channel_id.is_none());
    }

    #[tokio::test]
    async fn parse_event_with_files() {
        use super::super::types::SlackFile;

        let mut event = make_slack_event("message");
        event.files = Some(vec![SlackFile {
            id: "F123".to_string(),
            name: Some("test.pdf".to_string()),
            mimetype: Some("application/pdf".to_string()),
            url_private: Some("https://files.slack.com/files-pri/T123-F123/test.pdf".to_string()),
            size: Some(1024),
            filetype: Some("pdf".to_string()),
        }]);
        let cb = make_event(event);
        let msg = parse_event(&cb, None, None).await.unwrap();
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].mime_type, "application/pdf");
        assert_eq!(msg.attachments[0].filename.as_deref(), Some("test.pdf"));
    }
}
