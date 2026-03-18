use reqwest::Client;
use serde_json::{json, Value};

use super::validate_resource_id;
use crate::config::Config;
use crate::types::ToolDefinition;

const API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

pub fn tool_definition() -> ToolDefinition {
    ToolDefinition::new(
        "gmail",
        "Send, search, and read emails via Gmail API.",
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["send", "search", "read"],
                    "description": "Action to perform"
                },
                "to": { "type": "string", "description": "Recipient email (for send)" },
                "subject": { "type": "string", "description": "Email subject (for send)" },
                "body": { "type": "string", "description": "Email body (for send)" },
                "query": { "type": "string", "description": "Search query (for search)" },
                "message_id": { "type": "string", "description": "Message ID (for read)" },
                "limit": { "type": "integer", "description": "Max results (for search, default 10)" }
            },
            "required": ["action"]
        }),
    )
}

pub async fn handle(arguments: &Value, config: &Config) -> Result<String, String> {
    let token = config
        .resolve_credential_or_env("GMAIL_API_KEY")
        .ok_or_else(|| "GMAIL_API_KEY not configured".to_string())?;

    let action = arguments["action"]
        .as_str()
        .ok_or_else(|| "Missing 'action' parameter".to_string())?;

    let client = Client::new();

    match action {
        "send" => send_email(&client, &token, arguments).await,
        "search" => search_emails(&client, &token, arguments).await,
        "read" => read_email(&client, &token, arguments).await,
        _ => Err(format!("Unknown action: {action}")),
    }
}

async fn send_email(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let to = args["to"].as_str().ok_or("Missing 'to'")?;
    let subject = args["subject"].as_str().unwrap_or("(no subject)");
    let body = args["body"].as_str().unwrap_or("");

    // Prevent header injection via \r or \n in header fields
    if to.contains('\r') || to.contains('\n') {
        return Err("Invalid 'to': contains newline characters".to_string());
    }
    if subject.contains('\r') || subject.contains('\n') {
        return Err("Invalid 'subject': contains newline characters".to_string());
    }

    let raw = format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}"
    );
    let encoded = base64_url_encode(raw.as_bytes());

    let resp = client
        .post(format!("{API_BASE}/messages/send"))
        .bearer_auth(token)
        .json(&json!({ "raw": encoded }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail API error: {text}"));
    }

    let result: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    Ok(format!(
        "Email sent (id: {})",
        result["id"].as_str().unwrap_or("unknown")
    ))
}

async fn search_emails(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let query = args["query"].as_str().ok_or("Missing 'query'")?;
    let limit = args["limit"].as_u64().unwrap_or(10);

    let resp = client
        .get(format!("{API_BASE}/messages"))
        .bearer_auth(token)
        .query(&[("q", query), ("maxResults", &limit.to_string())])
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail API error: {text}"));
    }

    let result: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let messages = result["messages"].as_array();

    match messages {
        Some(msgs) if !msgs.is_empty() => {
            let ids: Vec<&str> = msgs.iter().filter_map(|m| m["id"].as_str()).collect();
            Ok(format!(
                "Found {} message(s): {}",
                ids.len(),
                ids.join(", ")
            ))
        }
        _ => Ok("No messages found.".to_string()),
    }
}

async fn read_email(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let message_id = args["message_id"].as_str().ok_or("Missing 'message_id'")?;
    let message_id = validate_resource_id(message_id, "message_id")?;

    let resp = client
        .get(format!("{API_BASE}/messages/{message_id}"))
        .bearer_auth(token)
        .query(&[("format", "full")])
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail API error: {text}"));
    }

    let result: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;

    let subject = extract_header(&result, "Subject").unwrap_or("(no subject)".to_string());
    let from = extract_header(&result, "From").unwrap_or("unknown".to_string());
    let snippet = result["snippet"].as_str().unwrap_or("");

    Ok(format!("From: {from}\nSubject: {subject}\n\n{snippet}"))
}

fn extract_header(message: &Value, name: &str) -> Option<String> {
    message["payload"]["headers"]
        .as_array()?
        .iter()
        .find(|h| h["name"].as_str() == Some(name))
        .and_then(|h| h["value"].as_str())
        .map(String::from)
}

fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let def = tool_definition();
        assert_eq!(def.function.name, "gmail");
        assert!(!def.function.description.is_empty());
        assert!(def.function.parameters["properties"]["action"].is_object());
    }

    #[test]
    fn test_base64_url_encode() {
        let encoded = base64_url_encode(b"To: test@example.com\r\nSubject: Hi\r\n\r\nBody");
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn test_extract_header() {
        let msg = json!({
            "payload": {
                "headers": [
                    { "name": "Subject", "value": "Test Subject" },
                    { "name": "From", "value": "alice@example.com" }
                ]
            }
        });
        assert_eq!(
            extract_header(&msg, "Subject"),
            Some("Test Subject".to_string())
        );
        assert_eq!(
            extract_header(&msg, "From"),
            Some("alice@example.com".to_string())
        );
        assert_eq!(extract_header(&msg, "Missing"), None);
    }

    #[tokio::test]
    async fn handle_missing_credential() {
        let config = Config::default();
        let args = json!({"action": "search", "query": "test"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "GMAIL_API_KEY not configured");
    }

    #[tokio::test]
    async fn handle_unknown_action() {
        let mut config = Config::default();
        config.credentials.insert(
            "GMAIL_API_KEY".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_GMAIL_API_KEY__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_GMAIL_API_KEY__", "fake-token");
        }
        let args = json!({"action": "delete"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Unknown action: delete");
    }

    #[tokio::test]
    async fn handle_missing_action_param() {
        let mut config = Config::default();
        config.credentials.insert(
            "GMAIL_API_KEY".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_GMAIL_API_KEY__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_GMAIL_API_KEY__", "fake-token");
        }
        let args = json!({});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Missing 'action' parameter");
    }
}
