use serde::{Deserialize, Deserializer, Serialize};

/// Discord interaction types.
///
/// Uses a custom deserializer to map unknown u8 values to `Unknown(u8)`.
#[derive(Debug, Clone, PartialEq)]
pub enum InteractionType {
    Ping,
    ApplicationCommand,
    MessageComponent,
    ApplicationCommandAutocomplete,
    ModalSubmit,
    Unknown(u8),
}

impl<'de> Deserialize<'de> for InteractionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Ok(match value {
            1 => InteractionType::Ping,
            2 => InteractionType::ApplicationCommand,
            3 => InteractionType::MessageComponent,
            4 => InteractionType::ApplicationCommandAutocomplete,
            5 => InteractionType::ModalSubmit,
            other => InteractionType::Unknown(other),
        })
    }
}

/// A Discord interaction received via webhook.
#[derive(Debug, Clone, Deserialize)]
pub struct Interaction {
    pub id: String,
    #[serde(rename = "type")]
    pub interaction_type: InteractionType,
    pub application_id: Option<String>,
    pub data: Option<InteractionData>,
    pub member: Option<GuildMember>,
    pub user: Option<DiscordUser>,
    pub token: String,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
}

/// Data payload for application commands and components.
#[derive(Debug, Clone, Deserialize)]
pub struct InteractionData {
    pub id: Option<String>,
    pub name: Option<String>,
    pub options: Option<Vec<CommandOption>>,
    pub custom_id: Option<String>,
}

/// A single command option (name + value).
#[derive(Debug, Clone, Deserialize)]
pub struct CommandOption {
    pub name: String,
    pub value: serde_json::Value,
}

/// A Discord guild member (may contain a nested user).
#[derive(Debug, Clone, Deserialize)]
pub struct GuildMember {
    pub user: Option<DiscordUser>,
}

/// A Discord user.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscordUser {
    pub id: String,
    pub username: String,
    pub bot: Option<bool>,
}

/// Response sent back to Discord for an interaction.
#[derive(Debug, Clone, Serialize)]
pub struct InteractionResponse {
    #[serde(rename = "type")]
    pub response_type: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<InteractionResponseData>,
}

/// Data within an interaction response.
#[derive(Debug, Clone, Serialize)]
pub struct InteractionResponseData {
    pub content: String,
}

impl InteractionResponse {
    /// PONG response (type 1) — reply to a Ping interaction.
    pub fn pong() -> Self {
        Self {
            response_type: 1,
            data: None,
        }
    }

    /// Channel message with source response (type 4).
    pub fn message(text: &str) -> Self {
        Self {
            response_type: 4,
            data: Some(InteractionResponseData {
                content: text.to_string(),
            }),
        }
    }

    /// Deferred channel message with source response (type 5).
    pub fn deferred() -> Self {
        Self {
            response_type: 5,
            data: None,
        }
    }
}

/// Response from GET /users/@me.
#[derive(Debug, Clone, Deserialize)]
pub struct CurrentUser {
    pub id: String,
    pub username: String,
    pub bot: Option<bool>,
}

