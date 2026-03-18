use anyhow::{bail, Context, Result};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::debug;

use borg_sandbox::policy::SandboxPolicy;

/// Output from a script execution.
pub struct ScriptOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

/// Shared subprocess runner for tools and channel scripts.
pub struct ScriptRunner<'a> {
    pub runtime: &'a str,
    pub script_path: &'a Path,
    pub work_dir: &'a Path,
    pub sandbox_policy: SandboxPolicy,
    pub timeout_ms: u64,
    pub extra_env: &'a [(String, String)],
    pub name: &'a str,
}

impl<'a> ScriptRunner<'a> {
    pub async fn run(&self, input_json: &str) -> Result<ScriptOutput> {
        let (program, base_args) = resolve_runtime(self.runtime, self.work_dir)?;

        if !self.script_path.exists() {
            bail!("Script not found: {}", self.script_path.display());
        }

        let mut cmd_args = base_args;
        cmd_args.push(self.script_path.to_string_lossy().to_string());

        let sandboxed = self
            .sandbox_policy
            .wrap_command(&program, &cmd_args, self.work_dir);

        debug!(
            "Executing '{}': {} {:?}",
            self.name, sandboxed.program, sandboxed.args
        );

        let timeout = std::time::Duration::from_millis(self.timeout_ms);

        let mut cmd = Command::new(&sandboxed.program);
        cmd.args(&sandboxed.args)
            .current_dir(self.work_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (key, val) in self.extra_env {
            cmd.env(key, val);
        }

        let mut child = cmd.kill_on_drop(true).spawn().with_context(|| {
            format!(
                "Failed to spawn '{}' (runtime: {})",
                self.name, self.runtime
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = stdin.write_all(input_json.as_bytes()).await {
                tracing::warn!("Failed to write input to '{}': {e}", self.name);
            }
            drop(stdin);
        }

        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| anyhow::anyhow!("'{}' timed out after {}ms", self.name, self.timeout_ms))?
            .context("Failed to wait for script process")?;

        Ok(ScriptOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
        })
    }

    /// Like `run()`, but invokes `on_output` for each line of stdout/stderr as it arrives.
    pub async fn run_streaming<F>(&self, input_json: &str, mut on_output: F) -> Result<ScriptOutput>
    where
        F: FnMut(&str, bool) + Send,
    {
        let (program, base_args) = resolve_runtime(self.runtime, self.work_dir)?;

        if !self.script_path.exists() {
            bail!("Script not found: {}", self.script_path.display());
        }

        let mut cmd_args = base_args;
        cmd_args.push(self.script_path.to_string_lossy().to_string());

        let sandboxed = self
            .sandbox_policy
            .wrap_command(&program, &cmd_args, self.work_dir);

        debug!(
            "Executing streaming '{}': {} {:?}",
            self.name, sandboxed.program, sandboxed.args
        );

        let timeout = std::time::Duration::from_millis(self.timeout_ms);

        let mut cmd = Command::new(&sandboxed.program);
        cmd.args(&sandboxed.args)
            .current_dir(self.work_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (key, val) in self.extra_env {
            cmd.env(key, val);
        }

        let mut child = cmd.kill_on_drop(true).spawn().with_context(|| {
            format!(
                "Failed to spawn '{}' (runtime: {})",
                self.name, self.runtime
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = stdin.write_all(input_json.as_bytes()).await {
                tracing::warn!("Failed to write input to '{}': {e}", self.name);
            }
            drop(stdin);
        }

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();
        let mut callback_bytes: usize = 0;
        const STREAM_CALLBACK_LIMIT: usize = 120 * 1024;

        let streaming_future = async {
            let mut stdout_lines = stdout_pipe.map(|p| BufReader::new(p).lines());
            let mut stderr_lines = stderr_pipe.map(|p| BufReader::new(p).lines());
            let mut stdout_done = stdout_lines.is_none();
            let mut stderr_done = stderr_lines.is_none();

            while !stdout_done || !stderr_done {
                tokio::select! {
                    line = async {
                        match stdout_lines.as_mut() {
                            Some(reader) => reader.next_line().await,
                            None => std::future::pending().await,
                        }
                    }, if !stdout_done => {
                        match line {
                            Ok(Some(l)) => {
                                if callback_bytes < STREAM_CALLBACK_LIMIT {
                                    on_output(&l, false);
                                    callback_bytes += l.len();
                                }
                                stdout_buf.push_str(&l);
                                stdout_buf.push('\n');
                            }
                            _ => { stdout_done = true; }
                        }
                    }
                    line = async {
                        match stderr_lines.as_mut() {
                            Some(reader) => reader.next_line().await,
                            None => std::future::pending().await,
                        }
                    }, if !stderr_done => {
                        match line {
                            Ok(Some(l)) => {
                                if callback_bytes < STREAM_CALLBACK_LIMIT {
                                    on_output(&l, true);
                                    callback_bytes += l.len();
                                }
                                stderr_buf.push_str(&l);
                                stderr_buf.push('\n');
                            }
                            _ => { stderr_done = true; }
                        }
                    }
                }
            }

            child.wait().await
        };

        let status = tokio::time::timeout(timeout, streaming_future)
            .await
            .map_err(|_| anyhow::anyhow!("'{}' timed out after {}ms", self.name, self.timeout_ms))?
            .context("Failed to wait for script process")?;

        Ok(ScriptOutput {
            stdout: stdout_buf,
            stderr: stderr_buf,
            exit_code: status.code(),
        })
    }
}

impl ScriptOutput {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// Resolve a runtime string to a (program, base_args) pair.
pub fn resolve_runtime(runtime: &str, work_dir: &Path) -> Result<(String, Vec<String>)> {
    match runtime {
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
            let deno =
                which::which("deno").context("Deno not found. Install deno to use Deno tools.")?;
            let allow_read = format!("--allow-read={}", work_dir.display());
            Ok((
                deno.to_string_lossy().to_string(),
                vec!["run".to_string(), allow_read],
            ))
        }
        "bash" => Ok(("bash".to_string(), vec![])),
        other => bail!("Unsupported runtime: {other}"),
    }
}
