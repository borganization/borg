use reqwest::Client;
use serde_json::{json, Value};

use super::validate_resource_id;
use crate::config::Config;
use crate::types::ToolDefinition;

const API_BASE: &str = "https://www.googleapis.com/calendar/v3";

pub fn tool_definition() -> ToolDefinition {
    ToolDefinition::new(
        "google_calendar",
        "List, create, and delete Google Calendar events.",
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "create", "delete"],
                    "description": "Action to perform"
                },
                "days": { "type": "integer", "description": "Number of days to list (default 7)" },
                "summary": { "type": "string", "description": "Event title (for create)" },
                "start": { "type": "string", "description": "Start datetime in RFC3339 (for create)" },
                "end": { "type": "string", "description": "End datetime in RFC3339 (for create)" },
                "description": { "type": "string", "description": "Event description (for create)" },
                "event_id": { "type": "string", "description": "Event ID (for delete)" }
            },
            "required": ["action"]
        }),
    )
}

pub async fn handle(arguments: &Value, config: &Config) -> Result<String, String> {
    let token = config
        .resolve_credential_or_env("GOOGLE_CALENDAR_TOKEN")
        .ok_or_else(|| "GOOGLE_CALENDAR_TOKEN not configured".to_string())?;

    let action = arguments["action"]
        .as_str()
        .ok_or_else(|| "Missing 'action' parameter".to_string())?;

    let client = Client::new();

    match action {
        "list" => list_events(&client, &token, arguments).await,
        "create" => create_event(&client, &token, arguments).await,
        "delete" => delete_event(&client, &token, arguments).await,
        _ => Err(format!("Unknown action: {action}")),
    }
}

async fn list_events(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let days = args["days"].as_u64().unwrap_or(7);
    let now = chrono::Utc::now();
    let time_min = now.to_rfc3339();
    let time_max = (now + chrono::TimeDelta::days(days as i64)).to_rfc3339();

    let resp = client
        .get(format!("{API_BASE}/calendars/primary/events"))
        .bearer_auth(token)
        .query(&[
            ("timeMin", &time_min),
            ("timeMax", &time_max),
            ("singleEvents", &"true".to_string()),
            ("orderBy", &"startTime".to_string()),
            ("maxResults", &"50".to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Calendar API error: {text}"));
    }

    let result: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let events = result["items"].as_array();

    match events {
        Some(evts) if !evts.is_empty() => {
            let summaries: Vec<String> = evts
                .iter()
                .map(|e| {
                    let summary = e["summary"].as_str().unwrap_or("(untitled)");
                    let start = e["start"]["dateTime"]
                        .as_str()
                        .or_else(|| e["start"]["date"].as_str())
                        .unwrap_or("?");
                    let id = e["id"].as_str().unwrap_or("");
                    format!("- {start}: {summary} (id: {id})")
                })
                .collect();
            Ok(format!(
                "Events in the next {days} day(s):\n{}",
                summaries.join("\n")
            ))
        }
        _ => Ok(format!("No events in the next {days} day(s).")),
    }
}

async fn create_event(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let summary = args["summary"].as_str().ok_or("Missing 'summary'")?;
    let start = args["start"]
        .as_str()
        .ok_or("Missing 'start' (RFC3339 datetime)")?;
    let end = args["end"]
        .as_str()
        .ok_or("Missing 'end' (RFC3339 datetime)")?;
    let description = args["description"].as_str().unwrap_or("");

    let payload = json!({
        "summary": summary,
        "description": description,
        "start": { "dateTime": start },
        "end": { "dateTime": end }
    });

    let resp = client
        .post(format!("{API_BASE}/calendars/primary/events"))
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Calendar API error: {text}"));
    }

    let result: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let id = result["id"].as_str().unwrap_or("unknown");
    Ok(format!("Event created: {summary} (id: {id})"))
}

async fn delete_event(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let event_id = args["event_id"].as_str().ok_or("Missing 'event_id'")?;
    let event_id = validate_resource_id(event_id, "event_id")?;

    let resp = client
        .delete(format!("{API_BASE}/calendars/primary/events/{event_id}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Calendar API error: {text}"));
    }

    Ok(format!("Event {event_id} deleted."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let def = tool_definition();
        assert_eq!(def.function.name, "google_calendar");
        assert!(def.function.parameters["properties"]["action"].is_object());
        assert!(def.function.parameters["properties"]["start"].is_object());
    }

    #[tokio::test]
    async fn handle_missing_credential() {
        let config = Config::default();
        let args = json!({"action": "list"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "GOOGLE_CALENDAR_TOKEN not configured");
    }

    #[tokio::test]
    async fn handle_unknown_action() {
        let mut config = Config::default();
        config.credentials.insert(
            "GOOGLE_CALENDAR_TOKEN".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_GCAL_TOKEN__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_GCAL_TOKEN__", "fake-token");
        }
        let args = json!({"action": "update"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Unknown action: update");
    }

    #[tokio::test]
    async fn handle_missing_action_param() {
        let mut config = Config::default();
        config.credentials.insert(
            "GOOGLE_CALENDAR_TOKEN".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_GCAL_TOKEN__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_GCAL_TOKEN__", "fake-token");
        }
        let args = json!({});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Missing 'action' parameter");
    }
}
