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
        let (program, base_args) = self.resolve_runtime()?;
        let entrypoint = self.tool_dir.join(&self.manifest.entrypoint);

        if !entrypoint.exists() {
            bail!("Tool entrypoint not found: {}", entrypoint.display());
        }

        let mut cmd_args = base_args;
        cmd_args.push(entrypoint.to_string_lossy().to_string());

        // Apply sandbox wrapping
        let sandbox_policy = self.manifest.sandbox_policy();
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
