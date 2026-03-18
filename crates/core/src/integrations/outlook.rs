use reqwest::Client;
use serde_json::{json, Value};

use super::validate_resource_id;
use crate::config::Config;
use crate::types::ToolDefinition;

const GRAPH_API: &str = "https://graph.microsoft.com/v1.0/me";

pub fn tool_definition() -> ToolDefinition {
    ToolDefinition::new(
        "outlook",
        "Send, search, and read emails via Microsoft Graph (Outlook).",
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
        .resolve_credential_or_env("MS_GRAPH_TOKEN")
        .ok_or_else(|| "MS_GRAPH_TOKEN not configured".to_string())?;

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

    let payload = json!({
        "message": {
            "subject": subject,
            "body": { "contentType": "Text", "content": body },
            "toRecipients": [
                { "emailAddress": { "address": to } }
            ]
        }
    });

    let resp = client
        .post(format!("{GRAPH_API}/sendMail"))
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Graph API error: {text}"));
    }

    Ok(format!("Email sent to {to}"))
}

async fn search_emails(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let query = args["query"].as_str().ok_or("Missing 'query'")?;
    let limit = args["limit"].as_u64().unwrap_or(10);

    let resp = client
        .get(format!("{GRAPH_API}/messages"))
        .bearer_auth(token)
        .query(&[
            ("$search", &format!("\"{}\"", query.replace('"', ""))),
            ("$top", &limit.to_string()),
            ("$select", &"id,subject,from,receivedDateTime".to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Graph API error: {text}"));
    }

    let result: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let messages = result["value"].as_array();

    match messages {
        Some(msgs) if !msgs.is_empty() => {
            let summaries: Vec<String> = msgs
                .iter()
                .map(|m| {
                    let subject = m["subject"].as_str().unwrap_or("(no subject)");
                    let from = m["from"]["emailAddress"]["address"]
                        .as_str()
                        .unwrap_or("unknown");
                    format!("- {subject} (from: {from})")
                })
                .collect();
            Ok(format!(
                "Found {} email(s):\n{}",
                summaries.len(),
                summaries.join("\n")
            ))
        }
        _ => Ok("No emails found.".to_string()),
    }
}

async fn read_email(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let message_id = args["message_id"].as_str().ok_or("Missing 'message_id'")?;
    let message_id = validate_resource_id(message_id, "message_id")?;

    let resp = client
        .get(format!("{GRAPH_API}/messages/{message_id}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Graph API error: {text}"));
    }

    let msg: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let subject = msg["subject"].as_str().unwrap_or("(no subject)");
    let from = msg["from"]["emailAddress"]["address"]
        .as_str()
        .unwrap_or("unknown");
    let body = msg["body"]["content"].as_str().unwrap_or("");

    Ok(format!("From: {from}\nSubject: {subject}\n\n{body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let def = tool_definition();
        assert_eq!(def.function.name, "outlook");
        assert!(def.function.parameters["properties"]["action"].is_object());
    }

    #[tokio::test]
    async fn handle_missing_credential() {
        let config = Config::default();
        let args = json!({"action": "search", "query": "test"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "MS_GRAPH_TOKEN not configured");
    }

    #[tokio::test]
    async fn handle_unknown_action() {
        let mut config = Config::default();
        config.credentials.insert(
            "MS_GRAPH_TOKEN".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_MS_GRAPH_TOKEN__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_MS_GRAPH_TOKEN__", "fake-token");
        }
        let args = json!({"action": "delete"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Unknown action: delete");
    }

    #[tokio::test]
    async fn handle_missing_action_param() {
        let mut config = Config::default();
        config.credentials.insert(
            "MS_GRAPH_TOKEN".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_MS_GRAPH_TOKEN__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_MS_GRAPH_TOKEN__", "fake-token");
        }
        let args = json!({});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Missing 'action' parameter");
    }
}
