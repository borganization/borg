use reqwest::Client;
use serde_json::{json, Value};

use super::validate_resource_id;
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
    let token = config
        .resolve_credential_or_env("NOTION_API_KEY")
        .ok_or_else(|| "NOTION_API_KEY not configured".to_string())?;

    let action = arguments["action"]
        .as_str()
        .ok_or_else(|| "Missing 'action' parameter".to_string())?;

    let client = Client::new();

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

    let resp = notion_request(
        client,
        reqwest::Method::POST,
        &format!("{API_BASE}/search"),
        token,
    )
    .json(&json!({ "query": query }))
    .send()
    .await
    .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Notion API error: {text}"));
    }

    let result: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let results = result["results"].as_array();

    match results {
        Some(items) if !items.is_empty() => {
            let summaries: Vec<String> = items
                .iter()
                .take(20)
                .map(|item| {
                    let obj_type = item["object"].as_str().unwrap_or("unknown");
                    let id = item["id"].as_str().unwrap_or("");
                    let title = extract_title(item).unwrap_or_else(|| "(untitled)".to_string());
                    format!("- [{obj_type}] {title} (id: {id})")
                })
                .collect();
            Ok(format!(
                "Found {} result(s):\n{}",
                summaries.len(),
                summaries.join("\n")
            ))
        }
        _ => Ok("No results found.".to_string()),
    }
}

async fn create_page(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let title = args["title"].as_str().ok_or("Missing 'title'")?;
    let content = args["content"].as_str().unwrap_or("");
    let parent_id = args["parent_id"].as_str().ok_or("Missing 'parent_id'")?;
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

    let resp = notion_request(
        client,
        reqwest::Method::POST,
        &format!("{API_BASE}/pages"),
        token,
    )
    .json(&payload)
    .send()
    .await
    .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Notion API error: {text}"));
    }

    let result: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let id = result["id"].as_str().unwrap_or("unknown");
    let url = result["url"].as_str().unwrap_or("");
    Ok(format!("Page created: {title} (id: {id}, url: {url})"))
}

async fn read_page(client: &Client, token: &str, args: &Value) -> Result<String, String> {
    let page_id = args["page_id"].as_str().ok_or("Missing 'page_id'")?;
    let page_id = validate_resource_id(page_id, "page_id")?;

    // Fetch page properties
    let resp = notion_request(
        client,
        reqwest::Method::GET,
        &format!("{API_BASE}/pages/{page_id}"),
        token,
    )
    .send()
    .await
    .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Notion API error: {text}"));
    }

    let page: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let title = extract_title(&page).unwrap_or_else(|| "(untitled)".to_string());

    // Fetch page content (blocks)
    let blocks_resp = notion_request(
        client,
        reqwest::Method::GET,
        &format!("{API_BASE}/blocks/{page_id}/children"),
        token,
    )
    .send()
    .await
    .map_err(|e| format!("Request failed: {e}"))?;

    let mut content = String::new();
    if blocks_resp.status().is_success() {
        let blocks: Value = blocks_resp
            .json()
            .await
            .map_err(|e| format!("Parse error: {e}"))?;
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
    let database_id = args["database_id"]
        .as_str()
        .ok_or("Missing 'database_id'")?;
    let database_id = validate_resource_id(database_id, "database_id")?;

    let resp = notion_request(
        client,
        reqwest::Method::POST,
        &format!("{API_BASE}/databases/{database_id}/query"),
        token,
    )
    .json(&json!({ "page_size": 20 }))
    .send()
    .await
    .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Notion API error: {text}"));
    }

    let result: Value = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let results = result["results"].as_array();

    match results {
        Some(items) if !items.is_empty() => {
            let summaries: Vec<String> = items
                .iter()
                .map(|item| {
                    let id = item["id"].as_str().unwrap_or("");
                    let title = extract_title(item).unwrap_or_else(|| "(untitled)".to_string());
                    format!("- {title} (id: {id})")
                })
                .collect();
            Ok(format!(
                "Database has {} row(s):\n{}",
                summaries.len(),
                summaries.join("\n")
            ))
        }
        _ => Ok("Database is empty.".to_string()),
    }
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

    #[tokio::test]
    async fn handle_missing_credential() {
        let config = Config::default();
        let args = json!({"action": "search", "query": "test"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "NOTION_API_KEY not configured");
    }

    #[tokio::test]
    async fn handle_unknown_action() {
        let mut config = Config::default();
        config.credentials.insert(
            "NOTION_API_KEY".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_NOTION_API_KEY__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_NOTION_API_KEY__", "fake-token");
        }
        let args = json!({"action": "delete"});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Unknown action: delete");
    }

    #[tokio::test]
    async fn handle_missing_action_param() {
        let mut config = Config::default();
        config.credentials.insert(
            "NOTION_API_KEY".to_string(),
            crate::config::CredentialValue::EnvVar("__BORG_TEST_NOTION_API_KEY__".to_string()),
        );
        unsafe {
            std::env::set_var("__BORG_TEST_NOTION_API_KEY__", "fake-token");
        }
        let args = json!({});
        let result = handle(&args, &config).await;
        assert_eq!(result.unwrap_err(), "Missing 'action' parameter");
    }
}
