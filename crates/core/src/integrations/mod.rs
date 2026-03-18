pub mod gmail;
pub mod google_calendar;
pub mod linear;
pub mod notion;
pub mod outlook;

use serde_json::Value;

/// Validate that an API resource ID contains only safe characters
/// (alphanumeric, hyphens, underscores, dots). Prevents path traversal
/// when IDs are interpolated into API URLs.
pub fn validate_resource_id<'a>(id: &'a str, field_name: &str) -> Result<&'a str, String> {
    if id.is_empty() {
        return Err(format!("Empty {field_name}"));
    }
    if id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        Ok(id)
    } else {
        Err(format!("Invalid {field_name}: contains illegal characters"))
    }
}

/// Collect all integration tool definitions.
#[allow(clippy::vec_init_then_push)]
pub fn enabled_tool_definitions() -> Vec<crate::types::ToolDefinition> {
    let mut tools = Vec::new();

    tools.push(gmail::tool_definition());
    tools.push(outlook::tool_definition());
    tools.push(google_calendar::tool_definition());
    tools.push(notion::tool_definition());
    tools.push(linear::tool_definition());

    tools
}

/// Dispatch a tool call to the appropriate integration handler.
/// Returns `None` if the tool name doesn't match any integration.
pub async fn dispatch_tool_call(
    tool_name: &str,
    arguments: &Value,
    config: &crate::config::Config,
) -> Option<Result<String, String>> {
    match tool_name {
        "gmail" => Some(gmail::handle(arguments, config).await),
        "outlook" => Some(outlook::handle(arguments, config).await),
        "google_calendar" => Some(google_calendar::handle(arguments, config).await),
        "notion" => Some(notion::handle(arguments, config).await),
        "linear" => Some(linear::handle(arguments, config).await),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_resource_id_accepts_valid() {
        assert!(validate_resource_id("abc-123_def.456", "test").is_ok());
        assert!(validate_resource_id("SM1234567890ABCDE", "sid").is_ok());
    }

    #[test]
    fn validate_resource_id_rejects_traversal() {
        assert!(validate_resource_id("../../admin", "id").is_err());
        assert!(validate_resource_id("foo?bar=1", "id").is_err());
        assert!(validate_resource_id("id/nested", "id").is_err());
        assert!(validate_resource_id("", "id").is_err());
    }

    #[test]
    fn validate_resource_id_allows_dots_hyphens_underscores() {
        assert_eq!(validate_resource_id("a", "id").unwrap(), "a");
        assert_eq!(validate_resource_id("x-y_z.0", "id").unwrap(), "x-y_z.0");
        assert_eq!(
            validate_resource_id("a".repeat(200).as_str(), "id").unwrap(),
            "a".repeat(200)
        );
        assert_eq!(validate_resource_id(".", "id").unwrap(), ".");
        assert_eq!(validate_resource_id("_", "id").unwrap(), "_");
        assert_eq!(validate_resource_id("-", "id").unwrap(), "-");
    }

    #[test]
    fn enabled_tool_definitions_all_valid() {
        let defs = enabled_tool_definitions();
        let mut names = std::collections::HashSet::new();
        for def in &defs {
            assert!(!def.function.name.is_empty(), "tool name must not be empty");
            assert!(
                !def.function.description.is_empty(),
                "tool description must not be empty for {}",
                def.function.name
            );
            assert!(
                names.insert(&def.function.name),
                "duplicate tool name: {}",
                def.function.name
            );
        }
    }

    #[tokio::test]
    async fn dispatch_unknown_tool_returns_none() {
        let config = crate::config::Config::default();
        let args = serde_json::json!({});
        assert!(dispatch_tool_call("nonexistent", &args, &config)
            .await
            .is_none());
        assert!(dispatch_tool_call("", &args, &config).await.is_none());
        assert!(dispatch_tool_call("gmail_extended", &args, &config)
            .await
            .is_none());
    }
}
