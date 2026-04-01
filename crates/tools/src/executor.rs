use anyhow::Result;
use std::path::Path;
use tracing::info;

use crate::manifest::ToolManifest;
use crate::runner::{run_sandboxed_script, validate_script_path, ScriptRunner};

pub struct ToolExecutor<'a> {
    manifest: &'a ToolManifest,
    tool_dir: &'a Path,
}

impl<'a> ToolExecutor<'a> {
    pub fn new(manifest: &'a ToolManifest, tool_dir: &'a Path) -> Self {
        Self { manifest, tool_dir }
    }

    /// Resolve and validate that the entrypoint stays within tool_dir.
    fn validated_entrypoint(&self) -> Result<std::path::PathBuf> {
        validate_script_path(self.tool_dir, &self.manifest.entrypoint)
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
        let entrypoint = self.validated_entrypoint()?;
        let sandbox_policy = self.manifest.sandbox_policy().configured(blocked_paths);

        let (ok, text) = run_sandboxed_script(
            &self.manifest.runtime,
            &entrypoint,
            self.tool_dir,
            sandbox_policy,
            self.manifest.timeout_ms,
            extra_env,
            &self.manifest.name,
            args_json,
        )
        .await?;

        if !ok {
            info!("Tool '{}' failed", self.manifest.name);
        }
        Ok(text)
    }

    pub async fn execute_streaming<F>(
        &self,
        args_json: &str,
        extra_env: &[(String, String)],
        blocked_paths: &[String],
        on_output: F,
    ) -> Result<String>
    where
        F: FnMut(&str, bool) + Send,
    {
        let entrypoint = self.validated_entrypoint()?;
        let sandbox_policy = self.manifest.sandbox_policy().configured(blocked_paths);

        let runner = ScriptRunner {
            runtime: &self.manifest.runtime,
            script_path: &entrypoint,
            work_dir: self.tool_dir,
            sandbox_policy,
            timeout_ms: self.manifest.timeout_ms,
            extra_env,
            name: &self.manifest.name,
        };

        let output = runner.run_streaming(args_json, on_output).await?;
        let (ok, text) = output.into_result_string();
        if !ok {
            info!("Tool '{}' failed", self.manifest.name);
        }
        Ok(text)
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
    async fn execute_with_env_passes_vars() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("env_echo.sh");
        std::fs::write(&script, "#!/bin/bash\necho \"$MY_VAR\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let manifest = bash_manifest("env-tool", "env_echo.sh");
        let executor = ToolExecutor::new(&manifest, dir.path());
        let env = vec![("MY_VAR".to_string(), "hello_from_env".to_string())];
        let result = executor.execute_with_env("{}", &env).await.unwrap();
        assert_eq!(result.trim(), "hello_from_env");
    }

    #[tokio::test]
    async fn execute_bash_tool_stderr_only() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("stderr_only.sh");
        std::fs::write(&script, "#!/bin/bash\necho 'oops' >&2\nexit 2\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let manifest = bash_manifest("stderr-tool", "stderr_only.sh");
        let executor = ToolExecutor::new(&manifest, dir.path());
        let result = executor.execute("{}").await.unwrap();
        assert!(result.contains("Error"));
        assert!(result.contains("oops"));
    }

    #[tokio::test]
    async fn execute_path_traversal_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = bash_manifest("evil", "../../../etc/passwd");
        let executor = ToolExecutor::new(&manifest, dir.path());
        let result = executor.execute("{}").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_with_blocked_paths_runs() {
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
        let blocked = vec![".ssh".to_string(), ".aws".to_string()];
        let result = executor
            .execute_with_blocked_paths(r#"{"key":"val"}"#, &[], &blocked)
            .await
            .unwrap();
        assert_eq!(result, r#"{"key":"val"}"#);
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
