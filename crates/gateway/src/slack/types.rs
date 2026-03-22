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

/// Inner Slack event (message or app_mention).
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
    pub files: Option<Vec<SlackFile>>,
}

/// File metadata attached to a Slack message event.
#[derive(Debug, Clone, Deserialize)]
pub struct SlackFile {
    pub id: String,
    pub name: Option<String>,
    pub mimetype: Option<String>,
    pub url_private: Option<String>,
    pub size: Option<u64>,
    pub filetype: Option<String>,
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

/// Slack interactive payload (Block Kit buttons, select menus, modals).
/// Sent as `application/x-www-form-urlencoded` with a `payload` JSON field.
#[derive(Debug, Clone, Deserialize)]
pub struct InteractionPayload {
    #[serde(rename = "type")]
    pub interaction_type: String,
    pub user: InteractionUser,
    pub channel: Option<InteractionChannel>,
    pub actions: Option<Vec<BlockAction>>,
    pub trigger_id: Option<String>,
    pub response_url: Option<String>,
    pub view: Option<ViewPayload>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InteractionUser {
    pub id: String,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InteractionChannel {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockAction {
    pub action_id: String,
    #[serde(rename = "type")]
    pub action_type: Option<String>,
    pub value: Option<String>,
    pub block_id: Option<String>,
    pub selected_option: Option<SelectedOption>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelectedOption {
    pub value: String,
    pub text: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ViewPayload {
    pub callback_id: Option<String>,
    pub state: Option<ViewState>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ViewState {
    pub values: std::collections::HashMap<String, std::collections::HashMap<String, ActionValue>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionValue {
    #[serde(rename = "type")]
    pub action_type: Option<String>,
    pub value: Option<String>,
    pub selected_option: Option<SelectedOption>,
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
    fn deserialize_event_with_files() {
        let json = r#"{
            "type": "event_callback",
            "token": "tok",
            "team_id": "T123",
            "event_id": "Ev123",
            "event": {
                "type": "message",
                "user": "U456",
                "text": "check this file",
                "ts": "1234567890.123456",
                "channel": "C789",
                "files": [
                    {
                        "id": "F123",
                        "name": "report.pdf",
                        "mimetype": "application/pdf",
                        "url_private": "https://files.slack.com/files-pri/T123-F123/report.pdf",
                        "size": 2048,
                        "filetype": "pdf"
                    }
                ]
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                let files = cb.event.files.unwrap();
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].id, "F123");
                assert_eq!(files[0].name.as_deref(), Some("report.pdf"));
                assert_eq!(files[0].mimetype.as_deref(), Some("application/pdf"));
                assert_eq!(files[0].size, Some(2048));
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn deserialize_event_without_files() {
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
                "channel": "C789"
            }
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback(cb) => {
                assert!(cb.event.files.is_none());
            }
            _ => panic!("expected EventCallback"),
        }
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

    // ── Interactive payload tests ──

    #[test]
    fn deserialize_block_actions_payload() {
        let json = r#"{
            "type": "block_actions",
            "user": { "id": "U123", "username": "alice" },
            "channel": { "id": "C456", "name": "general" },
            "trigger_id": "tr1",
            "response_url": "https://hooks.slack.com/actions/T123/456/abc",
            "actions": [
                {
                    "action_id": "approve_btn",
                    "type": "button",
                    "value": "approved",
                    "block_id": "block_1"
                }
            ]
        }"#;

        let payload: InteractionPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.interaction_type, "block_actions");
        assert_eq!(payload.user.id, "U123");
        assert_eq!(payload.user.username.as_deref(), Some("alice"));
        assert_eq!(payload.channel.as_ref().unwrap().id, "C456");
        assert!(payload.response_url.is_some());
        let actions = payload.actions.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_id, "approve_btn");
        assert_eq!(actions[0].value.as_deref(), Some("approved"));
    }

    #[test]
    fn deserialize_select_menu_action() {
        let json = r#"{
            "type": "block_actions",
            "user": { "id": "U123" },
            "actions": [
                {
                    "action_id": "priority_select",
                    "type": "static_select",
                    "block_id": "block_2",
                    "selected_option": {
                        "value": "high",
                        "text": { "type": "plain_text", "text": "High" }
                    }
                }
            ]
        }"#;

        let payload: InteractionPayload = serde_json::from_str(json).unwrap();
        let actions = payload.actions.unwrap();
        assert!(actions[0].value.is_none());
        let opt = actions[0].selected_option.as_ref().unwrap();
        assert_eq!(opt.value, "high");
    }

    #[test]
    fn deserialize_view_submission() {
        let json = r#"{
            "type": "view_submission",
            "user": { "id": "U123" },
            "view": {
                "callback_id": "feedback_form",
                "state": {
                    "values": {
                        "block_1": {
                            "input_1": {
                                "type": "plain_text_input",
                                "value": "Great product!"
                            }
                        }
                    }
                }
            }
        }"#;

        let payload: InteractionPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.interaction_type, "view_submission");
        let view = payload.view.unwrap();
        assert_eq!(view.callback_id.as_deref(), Some("feedback_form"));
        let state = view.state.unwrap();
        let val = &state.values["block_1"]["input_1"];
        assert_eq!(val.value.as_deref(), Some("Great product!"));
    }

    #[test]
    fn deserialize_interaction_no_channel() {
        let json = r#"{
            "type": "view_submission",
            "user": { "id": "U123" },
            "view": { "callback_id": "test" }
        }"#;

        let payload: InteractionPayload = serde_json::from_str(json).unwrap();
        assert!(payload.channel.is_none());
        assert!(payload.actions.is_none());
    }
}
