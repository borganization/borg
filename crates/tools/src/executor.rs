use anyhow::{bail, Context, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::{debug, info};

use crate::manifest::ToolManifest;

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
        let (program, base_args) = self.resolve_runtime()?;
        let entrypoint = self.tool_dir.join(&self.manifest.entrypoint);

        if !entrypoint.exists() {
            bail!("Tool entrypoint not found: {}", entrypoint.display());
        }

        let mut cmd_args = base_args;
        cmd_args.push(entrypoint.to_string_lossy().to_string());

        // Apply sandbox wrapping with blocklist filtering
        let sandbox_policy = self
            .manifest
            .sandbox_policy()
            .with_tildes_expanded()
            .with_blocked_paths_filtered(blocked_paths);
        let sandboxed = sandbox_policy.wrap_command(&program, &cmd_args, self.tool_dir);

        debug!(
            "Executing tool '{}': {} {:?}",
            self.manifest.name, sandboxed.program, sandboxed.args
        );

        let timeout = std::time::Duration::from_millis(self.manifest.timeout_ms);

        let mut cmd = Command::new(&sandboxed.program);
        cmd.args(&sandboxed.args)
            .current_dir(self.tool_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (key, val) in extra_env {
            cmd.env(key, val);
        }
        let child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn tool '{}' (runtime: {})",
                self.manifest.name, self.manifest.runtime
            )
        })?;

        // Write args to stdin
        let mut child = child;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(args_json.as_bytes()).await;
            drop(stdin);
        }

        // Wait with timeout
        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Tool '{}' timed out after {}ms",
                    self.manifest.name,
                    self.manifest.timeout_ms
                )
            })?
            .context("Failed to wait for tool process")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            info!("Tool '{}' exited with code {code}", self.manifest.name);
            if !stderr.is_empty() {
                return Ok(format!("Error (exit {code}): {stderr}"));
            }
            return Ok(format!("Error (exit {code}): {stdout}"));
        }

        Ok(stdout)
    }

    fn resolve_runtime(&self) -> Result<(String, Vec<String>)> {
        match self.manifest.runtime.as_str() {
            "python" => {
                let python = which::which("python3")
                    .or_else(|_| which::which("python"))
                    .context("Python not found. Install python3 to use Python tools.")?;
                Ok((python.to_string_lossy().to_string(), vec![]))
            }
            "node" => {
                let node = which::which("node")
                    .context("Node.js not found. Install node to use Node tools.")?;
                Ok((node.to_string_lossy().to_string(), vec![]))
            }
            "deno" => {
                let deno = which::which("deno")
                    .context("Deno not found. Install deno to use Deno tools.")?;
                Ok((
                    deno.to_string_lossy().to_string(),
                    vec!["run".to_string(), "--allow-read".to_string()],
                ))
            }
            "bash" => Ok(("bash".to_string(), vec![])),
            other => bail!("Unsupported runtime: {other}"),
        }
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
