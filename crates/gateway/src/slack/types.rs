use serde::{Deserialize, Serialize};

/// Top-level Slack Event API envelope.
/// Slack sends either a `url_verification` challenge or an `event_callback`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum SlackEnvelope {
    #[serde(rename = "url_verification")]
    UrlVerification { challenge: String },
    #[serde(rename = "event_callback")]
    EventCallback(Box<EventCallback>),
}

/// An `event_callback` envelope containing a Slack event.
#[derive(Debug, Clone, Deserialize)]
pub struct EventCallback {
    pub token: Option<String>,
    pub team_id: Option<String>,
    pub event_id: Option<String>,
    pub event: SlackEvent,
}

/// Inner Slack event (message, app_mention, reaction_added, etc.).
#[derive(Debug, Clone, Deserialize)]
pub struct SlackEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub subtype: Option<String>,
    pub user: Option<String>,
    pub text: Option<String>,
    pub ts: Option<String>,
    pub thread_ts: Option<String>,
    pub channel: Option<String>,
    pub channel_type: Option<String>,
    pub bot_id: Option<String>,
    /// Files attached to the message (images, documents, etc.).
    #[serde(default)]
    pub files: Vec<SlackFile>,
    /// Reaction emoji name (for reaction_added/reaction_removed events).
    pub reaction: Option<String>,
    /// The item that was reacted to (for reaction events).
    pub item: Option<ReactionItem>,
}

/// A file attached to a Slack message.
#[derive(Debug, Clone, Deserialize)]
pub struct SlackFile {
    pub id: Option<String>,
    pub name: Option<String>,
    pub mimetype: Option<String>,
    pub filetype: Option<String>,
    pub url_private_download: Option<String>,
    pub size: Option<u64>,
}

/// The item a reaction was applied to.
#[derive(Debug, Clone, Deserialize)]
pub struct ReactionItem {
    pub channel: Option<String>,
    pub ts: Option<String>,
}

/// Response from Slack `auth.test` API.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthTestResponse {
    pub ok: bool,
    pub user_id: Option<String>,
    pub user: Option<String>,
    pub team: Option<String>,
    pub team_id: Option<String>,
    pub error: Option<String>,
}

/// Request body for `chat.postMessage`.
#[derive(Debug, Serialize)]
pub struct PostMessageRequest {
    pub channel: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<serde_json::Value>>,
}

/// Response from `chat.postMessage` API — captures the message timestamp.
#[derive(Debug, Deserialize)]
pub struct PostMessageResponse {
    pub ok: bool,
    pub ts: Option<String>,
    pub channel: Option<String>,
    pub error: Option<String>,
}

/// Request body for `chat.update` (edit a previously sent message).
#[derive(Debug, Serialize)]
pub struct UpdateMessageRequest {
    pub channel: String,
    pub ts: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<serde_json::Value>>,
}

/// Slack slash command payload (application/x-www-form-urlencoded).
#[derive(Debug, Clone, Deserialize)]
pub struct SlashCommandPayload {
    pub command: String,
    pub text: Option<String>,
    pub user_id: String,
    pub user_name: Option<String>,
    pub channel_id: String,
    pub channel_name: Option<String>,
    pub team_id: Option<String>,
    pub response_url: Option<String>,
    pub trigger_id: Option<String>,
}

