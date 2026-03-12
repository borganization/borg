use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, warn};

use crate::executor::ToolExecutor;
use crate::manifest::ToolManifest;

pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
    tools_dir: PathBuf,
}

pub struct RegisteredTool {
    pub manifest: ToolManifest,
    pub dir: PathBuf,
}

impl ToolRegistry {
    pub fn new() -> Result<Self> {
        let tools_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".tamagotchi")
            .join("tools");

        let mut registry = Self {
            tools: HashMap::new(),
            tools_dir,
        };

        registry.scan()?;
        Ok(registry)
    }

    pub fn scan(&mut self) -> Result<()> {
        self.tools.clear();

        if !self.tools_dir.exists() {
            debug!(
                "Tools directory does not exist: {}",
                self.tools_dir.display()
            );
            return Ok(());
        }

        let entries = std::fs::read_dir(&self.tools_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("tool.toml");
            if !manifest_path.exists() {
                continue;
            }

            match ToolManifest::load(&manifest_path) {
                Ok(manifest) => {
                    debug!("Registered tool: {} from {}", manifest.name, path.display());
                    let name = manifest.name.clone();
                    self.tools.insert(
                        name,
                        RegisteredTool {
                            manifest,
                            dir: path,
                        },
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to load tool manifest {}: {e}",
                        manifest_path.display()
                    );
                }
            }
        }

        debug!("Loaded {} user tools", self.tools.len());
        Ok(())
    }

    pub fn list_tools(&self) -> Vec<String> {
        self.tools
            .values()
            .map(|t| format!("{}: {}", t.manifest.name, t.manifest.description))
            .collect()
    }

    pub fn tool_definitions(&self) -> Vec<tamagotchi_core_types::ToolDefinition> {
        self.tools
            .values()
            .map(|t| {
                tamagotchi_core_types::ToolDefinition::new(
                    &t.manifest.name,
                    &t.manifest.description,
                    t.manifest.parameters_json_schema(),
                )
            })
            .collect()
    }

    pub async fn execute_tool(&self, name: &str, args_json: &str) -> Result<String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {name}"))?;

        let executor = ToolExecutor::new(&tool.manifest, &tool.dir);
        executor.execute(args_json).await
    }
}

/// Minimal type aliases so tools crate doesn't depend on full core
pub mod tamagotchi_core_types {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolDefinition {
        #[serde(rename = "type")]
        pub tool_type: String,
        pub function: FunctionDefinition,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FunctionDefinition {
        pub name: String,
        pub description: String,
        pub parameters: serde_json::Value,
    }

    impl ToolDefinition {
        pub fn new(
            name: impl Into<String>,
            description: impl Into<String>,
            parameters: serde_json::Value,
        ) -> Self {
            Self {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: name.into(),
                    description: description.into(),
                    parameters,
                },
            }
        }
    }
}
