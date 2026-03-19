use reqwest::Client;
use serde_json::{json, Value};

use super::http::send_json;
use super::validate_resource_id;
use crate::config::Config;
use crate::types::ToolDefinition;

const GRAPHQL_URL: &str = "https://api.linear.app/graphql";

pub fn tool_definition() -> ToolDefinition {
    ToolDefinition::new(
        "linear",
        "List, create, and search issues in Linear via GraphQL.",
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "create", "search"],
                    "description": "Action to perform"
                },
                "team_id": { "type": "string", "description": "Team ID filter (for list/create)" },
                "title": { "type": "string", "description": "Issue title (for create)" },
                "description": { "type": "string", "description": "Issue description (for create)" },
                "query": { "type": "string", "description": "Search query (for search)" }
            },
            "required": ["action"]
        }),
    )
}

pub async fn handle(arguments: &Value, config: &Config) -> Result<String, String> {
    let token = config
        .resolve_credential_or_env("LINEAR_API_KEY")
        .ok_or_else(|| "LINEAR_API_KEY not configured".to_string())?;

    let action = arguments["action"]
        .as_str()
        .ok_or_else(|| "Missing 'action' parameter".to_string())?;

    let client = Client::new();

    match action {
        "list" => list_issues(&client, &token, arguments).await,
        "create" => create_issue(&client, &token, arguments).await,
        "search" => search_issues(&client, &token, arguments).await,
        _ => Err(format!("Unknown action: {action}")),
    }
}

async fn graphql_request(
    client: &Client,
    token: &str,
    query: &str,
    variables: Value,
) -> Result<Value, String> {
    let result: Value = send_json(
        client
            .post(GRAPHQL_URL)
            .bearer_auth(token)
            .json(&json!({ "query": query, "variables": variables })),
        "Linear",
    )
    .await?;

    if let Some(errors) = result["errors"].as_array() {
        if !errors.is_empty() {
            let msgs: Vec<&str> = errors
                .iter()
                .filter_map(|e| e["message"].as_str())
                .collect();
            return Err(format!("GraphQL errors: {}", msgs.join("; ")));
        }
    }

    Ok(result["data"].clone())
}

async fn list_issues(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let team_filter = args["team_id"]
        .as_str()
        .map(|id| validate_resource_id(id, "team_id"))
        .transpose()?;

    let query = if team_filter.is_some() {
        r#"query($teamId: String!) {
            issues(filter: { team: { id: { eq: $teamId } } }, first: 20, orderBy: updatedAt) {
                nodes { id identifier title state { name } assignee { name } updatedAt }
            }
        }"#
    } else {
        r#"query {
            issues(first: 20, orderBy: updatedAt) {
                nodes { id identifier title state { name } assignee { name } updatedAt }
            }
        }"#
    };

    let variables = match team_filter {
        Some(tid) => json!({ "teamId": tid }),
        None => json!({}),
    };

    let data = graphql_request(client, token, query, variables).await?;
    let nodes = data["issues"]["nodes"].as_array();

    match nodes {
        Some(issues) if !issues.is_empty() => {
            let summaries: Vec<String> = issues
                .iter()
                .map(|i| {
                    let identifier = i["identifier"].as_str().unwrap_or("");
                    let title = i["title"].as_str().unwrap_or("(untitled)");
                    let state = i["state"]["name"].as_str().unwrap_or("?");
                    let assignee = i["assignee"]["name"].as_str().unwrap_or("unassigned");
                    format!("- {identifier}: {title} [{state}] ({assignee})")
                })
                .collect();
            Ok(format!(
                "{} issue(s):\n{}",
                summaries.len(),
                summaries.join("\n")
            ))
        }
        _ => Ok("No issues found.".to_string()),
    }
}

async fn create_issue(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let title = args["title"].as_str().ok_or("Missing 'title'")?;
    let description = args["description"].as_str().unwrap_or("");
    let team_id = args["team_id"].as_str().ok_or("Missing 'team_id'")?;
    let team_id = validate_resource_id(team_id, "team_id")?;

    let query = r#"mutation($title: String!, $description: String, $teamId: String!) {
        issueCreate(input: { title: $title, description: $description, teamId: $teamId }) {
            success
            issue { id identifier title url }
        }
    }"#;

    let variables = json!({
        "title": title,
        "description": description,
        "teamId": team_id,
    });

    let data = graphql_request(client, token, query, variables).await?;

    if data["issueCreate"]["success"].as_bool() != Some(true) {
        return Err("Failed to create issue".to_string());
    }

    let issue = &data["issueCreate"]["issue"];
    let identifier = issue["identifier"].as_str().unwrap_or("?");
    let url = issue["url"].as_str().unwrap_or("");

    Ok(format!("Issue created: {identifier} — {title}\n{url}"))
}

async fn search_issues(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let search_query = args["query"].as_str().ok_or("Missing 'query'")?;

    let query = r#"query($query: String!) {
        searchIssues(query: $query, first: 20) {
            nodes { id identifier title state { name } assignee { name } }
        }
    }"#;

    let variables = json!({ "query": search_query });
    let data = graphql_request(client, token, query, variables).await?;
    let nodes = data["searchIssues"]["nodes"].as_array();

    match nodes {
        Some(issues) if !issues.is_empty() => {
            let summaries: Vec<String> = issues
                .iter()
                .map(|i| {
                    let identifier = i["identifier"].as_str().unwrap_or("");
                    let title = i["title"].as_str().unwrap_or("(untitled)");
                    let state = i["state"]["name"].as_str().unwrap_or("?");
                    format!("- {identifier}: {title} [{state}]")
                })
                .collect();
            Ok(format!(
                "Found {} issue(s):\n{}",
                summaries.len(),
                summaries.join("\n")
            ))
        }
        _ => Ok("No matching issues found.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let def = tool_definition();
        assert_eq!(def.function.name, "linear");
        assert!(def.function.parameters["properties"]["action"].is_object());
    }

    #[test]
    fn test_graphql_url() {
        assert_eq!(GRAPHQL_URL, "https://api.linear.app/graphql");
    }

    #[tokio::test]
    async fn handle_missing_credential() {
        let config = Config::default();
        let args = json!({"action": "list"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "LINEAR_API_KEY not configured");
    }

    #[tokio::test]
    async fn handle_unknown_action() {
        let mut config = Config::default();
        config.credentials.insert(
            "LINEAR_API_KEY".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_LINEAR_API_KEY__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_LINEAR_API_KEY__", "fake-token");
        }
        let args = json!({"action": "delete"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Unknown action: delete");
    }

    #[tokio::test]
    async fn handle_missing_action_param() {
        let mut config = Config::default();
        config.credentials.insert(
            "LINEAR_API_KEY".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_LINEAR_API_KEY__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_LINEAR_API_KEY__", "fake-token");
        }
        let args = json!({});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Missing 'action' parameter");
    }
}
