use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::executor::ToolExecutor;
use crate::manifest::ToolManifest;

// --- Generic ManifestRegistry ---

/// Trait for manifest types that can be used with `ManifestRegistry`.
pub trait ManifestItem: Sized {
    fn load(path: &Path) -> Result<Self>;
    fn item_name(&self) -> &str;
    const MANIFEST_FILENAME: &'static str;
    const SUBDIR: &'static str;
    const ITEM_TYPE: &'static str;
}

#[derive(Clone)]
pub struct RegisteredItem<M: Clone> {
    pub manifest: M,
    pub dir: PathBuf,
}

pub struct ManifestRegistry<M: Clone> {
    items: HashMap<String, RegisteredItem<M>>,
    base_dir: PathBuf,
}

impl<M: ManifestItem + Clone> ManifestRegistry<M> {
    pub fn new() -> Result<Self> {
        let base_dir = std::env::var("BORG_DATA_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))
                    .map(|h| h.join(".borg"))
            })?
            .join(M::SUBDIR);

        let mut registry = Self {
            items: HashMap::new(),
            base_dir,
        };

        registry.scan()?;
        Ok(registry)
    }

    pub fn with_dir(dir: PathBuf) -> Result<Self> {
        let mut registry = Self {
            items: HashMap::new(),
            base_dir: dir,
        };

        registry.scan()?;
        Ok(registry)
    }

    pub fn scan(&mut self) -> Result<()> {
        self.items.clear();

        let scanned = crate::scan::scan_manifest_dir(
            &self.base_dir,
            M::MANIFEST_FILENAME,
            M::load,
            |m| m.item_name().to_string(),
            M::ITEM_TYPE,
        )?;

        for (name, (manifest, dir)) in scanned {
            self.items.insert(name, RegisteredItem { manifest, dir });
        }

        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&RegisteredItem<M>> {
        self.items.get(name)
    }

    pub fn items(&self) -> impl Iterator<Item = &RegisteredItem<M>> {
        self.items.values()
    }

    /// Find the best fuzzy match for a query string among registered item names.
    /// Returns `None` if no match exceeds the similarity threshold (0.6).
    pub fn fuzzy_find(&self, query: &str) -> Option<&RegisteredItem<M>> {
        fuzzy_best_match(query, self.items.iter().map(|(k, v)| (k.as_str(), v)))
    }
}

/// Minimum Jaro-Winkler similarity score to consider a match valid.
const FUZZY_THRESHOLD: f64 = 0.6;

/// Find the best fuzzy match for `query` among `(name, value)` pairs.
/// Returns `None` if no match meets `FUZZY_THRESHOLD`.
pub fn fuzzy_best_match<'a, T>(
    query: &str,
    candidates: impl Iterator<Item = (&'a str, &'a T)>,
) -> Option<&'a T> {
    let query_lower = query.to_lowercase();
    candidates
        .map(|(name, item)| {
            let score = strsim::jaro_winkler(&query_lower, &name.to_lowercase());
            (score, item)
        })
        .filter(|(score, _)| *score >= FUZZY_THRESHOLD)
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, item)| item)
}

// --- ManifestItem impl for ToolManifest ---

impl ManifestItem for ToolManifest {
    fn load(path: &Path) -> Result<Self> {
        ToolManifest::load(path)
    }
    fn item_name(&self) -> &str {
        &self.name
    }
    const MANIFEST_FILENAME: &'static str = "tool.toml";
    const SUBDIR: &'static str = "tools";
    const ITEM_TYPE: &'static str = "tool";
}

// --- ToolRegistry (wraps ManifestRegistry<ToolManifest>) ---

pub struct ToolRegistry {
    inner: ManifestRegistry<ToolManifest>,
}

/// Legacy type alias for backward compatibility.
pub type RegisteredTool = RegisteredItem<ToolManifest>;

impl ToolRegistry {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: ManifestRegistry::new()?,
        })
    }

    pub fn with_dir(dir: PathBuf) -> Result<Self> {
        Ok(Self {
            inner: ManifestRegistry::with_dir(dir)?,
        })
    }

    pub fn scan(&mut self) -> Result<()> {
        self.inner.scan()
    }

    pub fn list_tools(&self) -> Vec<String> {
        self.inner
            .items()
            .map(|t| format!("{}: {}", t.manifest.name, t.manifest.description))
            .collect()
    }

    pub fn tool_definitions(&self) -> Vec<borg_core_types::ToolDefinition> {
        self.inner
            .items()
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
            .inner
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
            .inner
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {name}"))?;

        let executor = ToolExecutor::new(&tool.manifest, &tool.dir);
        executor
            .execute_streaming(args_json, extra_env, blocked_paths, on_output)
            .await
    }

    pub fn tool_credentials(&self, name: &str) -> Vec<String> {
        self.inner
            .get(name)
            .map(|t| t.manifest.credentials.clone())
            .unwrap_or_default()
    }

    /// Find a tool by fuzzy name match. Returns `None` if no good match found.
    pub fn fuzzy_find(&self, query: &str) -> Option<&RegisteredTool> {
        self.inner.fuzzy_find(query)
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

    #[test]
    fn fuzzy_find_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        write_tool_toml(dir.path(), "weather", "Get the weather");
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        let found = registry.fuzzy_find("weather");
        assert!(found.is_some());
        assert_eq!(found.unwrap().manifest.name, "weather");
    }

    #[test]
    fn fuzzy_find_close_typo() {
        let dir = tempfile::tempdir().unwrap();
        write_tool_toml(dir.path(), "weather", "Get the weather");
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        let found = registry.fuzzy_find("weathr");
        assert!(found.is_some());
        assert_eq!(found.unwrap().manifest.name, "weather");
    }

    #[test]
    fn fuzzy_find_no_match() {
        let dir = tempfile::tempdir().unwrap();
        write_tool_toml(dir.path(), "weather", "Get the weather");
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        let found = registry.fuzzy_find("zzzzzzz");
        assert!(found.is_none());
    }

    #[test]
    fn fuzzy_find_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        assert!(registry.fuzzy_find("anything").is_none());
    }

    #[test]
    fn fuzzy_find_best_of_multiple() {
        let dir = tempfile::tempdir().unwrap();
        write_tool_toml(dir.path(), "weather", "Get the weather");
        write_tool_toml(dir.path(), "web-search", "Search the web");
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        let found = registry.fuzzy_find("weathr");
        assert!(found.is_some());
        assert_eq!(found.unwrap().manifest.name, "weather");
    }

    #[test]
    fn fuzzy_find_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        write_tool_toml(dir.path(), "Weather", "Get the weather");
        let registry = ToolRegistry::with_dir(dir.path().to_path_buf()).unwrap();
        let found = registry.fuzzy_find("weather");
        assert!(found.is_some());
    }

    #[test]
    fn fuzzy_best_match_standalone() {
        let items = vec![("alpha", &1), ("beta", &2), ("gamma", &3)];
        let result = fuzzy_best_match("alph", items.into_iter());
        assert_eq!(result, Some(&1));
    }

    #[test]
    fn fuzzy_best_match_empty_query() {
        let items = vec![("alpha", &1)];
        let result = fuzzy_best_match("", items.into_iter());
        // Empty query has low similarity to any name
        assert!(result.is_none());
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
