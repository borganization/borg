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
