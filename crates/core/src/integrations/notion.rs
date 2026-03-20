use reqwest::Client;
use serde_json::{json, Value};

use super::http::{send_and_check, send_json};
use super::{format_list, require_str, validate_resource_id};
use crate::config::Config;
use crate::types::ToolDefinition;

const API_BASE: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";

pub fn tool_definition() -> ToolDefinition {
    ToolDefinition::new(
        "notion",
        "Search, create pages, read pages, and query databases in Notion.",
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "create_page", "read_page", "query_db"],
                    "description": "Action to perform"
                },
                "query": { "type": "string", "description": "Search query (for search)" },
                "title": { "type": "string", "description": "Page title (for create_page)" },
                "content": { "type": "string", "description": "Page content text (for create_page)" },
                "parent_id": { "type": "string", "description": "Parent page ID (for create_page)" },
                "page_id": { "type": "string", "description": "Page ID (for read_page)" },
                "database_id": { "type": "string", "description": "Database ID (for query_db)" }
            },
            "required": ["action"]
        }),
    )
}

pub async fn handle(arguments: &Value, config: &Config) -> Result<String, String> {
    let (client, token, action) =
        super::resolve_credential_and_action(arguments, config, "NOTION_API_KEY")?;

    match action {
        "search" => search(&client, &token, arguments).await,
        "create_page" => create_page(&client, &token, arguments).await,
        "read_page" => read_page(&client, &token, arguments).await,
        "query_db" => query_database(&client, &token, arguments).await,
        _ => Err(format!("Unknown action: {action}")),
    }
}

/// Build a Notion API request with required auth and version headers.
fn notion_request(
    client: &Client,
    method: reqwest::Method,
    url: &str,
    token: &str,
) -> reqwest::RequestBuilder {
    client
        .request(method, url)
        .bearer_auth(token)
        .header("Notion-Version", NOTION_VERSION)
}

async fn search(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let query = args["query"].as_str().unwrap_or("");

    let result: Value = send_json(
        notion_request(
            client,
            reqwest::Method::POST,
            &format!("{API_BASE}/search"),
            token,
        )
        .json(&json!({ "query": query })),
        "Notion",
    )
    .await?;

    Ok(format_list(
        result["results"].as_array().into_iter().flatten().take(20),
        |n| format!("Found {n} result(s):"),
        "No results found.",
        |item| {
            let obj_type = item["object"].as_str().unwrap_or("unknown");
            let id = item["id"].as_str().unwrap_or("");
            let title = extract_title(item).unwrap_or_else(|| "(untitled)".to_string());
            format!("- [{obj_type}] {title} (id: {id})")
        },
    ))
}

async fn create_page(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let title = require_str(args, "title")?;
    let content = args["content"].as_str().unwrap_or("");
    let parent_id = require_str(args, "parent_id")?;
    let parent_id = validate_resource_id(parent_id, "parent_id")?;

    let payload = json!({
        "parent": { "page_id": parent_id },
        "properties": {
            "title": {
                "title": [{ "text": { "content": title } }]
            }
        },
        "children": [
            {
                "object": "block",
                "type": "paragraph",
                "paragraph": {
                    "rich_text": [{ "text": { "content": content } }]
                }
            }
        ]
    });

    let result: Value = send_json(
        notion_request(
            client,
            reqwest::Method::POST,
            &format!("{API_BASE}/pages"),
            token,
        )
        .json(&payload),
        "Notion",
    )
    .await?;

    let id = result["id"].as_str().unwrap_or("unknown");
    let url = result["url"].as_str().unwrap_or("");
    Ok(format!("Page created: {title} (id: {id}, url: {url})"))
}

async fn read_page(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let page_id = require_str(args, "page_id")?;
    let page_id = validate_resource_id(page_id, "page_id")?;

    // Fetch page properties
    let page: Value = send_json(
        notion_request(
            client,
            reqwest::Method::GET,
            &format!("{API_BASE}/pages/{page_id}"),
            token,
        ),
        "Notion",
    )
    .await?;

    let title = extract_title(&page).unwrap_or_else(|| "(untitled)".to_string());

    // Fetch page content (blocks)
    let blocks_resp = send_and_check(
        notion_request(
            client,
            reqwest::Method::GET,
            &format!("{API_BASE}/blocks/{page_id}/children"),
            token,
        ),
        "Notion",
    )
    .await;

    let mut content = String::new();
    if let Ok(resp) = blocks_resp {
        let blocks: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
        if let Some(results) = blocks["results"].as_array() {
            for block in results {
                if let Some(text) = extract_block_text(block) {
                    content.push_str(&text);
                    content.push('\n');
                }
            }
        }
    }

    Ok(format!("# {title}\n\n{content}"))
}

async fn query_database(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let database_id = require_str(args, "database_id")?;
    let database_id = validate_resource_id(database_id, "database_id")?;

    let result: Value = send_json(
        notion_request(
            client,
            reqwest::Method::POST,
            &format!("{API_BASE}/databases/{database_id}/query"),
            token,
        )
        .json(&json!({ "page_size": 20 })),
        "Notion",
    )
    .await?;

    Ok(format_list(
        result["results"].as_array().into_iter().flatten(),
        |n| format!("Database has {n} row(s):"),
        "Database is empty.",
        |item| {
            let id = item["id"].as_str().unwrap_or("");
            let title = extract_title(item).unwrap_or_else(|| "(untitled)".to_string());
            format!("- {title} (id: {id})")
        },
    ))
}

/// Extract title from a Notion page's properties.
fn extract_title(item: &Value) -> Option<String> {
    let properties = item["properties"].as_object()?;
    for (_key, prop) in properties {
        if prop["type"].as_str() == Some("title") {
            if let Some(title_arr) = prop["title"].as_array() {
                let text: String = title_arr
                    .iter()
                    .filter_map(|t| t["plain_text"].as_str())
                    .collect();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
    }
    None
}

/// Extract text content from a Notion block.
fn extract_block_text(block: &Value) -> Option<String> {
    let block_type = block["type"].as_str()?;
    let rich_text = block[block_type]["rich_text"].as_array()?;
    let text: String = rich_text
        .iter()
        .filter_map(|t| t["plain_text"].as_str())
        .collect();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let def = tool_definition();
        assert_eq!(def.function.name, "notion");
        assert!(def.function.parameters["properties"]["action"].is_object());
    }

    #[test]
    fn test_extract_title() {
        let page = json!({
            "properties": {
                "Name": {
                    "type": "title",
                    "title": [{ "plain_text": "My Page" }]
                }
            }
        });
        assert_eq!(extract_title(&page), Some("My Page".to_string()));
    }

    #[test]
    fn test_extract_title_missing() {
        let page = json!({ "properties": {} });
        assert_eq!(extract_title(&page), None);
    }

    #[test]
    fn test_extract_block_text() {
        let block = json!({
            "type": "paragraph",
            "paragraph": {
                "rich_text": [
                    { "plain_text": "Hello " },
                    { "plain_text": "world" }
                ]
            }
        });
        assert_eq!(extract_block_text(&block), Some("Hello world".to_string()));
    }

    #[test]
    fn extract_block_text_empty_rich_text() {
        let block = json!({
            "type": "paragraph",
            "paragraph": {
                "rich_text": []
            }
        });
        assert_eq!(extract_block_text(&block), None);
    }

    integration_handle_tests!(notion, "NOTION_API_KEY");
}
