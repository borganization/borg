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

    pub fn sandbox_policy(&self) -> borg_sandbox::policy::SandboxPolicy {
        self.sandbox.to_policy()
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
}