/// Request body for POST /channels/{id}/messages.
#[derive(Debug, Clone, Serialize)]
pub struct CreateMessageRequest {
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_ping_interaction() {
        let json = r#"{
            "id": "123",
            "type": 1,
            "token": "tok",
            "application_id": "app1"
        }"#;
        let interaction: Interaction = serde_json::from_str(json).unwrap();
        assert_eq!(interaction.interaction_type, InteractionType::Ping);
        assert_eq!(interaction.id, "123");
        assert_eq!(interaction.application_id.as_deref(), Some("app1"));
    }

    #[test]
    fn deserialize_application_command() {
        let json = r#"{
            "id": "456",
            "type": 2,
            "token": "tok",
            "application_id": "app1",
            "data": {
                "id": "cmd1",
                "name": "ask",
                "options": [
                    { "name": "question", "value": "What is Rust?" }
                ]
            },
            "member": {
                "user": { "id": "u1", "username": "alice", "bot": false }
            },
            "channel_id": "ch1",
            "guild_id": "g1"
        }"#;
        let interaction: Interaction = serde_json::from_str(json).unwrap();
        assert_eq!(
            interaction.interaction_type,
            InteractionType::ApplicationCommand
        );
        let data = interaction.data.as_ref().unwrap();
        assert_eq!(data.name.as_deref(), Some("ask"));
        let opts = data.options.as_ref().unwrap();
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].name, "question");
        assert_eq!(opts[0].value.as_str(), Some("What is Rust?"));
        let member = interaction.member.as_ref().unwrap();
        let user = member.user.as_ref().unwrap();
        assert_eq!(user.id, "u1");
        assert_eq!(user.bot, Some(false));
    }

    #[test]
    fn deserialize_message_component() {
        let json = r#"{
            "id": "789",
            "type": 3,
            "token": "tok",
            "data": { "custom_id": "btn_confirm" },
            "user": { "id": "u2", "username": "bob" }
        }"#;
        let interaction: Interaction = serde_json::from_str(json).unwrap();
        assert_eq!(
            interaction.interaction_type,
            InteractionType::MessageComponent
        );
        assert_eq!(
            interaction.data.as_ref().unwrap().custom_id.as_deref(),
            Some("btn_confirm")
        );
    }

    #[test]
    fn deserialize_unknown_interaction_type() {
        let json = r#"{
            "id": "999",
            "type": 99,
            "token": "tok"
        }"#;
        let interaction: Interaction = serde_json::from_str(json).unwrap();
        assert_eq!(interaction.interaction_type, InteractionType::Unknown(99));
    }

    #[test]
    fn interaction_response_pong_serialization() {
        let resp = InteractionResponse::pong();
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["type"], 1);
        assert!(json.get("data").is_none());
    }

    #[test]
    fn interaction_response_message_serialization() {
        let resp = InteractionResponse::message("Hello!");
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["type"], 4);
        assert_eq!(json["data"]["content"], "Hello!");
    }

    #[test]
    fn interaction_response_deferred_serialization() {
        let resp = InteractionResponse::deferred();
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["type"], 5);
        assert!(json.get("data").is_none());
    }

    #[test]
    fn deserialize_autocomplete_interaction() {
        let json = r#"{
            "id": "100",
            "type": 4,
            "token": "tok",
            "data": { "name": "search" }
        }"#;
        let interaction: Interaction = serde_json::from_str(json).unwrap();
        assert_eq!(
            interaction.interaction_type,
            InteractionType::ApplicationCommandAutocomplete
        );
    }

    #[test]
    fn deserialize_modal_submit() {
        let json = r#"{
            "id": "200",
            "type": 5,
            "token": "tok",
            "data": { "custom_id": "feedback_form" },
            "user": { "id": "u3", "username": "carol" }
        }"#;
        let interaction: Interaction = serde_json::from_str(json).unwrap();
        assert_eq!(interaction.interaction_type, InteractionType::ModalSubmit);
    }

    #[test]
    fn deserialize_bot_user() {
        let json = r#"{
            "id": "300",
            "type": 2,
            "token": "tok",
            "user": { "id": "bot1", "username": "mybot", "bot": true }
        }"#;
        let interaction: Interaction = serde_json::from_str(json).unwrap();
        assert_eq!(interaction.user.as_ref().unwrap().bot, Some(true));
    }

    #[test]
    fn create_message_request_serialization() {
        let req = CreateMessageRequest {
            content: "test message".into(),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["content"], "test message");
    }

    #[test]
    fn deserialize_current_user() {
        let json = r#"{ "id": "u1", "username": "borgbot", "bot": true }"#;
        let user: CurrentUser = serde_json::from_str(json).unwrap();
        assert_eq!(user.id, "u1");
        assert_eq!(user.username, "borgbot");
        assert_eq!(user.bot, Some(true));
    }
}
