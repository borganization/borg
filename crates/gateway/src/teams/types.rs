use serde::{Deserialize, Serialize};

/// A Bot Framework Activity received from Microsoft Teams.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: String,
    #[serde(rename = "type")]
    pub activity_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub from: Option<ChannelAccount>,
    #[serde(default)]
    pub conversation: Option<ConversationAccount>,
    #[serde(default)]
    pub recipient: Option<ChannelAccount>,
    #[serde(default)]
    pub service_url: Option<String>,
    #[serde(default)]
    pub reply_to_id: Option<String>,
    #[serde(default)]
    pub entities: Option<Vec<Entity>>,
    #[serde(default)]
    pub timestamp: Option<String>,
}

/// A user or bot account in a Teams conversation.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChannelAccount {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// A conversation reference in a Teams activity.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAccount {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_group: Option<bool>,
}

/// An entity attached to an activity (e.g. an @mention).
#[derive(Debug, Clone, Deserialize)]
pub struct Entity {
    #[serde(rename = "type")]
    pub entity_type: String,
    #[serde(default)]
    pub mentioned: Option<ChannelAccount>,
}

/// An outbound reply activity sent back to Teams.
#[derive(Debug, Clone, Serialize)]
pub struct ReplyActivity {
    #[serde(rename = "type")]
    pub activity_type: String,
    pub text: String,
}

impl ReplyActivity {
    /// Create a new message reply activity.
    pub fn message(text: impl Into<String>) -> Self {
        Self {
            activity_type: "message".to_string(),
            text: text.into(),
        }
    }
}

/// OAuth2 token response from the Microsoft identity platform.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_message_activity() {
        let json = r#"{
            "id": "act-1",
            "type": "message",
            "text": "hello bot",
            "from": {"id": "user-1", "name": "Alice"},
            "conversation": {"id": "conv-1", "name": "General", "isGroup": true},
            "recipient": {"id": "bot-1", "name": "MyBot"},
            "serviceUrl": "https://smba.trafficmanager.net/teams/",
            "replyToId": "parent-1",
            "timestamp": "2026-03-17T12:00:00Z"
        }"#;

        let activity: Activity = serde_json::from_str(json).unwrap();
        assert_eq!(activity.id, "act-1");
        assert_eq!(activity.activity_type, "message");
        assert_eq!(activity.text.as_deref(), Some("hello bot"));
        assert_eq!(activity.from.as_ref().unwrap().id, "user-1");
        assert_eq!(
            activity.from.as_ref().unwrap().name.as_deref(),
            Some("Alice")
        );
        assert_eq!(activity.conversation.as_ref().unwrap().id, "conv-1");
        assert_eq!(activity.conversation.as_ref().unwrap().is_group, Some(true));
        assert_eq!(activity.recipient.as_ref().unwrap().id, "bot-1");
        assert_eq!(
            activity.service_url.as_deref(),
            Some("https://smba.trafficmanager.net/teams/")
        );
        assert_eq!(activity.reply_to_id.as_deref(), Some("parent-1"));
        assert_eq!(activity.timestamp.as_deref(), Some("2026-03-17T12:00:00Z"));
    }

    #[test]
    fn deserialize_minimal_activity() {
        let json = r#"{"id": "act-2", "type": "conversationUpdate"}"#;
        let activity: Activity = serde_json::from_str(json).unwrap();
        assert_eq!(activity.id, "act-2");
        assert_eq!(activity.activity_type, "conversationUpdate");
        assert!(activity.text.is_none());
        assert!(activity.from.is_none());
        assert!(activity.conversation.is_none());
        assert!(activity.recipient.is_none());
        assert!(activity.service_url.is_none());
        assert!(activity.entities.is_none());
    }

    #[test]
    fn deserialize_activity_with_entities() {
        let json = r#"{
            "id": "act-3",
            "type": "message",
            "text": "<at>MyBot</at> help me",
            "entities": [
                {"type": "mention", "mentioned": {"id": "bot-1", "name": "MyBot"}},
                {"type": "clientInfo"}
            ]
        }"#;
        let activity: Activity = serde_json::from_str(json).unwrap();
        let entities = activity.entities.unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].entity_type, "mention");
        assert_eq!(entities[0].mentioned.as_ref().unwrap().id, "bot-1");
        assert_eq!(entities[1].entity_type, "clientInfo");
        assert!(entities[1].mentioned.is_none());
    }

    #[test]
    fn serialize_reply_activity() {
        let reply = ReplyActivity::message("Hello back!");
        let json = serde_json::to_value(&reply).unwrap();
        assert_eq!(json["type"], "message");
        assert_eq!(json["text"], "Hello back!");
    }

    #[test]
    fn deserialize_token_response() {
        let json = r#"{"access_token": "eyJ0eXAi...", "expires_in": 3600}"#;
        let resp: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token, "eyJ0eXAi...");
        assert_eq!(resp.expires_in, 3600);
    }

    #[test]
    fn deserialize_conversation_account_no_group() {
        let json = r#"{"id": "conv-2"}"#;
        let conv: ConversationAccount = serde_json::from_str(json).unwrap();
        assert_eq!(conv.id, "conv-2");
        assert!(conv.name.is_none());
        assert!(conv.is_group.is_none());
    }

    #[test]
    fn channel_account_serialize_roundtrip() {
        let account = ChannelAccount {
            id: "user-1".to_string(),
            name: Some("Alice".to_string()),
        };
        let json = serde_json::to_string(&account).unwrap();
        let deserialized: ChannelAccount = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "user-1");
        assert_eq!(deserialized.name.as_deref(), Some("Alice"));
    }
}
