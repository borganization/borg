use anyhow::Result;
use std::path::Path;
use tracing::info;

use crate::manifest::ToolManifest;
use crate::runner::ScriptRunner;

pub struct ToolExecutor<'a> {
    manifest: &'a ToolManifest,
    tool_dir: &'a Path,
}

impl<'a> ToolExecutor<'a> {
    pub fn new(manifest: &'a ToolManifest, tool_dir: &'a Path) -> Self {
        Self { manifest, tool_dir }
    }

    pub async fn execute(&self, args_json: &str) -> Result<String> {
        self.execute_with_env(args_json, &[]).await
    }

    pub async fn execute_with_env(
        &self,
        args_json: &str,
        extra_env: &[(String, String)],
    ) -> Result<String> {
        self.execute_with_blocked_paths(args_json, extra_env, &[])
            .await
    }

    pub async fn execute_with_blocked_paths(
        &self,
        args_json: &str,
        extra_env: &[(String, String)],
        blocked_paths: &[String],
    ) -> Result<String> {
        let entrypoint = self.tool_dir.join(&self.manifest.entrypoint);
        let sandbox_policy = self
            .manifest
            .sandbox_policy()
            .with_tildes_expanded()
            .with_blocked_paths_filtered(blocked_paths);

        let runner = ScriptRunner {
            runtime: &self.manifest.runtime,
            script_path: &entrypoint,
            work_dir: self.tool_dir,
            sandbox_policy,
            timeout_ms: self.manifest.timeout_ms,
            extra_env,
            name: &self.manifest.name,
        };

        let output = runner.run(args_json).await?;

        if !output.success() {
            let code = output.exit_code.unwrap_or(-1);
            info!("Tool '{}' exited with code {code}", self.manifest.name);
            if !output.stderr.is_empty() {
                return Ok(format!("Error (exit {code}): {}", output.stderr));
            }
            return Ok(format!("Error (exit {code}): {}", output.stdout));
        }

        Ok(output.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ToolManifest;

    fn bash_manifest(name: &str, entrypoint: &str) -> ToolManifest {
        toml::from_str(&format!(
            "name = \"{name}\"\ndescription = \"test\"\nruntime = \"bash\"\nentrypoint = \"{entrypoint}\"\n"
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn execute_missing_entrypoint_errors() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = bash_manifest("test", "nonexistent.sh");
        let executor = ToolExecutor::new(&manifest, dir.path());
        let result = executor.execute("{}").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn execute_bash_tool_success() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("echo.sh");
        std::fs::write(&script, "#!/bin/bash\ncat\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let manifest = bash_manifest("echo-tool", "echo.sh");
        let executor = ToolExecutor::new(&manifest, dir.path());
        let result = executor.execute(r#"{"hello":"world"}"#).await.unwrap();
        assert_eq!(result, r#"{"hello":"world"}"#);
    }

    #[tokio::test]
    async fn execute_bash_tool_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fail.sh");
        std::fs::write(
            &script,
            "#!/bin/bash\necho 'something went wrong' >&2\nexit 1\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let manifest = bash_manifest("fail-tool", "fail.sh");
        let executor = ToolExecutor::new(&manifest, dir.path());
        let result = executor.execute("{}").await.unwrap();
        assert!(result.contains("Error"));
        assert!(result.contains("something went wrong"));
    }
}
