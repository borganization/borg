use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

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
            .join(".borg")
            .join("tools");

        let mut registry = Self {
            tools: HashMap::new(),
            tools_dir,
        };

        registry.scan()?;
        Ok(registry)
    }

    pub fn with_dir(dir: PathBuf) -> Result<Self> {
        let mut registry = Self {
            tools: HashMap::new(),
            tools_dir: dir,
        };

        registry.scan()?;
        Ok(registry)
    }

    pub fn scan(&mut self) -> Result<()> {
        self.tools.clear();

        let scanned = crate::scan::scan_manifest_dir(
            &self.tools_dir,
            "tool.toml",
            ToolManifest::load,
            |m| m.name.clone(),
            "tool",
        )?;

        for (name, (manifest, dir)) in scanned {
            self.tools.insert(name, RegisteredTool { manifest, dir });
        }

        Ok(())
    }

    pub fn list_tools(&self) -> Vec<String> {
        self.tools
            .values()
            .map(|t| format!("{}: {}", t.manifest.name, t.manifest.description))
            .collect()
    }

    pub fn tool_definitions(&self) -> Vec<borg_core_types::ToolDefinition> {
        self.tools
            .values()
            .map(|t| {
                borg_core_types::ToolDefinition::new(
                    &t.manifest.name,
                    &t.manifest.description,
                    t.manifest.parameters_json_schema(),
                )
            })
            .collect()
    }

    pub async fn execute_tool(&self, name: &str, args_json: &str) -> Result<String> {
        self.execute_tool_with_env(name, args_json, &[]).await
    }

    pub async fn execute_tool_with_env(
        &self,
        name: &str,
        args_json: &str,
        extra_env: &[(String, String)],
    ) -> Result<String> {
        self.execute_tool_full(name, args_json, extra_env, &[])
            .await
    }

    pub async fn execute_tool_full(
        &self,
        name: &str,
        args_json: &str,
        extra_env: &[(String, String)],
        blocked_paths: &[String],
    ) -> Result<String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {name}"))?;

        let executor = ToolExecutor::new(&tool.manifest, &tool.dir);
        executor
            .execute_with_blocked_paths(args_json, extra_env, blocked_paths)
            .await
    }

    pub async fn execute_tool_streaming<F>(
        &self,
        name: &str,
        args_json: &str,
        extra_env: &[(String, String)],
        blocked_paths: &[String],
        on_output: F,
    ) -> Result<String>
    where
        F: FnMut(&str, bool) + Send,
    {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {name}"))?;

        let executor = ToolExecutor::new(&tool.manifest, &tool.dir);
        executor
            .execute_streaming(args_json, extra_env, blocked_paths, on_output)
            .await
    }

    pub fn tool_credentials(&self, name: &str) -> Vec<String> {
        self.tools
            .get(name)
            .map(|t| t.manifest.credentials.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tool_toml(dir: &std::path::Path, name: &str, description: &str) {
        let tool_dir = dir.join(name);
        std::fs::create_dir_all(&tool_dir).unwrap();
        std::fs::write(
            tool_dir.join("tool.toml"),
            format!(
                "name = \"{name}\"\ndescription = \"{description}\"\nruntime = \"bash\"\nentrypoint = \"run.sh\"\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn scan_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        assert!(registry.list_tools().is_empty());
    }

    #[test]
    fn scan_nonexistent_dir() {
        let registry =
            ToolRegistry::with_dir(PathBuf::from("/tmp/nonexistent_tools_dir_xyz")).unwrap();
        assert!(registry.list_tools().is_empty());
    }

    #[test]
    fn scan_valid_tool() {
        let dir = tempfile::tempdir().unwrap();
        write_tool_toml(dir.path(), "hello", "Says hello");
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        assert_eq!(registry.list_tools().len(), 1);
        assert!(registry.list_tools()[0].contains("hello"));
        assert!(registry.list_tools()[0].contains("Says hello"));
    }

    #[test]
    fn scan_skips_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("not_a_dir.txt"), "just a file").unwrap();
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        assert!(registry.list_tools().is_empty());
    }

    #[test]
    fn scan_skips_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("no-manifest")).unwrap();
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        assert!(registry.list_tools().is_empty());
    }

    #[test]
    fn scan_skips_invalid_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let tool_dir = dir.path().join("bad-tool");
        std::fs::create_dir_all(&tool_dir).unwrap();
        std::fs::write(tool_dir.join("tool.toml"), "not valid toml {{{{").unwrap();
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        assert!(registry.list_tools().is_empty());
    }

    #[test]
    fn list_tools_format() {
        let dir = tempfile::tempdir().unwrap();
        write_tool_toml(dir.path(), "weather", "Get the weather");
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        let list = registry.list_tools();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], "weather: Get the weather");
    }

    #[test]
    fn tool_definitions_produces_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        write_tool_toml(dir.path(), "mytool", "A tool");
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        let defs = registry.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].tool_type, "function");
        assert_eq!(defs[0].function.name, "mytool");
        assert_eq!(defs[0].function.description, "A tool");
        assert_eq!(defs[0].function.parameters["type"], "object");
    }

    #[tokio::test]
    async fn execute_unknown_tool_errors() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        let result = registry.execute_tool("nonexistent", "{}").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown tool"));
    }

    #[test]
    fn tool_credentials_unknown_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        assert!(registry.tool_credentials("nonexistent").is_empty());
    }
}

/// Minimal type aliases so tools crate doesn't depend on full core
pub mod borg_core_types {
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
