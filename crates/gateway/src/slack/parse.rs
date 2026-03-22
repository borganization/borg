use super::types::EventCallback;
use crate::handler::InboundMessage;

/// Strip `<@BOT_ID>` mentions from message text.
///
/// Removes all occurrences of `<@bot_user_id>` and trims surrounding whitespace.
pub fn strip_bot_mention(text: &str, bot_user_id: &str) -> String {
    let mention = format!("<@{bot_user_id}>");
    text.replace(&mention, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse a Slack `EventCallback` into an `InboundMessage`.
///
/// Handles:
/// - `message` and `app_mention` events (with file attachment extraction)
/// - `reaction_added` events (emoji reactions on messages)
///
/// Returns `None` for:
/// - Bot messages (to avoid self-reply loops)
/// - Non-text message subtypes (edits, deletes, joins, etc.)
/// - Events without text (for message events)
/// - Unrecognized event types
///
/// If `bot_user_id` is provided, `<@BOT_ID>` mentions are stripped from the text.
pub fn parse_event(callback: &EventCallback, bot_user_id: Option<&str>) -> Option<InboundMessage> {
    let event = &callback.event;

    // Skip bot messages to prevent loops
    if event.bot_id.is_some() {
        return None;
    }

    match event.event_type.as_str() {
        "message" | "app_mention" => parse_message_or_mention(event, bot_user_id),
        "reaction_added" => parse_reaction(event),
        _ => None,
    }
}

/// Parse a message or app_mention event.
fn parse_message_or_mention(
    event: &super::types::SlackEvent,
    bot_user_id: Option<&str>,
) -> Option<InboundMessage> {
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

    // Strip bot mentions from text
    let cleaned_text = match bot_user_id {
        Some(uid) => {
            let stripped = strip_bot_mention(text, uid);
            if stripped.is_empty() {
                return None;
            }
            stripped
        }
        None => text.to_string(),
    };

    // Extract file metadata
    let metadata = extract_file_metadata(&event.files);

    Some(InboundMessage {
        sender_id: sender_id.to_string(),
        text: cleaned_text,
        channel_id: event.channel.clone(),
        thread_id: None,
        message_id: None,
        thread_ts: event.thread_ts.clone(),
        attachments: Vec::new(),
        reaction: None,
        metadata,
    })
}

/// Parse a reaction_added event.
fn parse_reaction(event: &super::types::SlackEvent) -> Option<InboundMessage> {
    let sender_id = event.user.as_deref()?;
    let emoji = event.reaction.as_deref()?;

    let channel_id = event.item.as_ref().and_then(|i| i.channel.clone());

    Some(InboundMessage {
        sender_id: sender_id.to_string(),
        text: format!("reacted with :{emoji}:"),
        channel_id,
        thread_id: None,
        message_id: None,
        thread_ts: event.item.as_ref().and_then(|i| i.ts.clone()),
        attachments: Vec::new(),
        reaction: Some(emoji.to_string()),
        metadata: serde_json::Value::Null,
    })
}

/// Extract file metadata from Slack files into metadata JSON.
///
/// File download URLs are stored in metadata (under `slack_files`) since downloading
/// requires the bot token which is not available in the parse layer. The handler or
/// api layer can use `SlackClient::download_file()` to fetch the actual content.
fn extract_file_metadata(files: &[super::types::SlackFile]) -> serde_json::Value {
    if files.is_empty() {
        return serde_json::Value::Null;
    }

    let mut file_entries = Vec::new();

    for f in files {
        let mut entry = serde_json::Map::new();
        if let Some(ref url) = f.url_private_download {
            entry.insert("url".into(), serde_json::Value::String(url.clone()));
        }
        if let Some(ref name) = f.name {
            entry.insert("name".into(), serde_json::Value::String(name.clone()));
        }
        if let Some(ref mime) = f.mimetype {
            entry.insert("mimetype".into(), serde_json::Value::String(mime.clone()));
        }
        if let Some(size) = f.size {
            entry.insert("size".into(), serde_json::json!(size));
        }
        file_entries.push(serde_json::Value::Object(entry));
    }

    serde_json::json!({ "slack_files": file_entries })
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
            files: Vec::new(),
            reaction: None,
            item: None,
        }
    }

    #[test]
    fn parse_message_event() {
        let cb = make_event(make_slack_event("message"));
        let msg = parse_event(&cb, None).unwrap();
        assert_eq!(msg.sender_id, "U456");
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.channel_id.as_deref(), Some("C789"));
        // Non-threaded messages have no thread_ts — replies go in-channel
        assert!(msg.thread_ts.is_none());
    }

    #[test]
    fn parse_app_mention_event() {
        let cb = make_event(make_slack_event("app_mention"));
        let msg = parse_event(&cb, None).unwrap();
        assert_eq!(msg.sender_id, "U456");
        assert_eq!(msg.text, "hello");
    }

    #[test]
    fn parse_threaded_message() {
        let mut event = make_slack_event("message");
        event.thread_ts = Some("1234567890.000001".to_string());
        let cb = make_event(event);
        let msg = parse_event(&cb, None).unwrap();
        assert_eq!(msg.thread_ts.as_deref(), Some("1234567890.000001"));
    }

    #[test]
    fn bot_message_returns_none() {
        let mut event = make_slack_event("message");
        event.bot_id = Some("B123".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn no_text_returns_none() {
        let mut event = make_slack_event("message");
        event.text = None;
        let cb = make_event(event);
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn empty_text_returns_none() {
        let mut event = make_slack_event("message");
        event.text = Some(String::new());
        let cb = make_event(event);
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn no_user_returns_none() {
        let mut event = make_slack_event("message");
        event.user = None;
        let cb = make_event(event);
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn incomplete_reaction_event_returns_none() {
        // reaction_added without reaction/item fields should be skipped
        let cb = make_event(make_slack_event("reaction_added"));
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn unknown_event_type_returns_none() {
        let cb = make_event(make_slack_event("channel_created"));
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn message_changed_subtype_returns_none() {
        let mut event = make_slack_event("message");
        event.subtype = Some("message_changed".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn bot_message_subtype_returns_none() {
        let mut event = make_slack_event("message");
        event.subtype = Some("bot_message".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn channel_join_subtype_returns_none() {
        let mut event = make_slack_event("message");
        event.subtype = Some("channel_join".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn missing_channel_still_parses() {
        let mut event = make_slack_event("message");
        event.channel = None;
        let cb = make_event(event);
        let msg = parse_event(&cb, None).unwrap();
        assert!(msg.channel_id.is_none());
    }

    // ── Mention stripping tests ───────────────────────────────────────

    #[test]
    fn strip_mention_from_start() {
        assert_eq!(strip_bot_mention("<@U123> help me", "U123"), "help me");
    }

    #[test]
    fn strip_mention_from_middle() {
        assert_eq!(
            strip_bot_mention("hey <@U123> help me", "U123"),
            "hey help me"
        );
    }

    #[test]
    fn strip_multiple_mentions() {
        assert_eq!(
            strip_bot_mention("<@U123> do <@U123> this", "U123"),
            "do this"
        );
    }

    #[test]
    fn strip_non_matching_id_unchanged() {
        assert_eq!(
            strip_bot_mention("<@U999> help me", "U123"),
            "<@U999> help me"
        );
    }

    #[test]
    fn parse_app_mention_strips_mention() {
        let mut event = make_slack_event("app_mention");
        event.text = Some("<@UBOT> help me".to_string());
        let cb = make_event(event);
        let msg = parse_event(&cb, Some("UBOT")).unwrap();
        assert_eq!(msg.text, "help me");
    }

    #[test]
    fn parse_event_without_bot_user_id_leaves_text() {
        let mut event = make_slack_event("app_mention");
        event.text = Some("<@UBOT> help me".to_string());
        let cb = make_event(event);
        let msg = parse_event(&cb, None).unwrap();
        assert_eq!(msg.text, "<@UBOT> help me");
    }

    #[test]
    fn parse_event_mention_only_returns_none() {
        let mut event = make_slack_event("app_mention");
        event.text = Some("<@UBOT>".to_string());
        let cb = make_event(event);
        assert!(parse_event(&cb, Some("UBOT")).is_none());
    }

    // ── File attachment tests ─────────────────────────────────────────

    #[test]
    fn parse_message_with_files_populates_metadata() {
        use super::super::types::SlackFile;

        let mut event = make_slack_event("message");
        event.files = vec![SlackFile {
            id: Some("F123".into()),
            name: Some("doc.pdf".into()),
            mimetype: Some("application/pdf".into()),
            filetype: Some("pdf".into()),
            url_private_download: Some("https://files.slack.com/doc.pdf".into()),
            size: Some(1024),
        }];
        let cb = make_event(event);
        let msg = parse_event(&cb, None).unwrap();

        let files = msg.metadata["slack_files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["name"], "doc.pdf");
        assert_eq!(files[0]["mimetype"], "application/pdf");
        assert_eq!(files[0]["url"], "https://files.slack.com/doc.pdf");
    }

    #[test]
    fn parse_message_no_files_null_metadata() {
        let cb = make_event(make_slack_event("message"));
        let msg = parse_event(&cb, None).unwrap();
        assert!(msg.metadata.is_null());
    }

    #[test]
    fn file_with_missing_optional_fields() {
        use super::super::types::SlackFile;

        let mut event = make_slack_event("message");
        event.files = vec![SlackFile {
            id: None,
            name: None,
            mimetype: None,
            filetype: None,
            url_private_download: None,
            size: None,
        }];
        let cb = make_event(event);
        let msg = parse_event(&cb, None).unwrap();

        let files = msg.metadata["slack_files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
    }

    // ── Reaction event tests ──────────────────────────────────────────

    #[test]
    fn parse_reaction_added_event() {
        use super::super::types::ReactionItem;

        let mut event = make_slack_event("reaction_added");
        event.text = None;
        event.reaction = Some("thumbsup".into());
        event.item = Some(ReactionItem {
            channel: Some("C789".into()),
            ts: Some("1234567890.123456".into()),
        });
        let cb = make_event(event);
        let msg = parse_event(&cb, None).unwrap();

        assert_eq!(msg.sender_id, "U456");
        assert_eq!(msg.text, "reacted with :thumbsup:");
        assert_eq!(msg.reaction.as_deref(), Some("thumbsup"));
        assert_eq!(msg.channel_id.as_deref(), Some("C789"));
        assert_eq!(msg.thread_ts.as_deref(), Some("1234567890.123456"));
    }

    #[test]
    fn reaction_without_user_returns_none() {
        use super::super::types::ReactionItem;

        let mut event = make_slack_event("reaction_added");
        event.user = None;
        event.text = None;
        event.reaction = Some("thumbsup".into());
        event.item = Some(ReactionItem {
            channel: Some("C789".into()),
            ts: None,
        });
        let cb = make_event(event);
        assert!(parse_event(&cb, None).is_none());
    }

    #[test]
    fn reaction_without_emoji_returns_none() {
        let mut event = make_slack_event("reaction_added");
        event.text = None;
        event.reaction = None;
        let cb = make_event(event);
        assert!(parse_event(&cb, None).is_none());
    }
}