/// Generic Slack Web API response.
#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub ok: bool,
    #[serde(flatten)]
    pub data: Option<T>,
    pub error: Option<String>,
    #[serde(default)]
    pub retry_after: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_url_verification() {
        let json = r#"{
            "type": "url_verification",
            "challenge": "abc123",
            "token": "xyz"
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::UrlVerification { challenge } => {
                assert_eq!(challenge, "abc123");
            }
            _ => panic!("expected UrlVerification"),
        }
    }

    #[test]
    fn deserialize_event_callback_message() {
        let json = r#"{
            "type": "event_callback",
            "token": "tok",
            "team_id": "T123",
            "event_id": "Ev123",
            "event": {
                "type": "message",
                "user": "U456",
                "text": "hello",
                "ts": "1234567890.123456",
                "channel": "C789",
                "channel_type": "channel"
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                assert_eq!(cb.team_id.as_deref(), Some("T123"));
                assert_eq!(cb.event.event_type, "message");
                assert_eq!(cb.event.user.as_deref(), Some("U456"));
                assert_eq!(cb.event.text.as_deref(), Some("hello"));
                assert_eq!(cb.event.channel.as_deref(), Some("C789"));
                assert!(cb.event.bot_id.is_none());
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn deserialize_event_callback_app_mention() {
        let json = r#"{
            "type": "event_callback",
            "token": "tok",
            "team_id": "T123",
            "event_id": "Ev456",
            "event": {
                "type": "app_mention",
                "user": "U456",
                "text": "<@U789> help",
                "ts": "1234567890.654321",
                "channel": "C789",
                "channel_type": "channel"
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                assert_eq!(cb.event.event_type, "app_mention");
                assert_eq!(cb.event.text.as_deref(), Some("<@U789> help"));
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn deserialize_bot_message() {
        let json = r#"{
            "type": "event_callback",
            "token": "tok",
            "team_id": "T123",
            "event_id": "Ev789",
            "event": {
                "type": "message",
                "text": "bot says hi",
                "ts": "1234567890.111111",
                "channel": "C789",
                "bot_id": "B123"
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                assert!(cb.event.bot_id.is_some());
                assert!(cb.event.user.is_none());
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn deserialize_threaded_message() {
        let json = r#"{
            "type": "event_callback",
            "token": "tok",
            "team_id": "T123",
            "event_id": "Ev101",
            "event": {
                "type": "message",
                "user": "U456",
                "text": "thread reply",
                "ts": "1234567890.222222",
                "thread_ts": "1234567890.111111",
                "channel": "C789"
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                assert_eq!(cb.event.thread_ts.as_deref(), Some("1234567890.111111"));
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn deserialize_auth_test_response() {
        let json = r#"{
            "ok": true,
            "user_id": "U123",
            "user": "bot",
            "team": "My Team",
            "team_id": "T123"
        }"#;

        let resp: AuthTestResponse = serde_json::from_str(json).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.user_id.as_deref(), Some("U123"));
        assert_eq!(resp.team.as_deref(), Some("My Team"));
    }

    #[test]
    fn deserialize_auth_test_error() {
        let json = r#"{
            "ok": false,
            "error": "invalid_auth"
        }"#;

        let resp: AuthTestResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("invalid_auth"));
    }

    #[test]
    fn serialize_post_message_request() {
        let req = PostMessageRequest {
            channel: "C789".into(),
            text: "hello".into(),
            thread_ts: Some("1234567890.111111".into()),
            blocks: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["channel"], "C789");
        assert_eq!(json["text"], "hello");
        assert_eq!(json["thread_ts"], "1234567890.111111");
    }

    #[test]
    fn serialize_post_message_no_thread() {
        let req = PostMessageRequest {
            channel: "C789".into(),
            text: "hello".into(),
            thread_ts: None,
            blocks: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("thread_ts").is_none());
    }

    #[test]
    fn deserialize_minimal_event_callback() {
        let json = r#"{
            "type": "event_callback",
            "event": {
                "type": "message"
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                assert!(cb.token.is_none());
                assert!(cb.team_id.is_none());
                assert!(cb.event.user.is_none());
                assert!(cb.event.text.is_none());
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn serialize_post_message_with_blocks() {
        let req = PostMessageRequest {
            channel: "C789".into(),
            text: "fallback".into(),
            thread_ts: None,
            blocks: Some(vec![serde_json::json!({
                "type": "section",
                "text": { "type": "mrkdwn", "text": "*Hello*" }
            })]),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["text"], "fallback");
        let blocks = json["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "section");
    }

    #[test]
    fn serialize_post_message_without_blocks() {
        let req = PostMessageRequest {
            channel: "C789".into(),
            text: "hello".into(),
            thread_ts: None,
            blocks: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("blocks").is_none());
    }

    #[test]
    fn deserialize_slash_command_payload() {
        let form = "command=%2Fborg&text=hello+world&user_id=U123&channel_id=C456";
        let payload: SlashCommandPayload = serde_urlencoded::from_str(form).unwrap();
        assert_eq!(payload.command, "/borg");
        assert_eq!(payload.text.as_deref(), Some("hello world"));
        assert_eq!(payload.user_id, "U123");
        assert_eq!(payload.channel_id, "C456");
        assert!(payload.user_name.is_none());
        assert!(payload.team_id.is_none());
        assert!(payload.response_url.is_none());
        assert!(payload.trigger_id.is_none());
    }

    #[test]
    fn deserialize_slash_command_full() {
        let form = "command=%2Fborg&text=deploy&user_id=U123&user_name=alice\
            &channel_id=C456&channel_name=general&team_id=T789\
            &response_url=https%3A%2F%2Fhooks.slack.com%2Fcommands%2Fxyz\
            &trigger_id=tr1";
        let payload: SlashCommandPayload = serde_urlencoded::from_str(form).unwrap();
        assert_eq!(payload.command, "/borg");
        assert_eq!(payload.text.as_deref(), Some("deploy"));
        assert_eq!(payload.user_name.as_deref(), Some("alice"));
        assert_eq!(payload.channel_name.as_deref(), Some("general"));
        assert_eq!(payload.team_id.as_deref(), Some("T789"));
        assert!(payload.response_url.is_some());
        assert_eq!(payload.trigger_id.as_deref(), Some("tr1"));
    }

    // ── File and reaction type tests ──────────────────────────────────

    #[test]
    fn deserialize_event_with_files() {
        let json = r#"{
            "type": "event_callback",
            "token": "tok",
            "team_id": "T123",
            "event_id": "Ev123",
            "event": {
                "type": "message",
                "user": "U456",
                "text": "check this",
                "ts": "1234567890.123456",
                "channel": "C789",
                "files": [
                    {
                        "id": "F123",
                        "name": "photo.png",
                        "mimetype": "image/png",
                        "filetype": "png",
                        "url_private_download": "https://files.slack.com/photo.png",
                        "size": 2048
                    }
                ]
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                assert_eq!(cb.event.files.len(), 1);
                assert_eq!(cb.event.files[0].name.as_deref(), Some("photo.png"));
                assert_eq!(cb.event.files[0].mimetype.as_deref(), Some("image/png"));
                assert_eq!(cb.event.files[0].size, Some(2048));
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn deserialize_event_without_files() {
        let json = r#"{
            "type": "event_callback",
            "event": {
                "type": "message",
                "user": "U456",
                "text": "no files"
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                assert!(cb.event.files.is_empty());
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn deserialize_reaction_added_event() {
        let json = r#"{
            "type": "event_callback",
            "token": "tok",
            "team_id": "T123",
            "event_id": "Ev789",
            "event": {
                "type": "reaction_added",
                "user": "U456",
                "reaction": "thumbsup",
                "item": {
                    "channel": "C789",
                    "ts": "1234567890.123456"
                }
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                assert_eq!(cb.event.event_type, "reaction_added");
                assert_eq!(cb.event.reaction.as_deref(), Some("thumbsup"));
                let item = cb.event.item.as_ref().unwrap();
                assert_eq!(item.channel.as_deref(), Some("C789"));
                assert_eq!(item.ts.as_deref(), Some("1234567890.123456"));
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn deserialize_slack_file_minimal() {
        let json = r#"{"id": "F123"}"#;
        let file: SlackFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.id.as_deref(), Some("F123"));
        assert!(file.name.is_none());
        assert!(file.mimetype.is_none());
        assert!(file.url_private_download.is_none());
        assert!(file.size.is_none());
    }

    // ── Message editing types ─────────────────────────────────────────

    #[test]
    fn deserialize_post_message_response_with_ts() {
        let json = r#"{"ok": true, "ts": "1234567890.123456", "channel": "C789"}"#;
        let resp: PostMessageResponse = serde_json::from_str(json).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.ts.as_deref(), Some("1234567890.123456"));
        assert_eq!(resp.channel.as_deref(), Some("C789"));
    }

    #[test]
    fn deserialize_post_message_response_error() {
        let json = r#"{"ok": false, "error": "channel_not_found"}"#;
        let resp: PostMessageResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("channel_not_found"));
        assert!(resp.ts.is_none());
    }

    #[test]
    fn serialize_update_message_request() {
        let req = UpdateMessageRequest {
            channel: "C789".into(),
            ts: "1234567890.123456".into(),
            text: "updated text".into(),
            blocks: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["channel"], "C789");
        assert_eq!(json["ts"], "1234567890.123456");
        assert_eq!(json["text"], "updated text");
        assert!(json.get("blocks").is_none());
    }

    #[test]
    fn serialize_update_message_request_with_blocks() {
        let req = UpdateMessageRequest {
            channel: "C789".into(),
            ts: "1234567890.123456".into(),
            text: "fallback".into(),
            blocks: Some(vec![serde_json::json!({"type": "section"})]),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["blocks"].as_array().unwrap().len(), 1);
    }
}
