use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub name: String,
    pub description: String,
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default = "default_entrypoint")]
    pub entrypoint: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub sandbox: SandboxSection,
    #[serde(default)]
    pub parameters: ParametersSection,
    #[serde(default)]
    pub credentials: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxSection {
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub fs_read: Vec<String>,
    #[serde(default)]
    pub fs_write: Vec<String>,
}

impl SandboxSection {
    pub fn to_policy(&self) -> borg_sandbox::policy::SandboxPolicy {
        borg_sandbox::policy::SandboxPolicy {
            network: self.network,
            fs_read: self.fs_read.clone(),
            fs_write: self.fs_write.clone(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParametersSection {
    #[serde(default = "default_param_type")]
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub properties: toml::Table,
    #[serde(default)]
    pub required: RequiredSection,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequiredSection {
    #[serde(default)]
    pub values: Vec<String>,
}

impl Default for ParametersSection {
    fn default() -> Self {
        Self {
            param_type: default_param_type(),
            properties: toml::Table::new(),
            required: RequiredSection::default(),
        }
    }
}

fn default_runtime() -> String {
    "python".to_string()
}
fn default_entrypoint() -> String {
    "main.py".to_string()
}
fn default_timeout() -> u64 {
    30000
}
fn default_param_type() -> String {
    "object".to_string()
}

impl ToolManifest {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let manifest: Self = toml::from_str(&content)?;
        Ok(manifest)
    }

    /// Convert parameters section to JSON Schema for the LLM.
    /// Applies sanitization to infer missing `type` fields and fill required
    /// child fields with permissive defaults.
    pub fn parameters_json_schema(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();

        for (key, value) in &self.parameters.properties {
            if let Some(table) = value.as_table() {
                let mut prop = serde_json::Map::new();
                if let Some(t) = table.get("type").and_then(|v| v.as_str()) {
                    prop.insert("type".to_string(), serde_json::Value::String(t.to_string()));
                }
                if let Some(d) = table.get("description").and_then(|v| v.as_str()) {
                    prop.insert(
                        "description".to_string(),
                        serde_json::Value::String(d.to_string()),
                    );
                }
                if let Some(e) = table.get("enum").and_then(|v| v.as_array()) {
                    let vals: Vec<JsonValue> = e
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| JsonValue::String(s.to_string())))
                        .collect();
                    prop.insert("enum".to_string(), JsonValue::Array(vals));
                }
                properties.insert(key.clone(), serde_json::Value::Object(prop));
            }
        }

        let mut schema = serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": self.parameters.required.values,
        });

        sanitize_json_schema(&mut schema);
        schema
    }

    pub fn sandbox_policy(&self) -> borg_sandbox::policy::SandboxPolicy {
        self.sandbox.to_policy()
    }
}

