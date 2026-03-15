use anyhow::{bail, Context, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::debug;

use crate::manifest::ChannelManifest;

pub struct ChannelExecutor<'a> {
    manifest: &'a ChannelManifest,
    channel_dir: &'a Path,
}

impl<'a> ChannelExecutor<'a> {
    pub fn new(manifest: &'a ChannelManifest, channel_dir: &'a Path) -> Self {
        Self {
            manifest,
            channel_dir,
        }
    }

    /// Run the verify script with {headers, body, secret} JSON on stdin.
    /// Returns Ok(true) if verified, Ok(false) if rejected.
    pub async fn verify(&self, input_json: &str, blocked_paths: &[String]) -> Result<bool> {
        let script_name = match &self.manifest.scripts.verify {
            Some(v) => v,
            None => return Ok(true), // No verify script = pass-through
        };

        let output = self
            .run_script(script_name, input_json, blocked_paths)
            .await?;
        let trimmed = output.trim();
        Ok(trimmed == "true" || trimmed == "ok" || trimmed == "1")
    }

    /// Run the inbound script with {headers, body} JSON on stdin.
    /// Returns the normalized message JSON from stdout.
    pub async fn parse_inbound(
        &self,
        input_json: &str,
        blocked_paths: &[String],
    ) -> Result<String> {
        self.run_script(&self.manifest.scripts.inbound, input_json, blocked_paths)
            .await
    }

    /// Run the outbound script with {text, sender_id, channel_id, token} JSON on stdin.
    /// Returns the script's stdout.
    pub async fn send_outbound(
        &self,
        input_json: &str,
        blocked_paths: &[String],
    ) -> Result<String> {
        self.run_script(&self.manifest.scripts.outbound, input_json, blocked_paths)
            .await
    }

    async fn run_script(
        &self,
        script_name: &str,
        input_json: &str,
        blocked_paths: &[String],
    ) -> Result<String> {
        let (program, base_args) = self.resolve_runtime()?;
        let script_path = self.channel_dir.join(script_name);

        if !script_path.exists() {
            bail!("Channel script not found: {}", script_path.display());
        }

        let mut cmd_args = base_args;
        cmd_args.push(script_path.to_string_lossy().to_string());

        let sandbox_policy = self
            .manifest
            .sandbox_policy()
            .with_blocked_paths_filtered(blocked_paths);
        let sandboxed = sandbox_policy.wrap_command(&program, &cmd_args, self.channel_dir);

        debug!(
            "Executing channel script '{}' for '{}': {} {:?}",
            script_name, self.manifest.name, sandboxed.program, sandboxed.args
        );

        let timeout = std::time::Duration::from_millis(self.manifest.settings.timeout_ms);

        let mut cmd = Command::new(&sandboxed.program);
        cmd.args(&sandboxed.args)
            .current_dir(self.channel_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Pass auth env vars if configured
        if let Some(ref secret_env) = self.manifest.auth.secret_env {
            if let Ok(val) = std::env::var(secret_env) {
                cmd.env(secret_env, val);
            }
        }
        if let Some(ref token_env) = self.manifest.auth.token_env {
            if let Ok(val) = std::env::var(token_env) {
                cmd.env(token_env, val);
            }
        }

        let mut child = cmd.kill_on_drop(true).spawn().with_context(|| {
            format!(
                "Failed to spawn channel script '{}' for '{}'",
                script_name, self.manifest.name
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(input_json.as_bytes())
                .await
                .with_context(|| {
                    format!("Failed to write stdin to channel script '{script_name}'")
                })?;
            drop(stdin);
        }

        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| {
                // child is killed on drop via kill_on_drop(true)
                anyhow::anyhow!(
                    "Channel script '{script_name}' for '{}' timed out after {}ms",
                    self.manifest.name,
                    self.manifest.settings.timeout_ms
                )
            })?
            .context("Failed to wait for channel script")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            if !stderr.is_empty() {
                bail!("Channel script '{script_name}' exited {code}: {stderr}");
            }
            bail!("Channel script '{script_name}' exited {code}: {stdout}");
        }

        Ok(stdout)
    }

    fn resolve_runtime(&self) -> Result<(String, Vec<String>)> {
        match self.manifest.runtime.as_str() {
            "python" => {
                let python = which::which("python3")
                    .or_else(|_| which::which("python"))
                    .context("Python not found")?;
                Ok((python.to_string_lossy().to_string(), vec![]))
            }
            "node" => {
                let node = which::which("node").context("Node.js not found")?;
                Ok((node.to_string_lossy().to_string(), vec![]))
            }
            "deno" => {
                let deno = which::which("deno").context("Deno not found")?;
                let allow_read = format!("--allow-read={}", self.channel_dir.display());
                Ok((
                    deno.to_string_lossy().to_string(),
                    vec!["run".to_string(), allow_read],
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
    use crate::manifest::ChannelManifest;

    fn bash_channel_manifest(name: &str, inbound: &str) -> ChannelManifest {
        toml::from_str(&format!(
            "name = \"{name}\"\ndescription = \"test\"\nruntime = \"bash\"\n\n[scripts]\ninbound = \"{inbound}\"\noutbound = \"out.sh\"\n"
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn run_bash_script_success() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("echo.sh");
        std::fs::write(&script, "#!/bin/bash\ncat\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let manifest = bash_channel_manifest("test-chan", "echo.sh");
        let executor = ChannelExecutor::new(&manifest, dir.path());
        let result = executor
            .parse_inbound(r#"{"text":"hi"}"#, &[])
            .await
            .unwrap();
        assert_eq!(result, r#"{"text":"hi"}"#);
    }

    #[tokio::test]
    async fn run_nonexistent_script_errors() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = bash_channel_manifest("test-chan", "missing.sh");
        let executor = ChannelExecutor::new(&manifest, dir.path());
        let result = executor.parse_inbound("{}", &[]).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
}
