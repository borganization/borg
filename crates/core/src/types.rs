use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl Message {
    fn now_rfc3339() -> String {
        chrono::Local::now().to_rfc3339()
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None, // system prompts don't need timestamps
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            timestamp: Some(Self::now_rfc3339()),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            timestamp: Some(Self::now_rfc3339()),
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            timestamp: Some(Self::now_rfc3339()),
        }
    }
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_system_constructor() {
        let msg = Message::system("You are helpful.");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.content.as_deref(), Some("You are helpful."));
        assert!(msg.tool_calls.is_none());
        assert!(msg.tool_call_id.is_none());
    }

    #[test]
    fn message_user_constructor() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn message_assistant_constructor() {
        let msg = Message::assistant("Hi there");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.as_deref(), Some("Hi there"));
    }

    #[test]
    fn message_tool_result_constructor() {
        let msg = Message::tool_result("call_123", "result text");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.content.as_deref(), Some("result text"));
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_123"));
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn message_constructors_accept_string_types() {
        let owned = String::from("owned string");
        let msg = Message::user(owned);
        assert_eq!(msg.content.as_deref(), Some("owned string"));

        let msg2 = Message::system("static str");
        assert_eq!(msg2.content.as_deref(), Some("static str"));
    }

    #[test]
    fn role_serializes_to_lowercase() {
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), "\"system\"");
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            "\"assistant\""
        );
        assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), "\"tool\"");
    }

    #[test]
    fn role_deserializes_from_lowercase() {
        let role: Role = serde_json::from_str("\"system\"").unwrap();
        assert_eq!(role, Role::System);
        let role: Role = serde_json::from_str("\"tool\"").unwrap();
        assert_eq!(role, Role::Tool);
    }

    #[test]
    fn message_serialization_skips_none_fields() {
        let msg = Message::user("test");
        let json = serde_json::to_value(&msg).unwrap();
        assert!(!json.as_object().unwrap().contains_key("tool_calls"));
        assert!(!json.as_object().unwrap().contains_key("tool_call_id"));
    }

    #[test]
    fn tool_definition_new_sets_function_type() {
        let td = ToolDefinition::new("my_tool", "A tool", serde_json::json!({"type": "object"}));
        assert_eq!(td.tool_type, "function");
        assert_eq!(td.function.name, "my_tool");
        assert_eq!(td.function.description, "A tool");
        assert_eq!(td.function.parameters["type"], "object");
    }

    #[test]
    fn tool_call_json_round_trip() {
        let tc = ToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "read_memory".to_string(),
                arguments: "{\"filename\":\"test.md\"}".to_string(),
            },
        };
        let json = serde_json::to_string(&tc).unwrap();
        let deserialized: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "call_1");
        assert_eq!(deserialized.function.name, "read_memory");
        assert_eq!(
            deserialized.function.arguments,
            "{\"filename\":\"test.md\"}"
        );
    }

    #[test]
    fn tool_result_serialization() {
        let tr = ToolResult {
            tool_call_id: "call_1".to_string(),
            content: "success".to_string(),
            is_error: false,
        };
        let json = serde_json::to_value(&tr).unwrap();
        assert_eq!(json["tool_call_id"], "call_1");
        assert_eq!(json["content"], "success");
        assert_eq!(json["is_error"], false);
    }
}
