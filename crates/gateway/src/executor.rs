use anyhow::{bail, Result};
use std::path::Path;

use crate::manifest::ChannelManifest;
use tamagotchi_tools::runner::ScriptRunner;

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
        // Try JSON first: {"valid": true/false}
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(valid) = parsed.get("valid").and_then(serde_json::Value::as_bool) {
                return Ok(valid);
            }
        }
        // Fall back to bare string matching
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

    /// Run the poll script with input JSON on stdin.
    /// Returns the script's stdout (expected to be a JSON array of messages).
    pub async fn poll(&self, input_json: &str, blocked_paths: &[String]) -> Result<String> {
        let script_name = match &self.manifest.scripts.poll {
            Some(p) => p.clone(),
            None => bail!(
                "Channel '{}' has no poll script configured",
                self.manifest.name
            ),
        };
        self.run_script(&script_name, input_json, blocked_paths)
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
        let script_path = self.channel_dir.join(script_name);
        let sandbox_policy = self
            .manifest
            .sandbox_policy()
            .with_tildes_expanded()
            .with_blocked_paths_filtered(blocked_paths);

        // Collect auth env vars
        let mut extra_env = Vec::new();
        if let Some(ref secret_env) = self.manifest.auth.secret_env {
            if let Ok(val) = std::env::var(secret_env) {
                extra_env.push((secret_env.clone(), val));
            }
        }
        if let Some(ref token_env) = self.manifest.auth.token_env {
            if let Ok(val) = std::env::var(token_env) {
                extra_env.push((token_env.clone(), val));
            }
        }

        let name = format!("channel:{}/{script_name}", self.manifest.name);
        let runner = ScriptRunner {
            runtime: &self.manifest.runtime,
            script_path: &script_path,
            work_dir: self.channel_dir,
            sandbox_policy,
            timeout_ms: self.manifest.settings.timeout_ms,
            extra_env: &extra_env,
            name: &name,
        };

        let output = runner.run(input_json).await?;

        if !output.success() {
            let code = output.exit_code.unwrap_or(-1);
            if !output.stderr.is_empty() {
                bail!(
                    "Channel script '{script_name}' exited {code}: {}",
                    output.stderr
                );
            }
            bail!(
                "Channel script '{script_name}' exited {code}: {}",
                output.stdout
            );
        }

        Ok(output.stdout)
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
