use reqwest::Client;
use serde_json::{json, Value};

use super::http::send_json;
use super::{format_list, require_str, validate_resource_id};
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
    let (client, token, action) =
        super::resolve_credential_and_action(arguments, config, "LINEAR_API_KEY")?;

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

    Ok(format_list(
        data["issues"]["nodes"].as_array().into_iter().flatten(),
        |n| format!("{n} issue(s):"),
        "No issues found.",
        |i| {
            let identifier = i["identifier"].as_str().unwrap_or("");
            let title = i["title"].as_str().unwrap_or("(untitled)");
            let state = i["state"]["name"].as_str().unwrap_or("?");
            let assignee = i["assignee"]["name"].as_str().unwrap_or("unassigned");
            format!("- {identifier}: {title} [{state}] ({assignee})")
        },
    ))
}

async fn create_issue(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let title = require_str(args, "title")?;
    let description = args["description"].as_str().unwrap_or("");
    let team_id = require_str(args, "team_id")?;
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
    let search_query = require_str(args, "query")?;

    let query = r#"query($query: String!) {
        searchIssues(query: $query, first: 20) {
            nodes { id identifier title state { name } assignee { name } }
        }
    }"#;

    let variables = json!({ "query": search_query });
    let data = graphql_request(client, token, query, variables).await?;

    Ok(format_list(
        data["searchIssues"]["nodes"]
            .as_array()
            .into_iter()
            .flatten(),
        |n| format!("Found {n} issue(s):"),
        "No matching issues found.",
        |i| {
            let identifier = i["identifier"].as_str().unwrap_or("");
            let title = i["title"].as_str().unwrap_or("(untitled)");
            let state = i["state"]["name"].as_str().unwrap_or("?");
            format!("- {identifier}: {title} [{state}]")
        },
    ))
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

    integration_handle_tests!(linear, "LINEAR_API_KEY");
}
