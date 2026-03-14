use serde::{Deserialize, Serialize};
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

    /// Convert parameters section to JSON Schema for the LLM
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
                properties.insert(key.clone(), serde_json::Value::Object(prop));
            }
        }

        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": self.parameters.required.values,
        })
    }

    pub fn sandbox_policy(&self) -> tamagotchi_sandbox::policy::SandboxPolicy {
        tamagotchi_sandbox::policy::SandboxPolicy {
            network: self.sandbox.network,
            fs_read: self.sandbox.fs_read.clone(),
            fs_write: self.sandbox.fs_write.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_TOML: &str = r#"
name = "weather"
description = "Get current weather"
runtime = "node"
entrypoint = "index.js"
timeout_ms = 15000

[sandbox]
network = true
fs_read = ["/etc/ssl"]
fs_write = ["/tmp/cache"]

[parameters]
type = "object"
[parameters.properties.city]
type = "string"
description = "City name"
[parameters.properties.units]
type = "string"
description = "Temperature units"
[parameters.required]
values = ["city"]
"#;

    const MINIMAL_TOML: &str = r#"
name = "hello"
description = "Say hello"
"#;

    #[test]
    fn parse_complete_tool_manifest() {
        let manifest: ToolManifest = toml::from_str(FULL_TOML).unwrap();
        assert_eq!(manifest.name, "weather");
        assert_eq!(manifest.description, "Get current weather");
        assert_eq!(manifest.runtime, "node");
        assert_eq!(manifest.entrypoint, "index.js");
        assert_eq!(manifest.timeout_ms, 15000);
        assert!(manifest.sandbox.network);
        assert_eq!(manifest.sandbox.fs_read, vec!["/etc/ssl"]);
        assert_eq!(manifest.sandbox.fs_write, vec!["/tmp/cache"]);
    }

    #[test]
    fn parse_minimal_manifest_applies_defaults() {
        let manifest: ToolManifest = toml::from_str(MINIMAL_TOML).unwrap();
        assert_eq!(manifest.name, "hello");
        assert_eq!(manifest.runtime, "python");
        assert_eq!(manifest.entrypoint, "main.py");
        assert_eq!(manifest.timeout_ms, 30000);
        assert!(!manifest.sandbox.network);
        assert!(manifest.sandbox.fs_read.is_empty());
        assert!(manifest.sandbox.fs_write.is_empty());
        assert_eq!(manifest.parameters.param_type, "object");
        assert!(manifest.parameters.required.values.is_empty());
    }

    #[test]
    fn parameters_json_schema_generation() {
        let manifest: ToolManifest = toml::from_str(FULL_TOML).unwrap();
        let schema = manifest.parameters_json_schema();

        assert_eq!(schema["type"], "object");

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("city"));
        assert!(props.contains_key("units"));
        assert_eq!(props["city"]["type"], "string");
        assert_eq!(props["city"]["description"], "City name");
        assert_eq!(props["units"]["type"], "string");

        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "city");
    }

    #[test]
    fn parameters_json_schema_empty_properties() {
        let manifest: ToolManifest = toml::from_str(MINIMAL_TOML).unwrap();
        let schema = manifest.parameters_json_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].as_object().unwrap().is_empty());
        assert!(schema["required"].as_array().unwrap().is_empty());
    }

    #[test]
    fn sandbox_policy_maps_correctly() {
        let manifest: ToolManifest = toml::from_str(FULL_TOML).unwrap();
        let policy = manifest.sandbox_policy();
        assert!(policy.network);
        assert_eq!(policy.fs_read, vec!["/etc/ssl"]);
        assert_eq!(policy.fs_write, vec!["/tmp/cache"]);
    }

    #[test]
    fn sandbox_policy_defaults() {
        let manifest: ToolManifest = toml::from_str(MINIMAL_TOML).unwrap();
        let policy = manifest.sandbox_policy();
        assert!(!policy.network);
        assert!(policy.fs_read.is_empty());
        assert!(policy.fs_write.is_empty());
    }

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool.toml");
        std::fs::write(&path, FULL_TOML).unwrap();

        let manifest = ToolManifest::load(&path).unwrap();
        assert_eq!(manifest.name, "weather");
        assert_eq!(manifest.runtime, "node");
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let result = ToolManifest::load(Path::new("/tmp/nonexistent_tool_toml_xyz.toml"));
        assert!(result.is_err());
    }
}
