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
}
