use anyhow::{bail, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::http::{send_and_check, send_json};
use super::{format_list, require_str, validate_resource_id};
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

pub async fn handle(arguments: &Value, config: &Config) -> Result<String> {
    let (client, token, action) =
        super::resolve_credential_and_action(arguments, config, "MS_GRAPH_TOKEN")?;

    match action {
        "send" => send_email(&client, &token, arguments).await,
        "search" => search_emails(&client, &token, arguments).await,
        "read" => read_email(&client, &token, arguments).await,
        _ => bail!("Unknown action: {action}"),
    }
}

async fn send_email(client: &Client, token: &str, args: &Value) -> Result<String> {
    let to = require_str(args, "to")?;
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

    send_and_check(
        client
            .post(format!("{GRAPH_API}/sendMail"))
            .bearer_auth(token)
            .json(&payload),
        "Graph",
    )
    .await?;

    Ok(format!("Email sent to {to}"))
}

async fn search_emails(client: &Client, token: &str, args: &Value) -> Result<String> {
    let query = require_str(args, "query")?;
    let limit = args["limit"].as_u64().unwrap_or(10);

    let result: Value = send_json(
        client
            .get(format!("{GRAPH_API}/messages"))
            .bearer_auth(token)
            .query(&[
                ("$search", &format!("\"{}\"", query.replace('"', ""))),
                ("$top", &limit.to_string()),
                ("$select", &"id,subject,from,receivedDateTime".to_string()),
            ]),
        "Graph",
    )
    .await?;

    Ok(format_list(
        result["value"].as_array().into_iter().flatten(),
        |n| format!("Found {n} email(s):"),
        "No emails found.",
        |m| {
            let subject = m["subject"].as_str().unwrap_or("(no subject)");
            let from = m["from"]["emailAddress"]["address"]
                .as_str()
                .unwrap_or("unknown");
            format!("- {subject} (from: {from})")
        },
    ))
}

async fn read_email(client: &Client, token: &str, args: &Value) -> Result<String> {
    let message_id = require_str(args, "message_id")?;
    let message_id = validate_resource_id(message_id, "message_id")?;

    let msg: Value = send_json(
        client
            .get(format!("{GRAPH_API}/messages/{message_id}"))
            .bearer_auth(token),
        "Graph",
    )
    .await?;

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

    integration_handle_tests!(outlook, "MS_GRAPH_TOKEN");
}