/// Sanitize a JSON Schema value to ensure LLM compatibility.
///
/// Ported from codex-rs `codex-tools`. This function:
/// - Ensures every schema object has a `type`. If missing, infers it from
///   common keywords (properties => object, items => array, enum/const/format => string).
/// - Fills required child fields (e.g. array `items`, object `properties`) with
///   permissive defaults when absent.
fn sanitize_json_schema(value: &mut JsonValue) {
    match value {
        JsonValue::Bool(_) => {
            *value = serde_json::json!({ "type": "string" });
        }
        JsonValue::Array(values) => {
            for v in values {
                sanitize_json_schema(v);
            }
        }
        JsonValue::Object(map) => {
            // Recurse into known sub-schema fields
            if let Some(properties) = map.get_mut("properties") {
                if let Some(props_map) = properties.as_object_mut() {
                    for v in props_map.values_mut() {
                        sanitize_json_schema(v);
                    }
                }
            }
            if let Some(items) = map.get_mut("items") {
                sanitize_json_schema(items);
            }
            for combiner in ["oneOf", "anyOf", "allOf"] {
                if let Some(v) = map.get_mut(combiner) {
                    sanitize_json_schema(v);
                }
            }

            // Infer missing type
            let mut schema_type = map.get("type").and_then(|v| v.as_str()).map(str::to_string);

            if schema_type.is_none() {
                if map.contains_key("properties")
                    || map.contains_key("required")
                    || map.contains_key("additionalProperties")
                {
                    schema_type = Some("object".to_string());
                } else if map.contains_key("items") {
                    schema_type = Some("array".to_string());
                // Note: JSON Schema allows non-string enums, but tool.toml enums
                // are typically strings. Defaulting to "string" is a safe heuristic.
                } else if map.contains_key("enum")
                    || map.contains_key("const")
                    || map.contains_key("format")
                {
                    schema_type = Some("string".to_string());
                } else if map.contains_key("minimum")
                    || map.contains_key("maximum")
                    || map.contains_key("multipleOf")
                {
                    schema_type = Some("number".to_string());
                }
            }

            let schema_type = schema_type.unwrap_or_else(|| "string".to_string());
            map.insert("type".to_string(), JsonValue::String(schema_type.clone()));

            // Ensure required children exist
            if schema_type == "object" && !map.contains_key("properties") {
                map.insert(
                    "properties".to_string(),
                    JsonValue::Object(serde_json::Map::new()),
                );
            }
            if schema_type == "array" && !map.contains_key("items") {
                map.insert("items".to_string(), serde_json::json!({ "type": "string" }));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FULL_TOML: &str = r#"
name = "weather"
description = "Get the weather"
runtime = "python"
entrypoint = "main.py"
timeout_ms = 15000

[sandbox]
network = true
fs_read = ["/etc/ssl"]
fs_write = []

[parameters]
type = "object"
[parameters.properties.city]
type = "string"
description = "City name"
[parameters.required]
values = ["city"]
"#;

    #[test]
    fn parse_full_manifest() {
        let manifest: ToolManifest = toml::from_str(FULL_TOML).unwrap();
        assert_eq!(manifest.name, "weather");
        assert_eq!(manifest.description, "Get the weather");
        assert_eq!(manifest.runtime, "python");
        assert_eq!(manifest.entrypoint, "main.py");
        assert_eq!(manifest.timeout_ms, 15000);
        assert!(manifest.sandbox.network);
        assert_eq!(manifest.sandbox.fs_read, vec!["/etc/ssl"]);
        assert!(manifest.sandbox.fs_write.is_empty());
        assert_eq!(manifest.parameters.required.values, vec!["city"]);
    }

    #[test]
    fn parse_minimal_manifest() {
        let toml_str = r#"
name = "hello"
description = "Says hello"
"#;
        let manifest: ToolManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "hello");
        assert_eq!(manifest.runtime, "python");
        assert_eq!(manifest.entrypoint, "main.py");
        assert_eq!(manifest.timeout_ms, 30000);
        assert!(!manifest.sandbox.network);
    }

    #[test]
    fn parameters_json_schema_output() {
        let manifest: ToolManifest = toml::from_str(FULL_TOML).unwrap();
        let schema = manifest.parameters_json_schema();

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["city"]["type"], "string");
        assert_eq!(schema["properties"]["city"]["description"], "City name");
        assert_eq!(schema["required"], serde_json::json!(["city"]));
    }

    #[test]
    fn parameters_json_schema_empty_properties() {
        let toml_str = r#"
name = "empty"
description = "No params"
"#;
        let manifest: ToolManifest = toml::from_str(toml_str).unwrap();
        let schema = manifest.parameters_json_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"], serde_json::json!({}));
    }

    #[test]
    fn sandbox_policy_conversion() {
        let manifest: ToolManifest = toml::from_str(FULL_TOML).unwrap();
        let policy = manifest.sandbox_policy();
        assert!(policy.network);
        assert_eq!(policy.fs_read, vec!["/etc/ssl"]);
        assert!(policy.fs_write.is_empty());
    }

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool.toml");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            write!(f, "{FULL_TOML}").unwrap();
        }
        let manifest = ToolManifest::load(&path).unwrap();
        assert_eq!(manifest.name, "weather");
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let result = ToolManifest::load(Path::new("/tmp/nonexistent_tool_toml_xyz.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn sanitize_infers_object_type_from_properties() {
        let mut schema = serde_json::json!({
            "properties": { "name": { "type": "string" } }
        });
        sanitize_json_schema(&mut schema);
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn sanitize_infers_array_type_from_items() {
        let mut schema = serde_json::json!({
            "items": { "type": "string" }
        });
        sanitize_json_schema(&mut schema);
        assert_eq!(schema["type"], "array");
    }

    #[test]
    fn sanitize_infers_string_type_from_enum() {
        let mut schema = serde_json::json!({
            "enum": ["a", "b"]
        });
        sanitize_json_schema(&mut schema);
        assert_eq!(schema["type"], "string");
    }

    #[test]
    fn sanitize_infers_number_type_from_minimum() {
        let mut schema = serde_json::json!({
            "minimum": 0
        });
        sanitize_json_schema(&mut schema);
        assert_eq!(schema["type"], "number");
    }

    #[test]
    fn sanitize_defaults_to_string() {
        let mut schema = serde_json::json!({});
        sanitize_json_schema(&mut schema);
        assert_eq!(schema["type"], "string");
    }

    #[test]
    fn sanitize_adds_missing_properties_to_object() {
        let mut schema = serde_json::json!({
            "type": "object"
        });
        sanitize_json_schema(&mut schema);
        assert_eq!(schema["properties"], serde_json::json!({}));
    }

    #[test]
    fn sanitize_adds_missing_items_to_array() {
        let mut schema = serde_json::json!({
            "type": "array"
        });
        sanitize_json_schema(&mut schema);
        assert_eq!(schema["items"]["type"], "string");
    }

    #[test]
    fn sanitize_preserves_existing_type() {
        let mut schema = serde_json::json!({
            "type": "boolean"
        });
        sanitize_json_schema(&mut schema);
        assert_eq!(schema["type"], "boolean");
    }

    #[test]
    fn sanitize_recurses_into_properties() {
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": { "items": { "type": "string" } }
            }
        });
        sanitize_json_schema(&mut schema);
        // tags should be inferred as array
        assert_eq!(schema["properties"]["tags"]["type"], "array");
    }

    #[test]
    fn sanitize_coerces_bool_schema() {
        let mut schema = serde_json::json!(true);
        sanitize_json_schema(&mut schema);
        assert_eq!(schema["type"], "string");
    }

    #[test]
    fn timeout_ms_zero_parses() {
        let toml_str = r#"
name = "fast"
description = "Zero timeout"
timeout_ms = 0
"#;
        let manifest: ToolManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.timeout_ms, 0);
    }

    #[test]
    fn unknown_runtime_parses() {
        let toml_str = r#"
name = "exotic"
description = "Unknown runtime"
runtime = "nonexistent_runtime"
"#;
        let manifest: ToolManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.runtime, "nonexistent_runtime");
    }

    #[test]
    fn missing_name_fails() {
        let toml_str = r#"
description = "No name field"
"#;
        let result: Result<ToolManifest, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "missing name should fail to parse");
    }

    #[test]
    fn extra_unknown_fields_ignored() {
        let toml_str = r#"
name = "flexible"
description = "Has extra fields"
foobar = 123
baz = "hello"
"#;
        // serde default: unknown fields are ignored (no deny_unknown_fields)
        let manifest: ToolManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "flexible");
    }

    #[test]
    fn duplicate_required_values_preserved() {
        let toml_str = r#"
name = "dupes"
description = "Duplicate required"

[parameters]
type = "object"
[parameters.properties.city]
type = "string"
description = "City"
[parameters.required]
values = ["city", "city"]
"#;
        let manifest: ToolManifest = toml::from_str(toml_str).unwrap();
        let schema = manifest.parameters_json_schema();
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 2, "duplicate required values are preserved");
    }

    #[test]
    fn parameters_json_schema_with_enum() {
        let toml_str = r#"
name = "color-tool"
description = "Pick a color"

[parameters]
type = "object"
[parameters.properties.color]
type = "string"
description = "The color"
enum = ["red", "green", "blue"]
[parameters.required]
values = ["color"]
"#;
        let manifest: ToolManifest = toml::from_str(toml_str).unwrap();
        let schema = manifest.parameters_json_schema();
        assert_eq!(schema["properties"]["color"]["type"], "string");
        assert_eq!(
            schema["properties"]["color"]["enum"],
            serde_json::json!(["red", "green", "blue"])
        );
    }
}
