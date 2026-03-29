use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine as _;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::agent::AgentEvent;
use crate::browser::{validate_browser_args, BrowserSession};
use crate::config::Config;
use crate::db::Database;
use crate::memory::{read_memory, write_memory_scoped, WriteMode};
use crate::policy::ExecutionPolicy;
use crate::skills::{load_all_skills, Skill};
use crate::tasks;
use crate::types::{ContentPart, MediaData, ToolDefinition, ToolOutput};
use crate::web;
use borg_apply_patch::apply_patch_to_dir;
use borg_tools::registry::ToolRegistry;

pub fn require_str_param<'a>(args: &'a serde_json::Value, name: &str) -> Result<&'a str> {
    args[name]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter '{name}'."))
}

pub fn handle_write_memory(args: &serde_json::Value) -> Result<String> {
    let filename = require_str_param(args, "filename")?;
    let content = require_str_param(args, "content")?;
    let mode = if args["append"].as_bool().unwrap_or(false) {
        WriteMode::Append
    } else {
        WriteMode::Overwrite
    };
    let scope = args["scope"].as_str().unwrap_or("global");
    write_memory_scoped(filename, content, mode, scope)
}

pub fn handle_read_memory(args: &serde_json::Value) -> Result<String> {
    let filename = require_str_param(args, "filename")?;
    read_memory(filename)
}

pub fn handle_list_tools(registry: &ToolRegistry, config: &Config) -> Result<String> {
    use crate::tool_catalog::{ToolProfile, ALL_GROUPS};

    let profile =
        ToolProfile::from_str_opt(&config.tools.policy.profile).unwrap_or(ToolProfile::Full);
    let active_groups = profile.groups();
    let mut out = format!("# Tools (profile: {profile:?})\n\n## Built-in Tools\n");

    for group in ALL_GROUPS {
        let active = if active_groups.contains(group) {
            ""
        } else {
            " (disabled)"
        };
        out.push_str(&format!("\n### {}{}\n", group.label(), active));
        for name in group.tool_names() {
            out.push_str(&format!("  - {name}\n"));
        }
    }

    let tool_list = registry.list_tools();
    out.push_str("\n## User Tools\n");
    if tool_list.is_empty() {
        out.push_str("  No user tools installed.\n");
    } else {
        for tool in &tool_list {
            out.push_str(&format!("  - {tool}\n"));
        }
    }
    Ok(out)
}

pub fn handle_list_skills(config: &Config) -> Result<String> {
    let resolved_creds = config.resolve_credentials();
    let skills = load_all_skills(&resolved_creds, &config.skills)?;
    if skills.is_empty() {
        Ok("No skills installed.".to_string())
    } else {
        Ok(skills
            .iter()
            .map(Skill::summary_line)
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Apply a patch to a directory, returning a formatted result message.
fn apply_patch_to(
    args: &serde_json::Value,
    base_dir: &std::path::Path,
    label: &str,
) -> Result<String> {
    let patch = require_str_param(args, "patch")?;
    std::fs::create_dir_all(base_dir)?;
    match apply_patch_to_dir(patch, base_dir) {
        Ok(affected) => Ok(format!(
            "{label} patch applied successfully.\n{}",
            affected.format_summary()
        )),
        Err(e) => Ok(format!("Error applying {label} patch: {e}")),
    }
}

pub fn handle_apply_skill_patch(args: &serde_json::Value) -> Result<String> {
    apply_patch_to(args, &Config::skills_dir()?, "Skill")
}

/// Unified apply_patch handler with `target` parameter.
/// Supports: cwd (default), tools, skills, channels.
pub fn handle_apply_patch_unified(
    args: &serde_json::Value,
    registry: &mut ToolRegistry,
) -> Result<String> {
    // Validate patch param exists before dispatching
    let _patch = require_str_param(args, "patch")?;
    let target = args["target"].as_str().unwrap_or("cwd");

    match target {
        "cwd" => handle_apply_patch(args),
        "tools" => handle_create_tool(args, registry),
        "skills" => handle_apply_skill_patch(args),
        "channels" => handle_create_channel(args),
        other => Ok(format!(
            "Unknown target: {other}. Use: cwd, tools, skills, channels."
        )),
    }
}

pub fn handle_apply_patch(args: &serde_json::Value) -> Result<String> {
    let patch = require_str_param(args, "patch")?;
    let base_dir =
        std::env::current_dir().context("Failed to determine current working directory")?;
    match apply_patch_to_dir(patch, &base_dir) {
        Ok(affected) => Ok(format!(
            "Patch applied successfully.\n{}",
            affected.format_summary()
        )),
        Err(e) => Ok(format!("Error applying patch: {e}")),
    }
}

pub fn handle_create_tool(args: &serde_json::Value, registry: &mut ToolRegistry) -> Result<String> {
    let patch = require_str_param(args, "patch")?;
    let base_dir = Config::tools_dir()?;
    std::fs::create_dir_all(&base_dir)?;
    match apply_patch_to_dir(patch, &base_dir) {
        Ok(affected) => {
            *registry = ToolRegistry::new()?;
            Ok(format!(
                "Patch applied successfully. Tool registry reloaded.\n{}",
                affected.format_summary()
            ))
        }
        Err(e) => Ok(format!("Error applying patch: {e}")),
    }
}

pub fn handle_create_channel(args: &serde_json::Value) -> Result<String> {
    apply_patch_to(args, &Config::channels_dir()?, "Channel")
}

/// Unified list handler: dispatches based on `what` parameter.
pub fn handle_list(
    args: &serde_json::Value,
    registry: &ToolRegistry,
    config: &Config,
    agent_control: Option<&crate::multi_agent::AgentControl>,
) -> Result<String> {
    let what = require_str_param(args, "what")?;
    match what {
        "tools" => handle_list_tools(registry, config),
        "skills" => handle_list_skills(config),
        "channels" => handle_list_channels(config),
        "agents" => {
            if let Some(ctrl) = agent_control {
                crate::multi_agent::tools::handle_list_agents(ctrl)
            } else {
                Ok("Multi-agent system is not enabled.".to_string())
            }
        }
        other => Ok(format!(
            "Unknown list target: {other}. Use: tools, skills, channels, agents."
        )),
    }
}

pub fn handle_list_channels(config: &Config) -> Result<String> {
    let mut channels = Vec::new();

    // Script-based channels from ~/.borg/channels/
    if let Ok(channels_dir) = Config::channels_dir() {
        if channels_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&channels_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let manifest_path = path.join("channel.toml");
                        if manifest_path.exists() {
                            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                                if let Ok(manifest) = toml::from_str::<toml::Value>(&content) {
                                    let name = manifest
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("?");
                                    let desc = manifest
                                        .get("description")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    channels.push(format!("{name}: {desc}"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Native channels detected via credentials
    for (name, desc) in config.detected_native_channels() {
        let prefix = format!("{name}:");
        if !channels.iter().any(|c| c.starts_with(&prefix)) {
            channels.push(format!("{name}: {desc}"));
        }
    }

    Ok(if channels.is_empty() {
        "No channels installed.".to_string()
    } else {
        channels.join("\n")
    })
}

/// Read stdout/stderr from a child process line-by-line, emitting `ToolOutputDelta` events.
async fn stream_child_output(
    child: &mut tokio::process::Child,
    timeout: Duration,
    event_tx: &mpsc::Sender<AgentEvent>,
    tool_name: &str,
) -> Result<(String, String, Option<i32>)> {
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();

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
                            let _ = event_tx.try_send(AgentEvent::ToolOutputDelta {
                                name: tool_name.to_string(),
                                delta: l.clone(),
                                is_stderr: false,
                            });
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
                            let _ = event_tx.try_send(AgentEvent::ToolOutputDelta {
                                name: tool_name.to_string(),
                                delta: l.clone(),
                                is_stderr: true,
                            });
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
        .map_err(|_| anyhow::anyhow!("command timed out after {}ms", timeout.as_millis()))?
        .context("Failed to wait for shell process")?;

    Ok((stdout_buf, stderr_buf, status.code()))
}

pub async fn handle_run_shell(
    args: &serde_json::Value,
    config: &Config,
    policy: &ExecutionPolicy,
    event_tx: &mpsc::Sender<AgentEvent>,
    skill_env_allowlist: Option<&std::collections::HashSet<String>>,
) -> Result<String> {
    let command = args["command"].as_str().context("Missing 'command'")?;
    let timeout_ms = config.tools.default_timeout_ms;
    let timeout_dur = Duration::from_millis(timeout_ms);

    match policy.check(command) {
        crate::policy::PolicyDecision::Deny => {
            return Ok("Shell command denied by policy.".to_string());
        }
        crate::policy::PolicyDecision::AutoApprove => {}
        crate::policy::PolicyDecision::Prompt => {
            let (confirm_tx, confirm_rx) = oneshot::channel();
            let _ = event_tx
                .send(AgentEvent::ShellConfirmation {
                    command: command.to_string(),
                    respond: confirm_tx,
                })
                .await;
            match confirm_rx.await {
                Ok(true) => {}
                Ok(false) => {
                    return Ok("Shell command denied by user.".to_string());
                }
                Err(_) => {
                    return Ok("Shell command cancelled (no response).".to_string());
                }
            }
        }
    }

    // Resolve credentials and filter to only those declared by skills
    let all_creds = config.resolve_credentials();
    let mut filtered_creds: std::collections::HashMap<String, String> = match skill_env_allowlist {
        Some(allowlist) => all_creds
            .into_iter()
            .filter(|(k, _)| allowlist.contains(k))
            .collect(),
        None => all_creds,
    };
    // Merge per-skill env vars (these are explicitly configured, always included)
    let skill_env = crate::skills::collect_skill_env(&config.skills);
    for (k, v) in skill_env {
        filtered_creds.entry(k).or_insert(v);
    }
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, val) in &filtered_creds {
        cmd.env(key, val);
    }

    let mut child = cmd
        .kill_on_drop(true)
        .spawn()
        .context("Failed to spawn shell command")?;

    match stream_child_output(&mut child, timeout_dur, event_tx, "run_shell").await {
        Ok((stdout, stderr, code)) => {
            let status = code.unwrap_or(-1);
            Ok(format!(
                "Exit code: {status}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
            ))
        }
        Err(e) => {
            if e.to_string().contains("timed out") {
                Ok(format!(
                    "Error: command timed out after {timeout_ms}ms\nCommand: {command}"
                ))
            } else {
                Err(e)
            }
        }
    }
}

pub async fn handle_web_fetch(args: &serde_json::Value, config: &Config) -> Result<String> {
    if !config.web.enabled {
        return Ok("Web access is disabled. Enable it in config: [web] enabled = true".to_string());
    }
    let url = require_str_param(args, "url")?;
    let max_chars = args["max_chars"].as_u64().map(|v| v as usize);
    match web::web_fetch(url, max_chars).await {
        Ok(content) => Ok(content),
        Err(e) => Ok(format!("Error fetching URL: {e}")),
    }
}

pub async fn handle_web_search(args: &serde_json::Value, config: &Config) -> Result<String> {
    if !config.web.enabled {
        return Ok("Web access is disabled. Enable it in config: [web] enabled = true".to_string());
    }
    let query = require_str_param(args, "query")?;
    match web::web_search(query, &config.web).await {
        Ok(results) => Ok(results),
        Err(e) => Ok(format!("Error searching: {e}")),
    }
}

/// Open the database and run a callback, formatting the open error if it fails.
fn with_db<F>(f: F) -> Result<String>
where
    F: FnOnce(&Database) -> Result<String>,
{
    match Database::open() {
        Ok(db) => f(&db),
        Err(e) => Ok(format!("Error opening database: {e}")),
    }
}

pub fn handle_manage_tasks(args: &serde_json::Value, _config: &Config) -> Result<String> {
    let action = require_str_param(args, "action")?;
    match action {
        "create" => {
            let task_name = require_str_param(args, "name")?;
            let prompt = require_str_param(args, "prompt")?;
            let schedule_type = args["schedule_type"].as_str().unwrap_or("interval");
            let schedule_expr = require_str_param(args, "schedule_expr")?;
            let timezone = args["timezone"].as_str().unwrap_or("local");
            if let Err(e) = tasks::validate_schedule(schedule_type, schedule_expr) {
                return Ok(format!("Error: Invalid schedule: {e}"));
            }
            let next_run = match tasks::calculate_next_run(schedule_type, schedule_expr) {
                Ok(nr) => nr,
                Err(e) => return Ok(format!("Error: Invalid schedule: {e}")),
            };
            let id = uuid::Uuid::new_v4().to_string();
            with_db(|db| match db.create_task(&crate::db::NewTask {
                id: &id,
                name: task_name,
                prompt,
                schedule_type,
                schedule_expr,
                timezone,
                next_run,
                max_retries: args["max_retries"].as_i64().map(|v| v as i32),
                timeout_ms: args["timeout_ms"].as_i64(),
                delivery_channel: args["delivery_channel"].as_str(),
                delivery_target: args["delivery_target"].as_str(),
            }) {
                Ok(()) => Ok(format!(
                    "Scheduled task created: {task_name} (id: {})",
                    &id[..8]
                )),
                Err(e) => Ok(format!("Error creating task: {e}")),
            })
        }
        "list" => with_db(|db| match db.list_tasks() {
            Ok(tl) if tl.is_empty() => Ok("No scheduled tasks.".to_string()),
            Ok(tl) => Ok(tl
                .iter()
                .map(tasks::format_task)
                .collect::<Vec<_>>()
                .join("\n\n")),
            Err(e) => Ok(format!("Error listing tasks: {e}")),
        }),
        "get" => {
            let task_id = require_str_param(args, "task_id")?;
            with_db(|db| match db.get_task_by_id(task_id) {
                Ok(Some(task)) => {
                    let mut output = tasks::format_task(&task);
                    if let Ok(Some(run)) = db.last_task_run(task_id) {
                        let status = if run.error.is_some() { "error" } else { "ok" };
                        let when = chrono::DateTime::from_timestamp(run.started_at, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| run.started_at.to_string());
                        output.push_str(&format!(
                            "\n    Last run: {status} at {when} ({} ms)",
                            run.duration_ms
                        ));
                    }
                    Ok(output)
                }
                Ok(None) => Ok(format!("Task {task_id} not found.")),
                Err(e) => Ok(format!("Error: {e}")),
            })
        }
        "update" => {
            let task_id = require_str_param(args, "task_id")?;
            let update = crate::db::UpdateTask {
                name: args["name"].as_str(),
                prompt: args["prompt"].as_str(),
                schedule_type: args["schedule_type"].as_str(),
                schedule_expr: args["schedule_expr"].as_str(),
                timezone: args["timezone"].as_str(),
            };
            if let Some(st) = update.schedule_type {
                let expr = update.schedule_expr.unwrap_or("");
                if let Err(e) = tasks::validate_schedule(st, expr) {
                    return Ok(format!("Error: Invalid schedule: {e}"));
                }
            } else if let Some(expr) = update.schedule_expr {
                let validation = with_db(|db| match db.get_task_by_id(task_id) {
                    Ok(Some(existing)) => {
                        if let Err(e) =
                            tasks::validate_schedule(&existing.schedule_type, expr)
                        {
                            return Ok(format!("Error: Invalid schedule: {e}"));
                        }
                        Ok(String::new())
                    }
                    Ok(None) => Ok(format!("Task {task_id} not found.")),
                    Err(e) => Ok(format!("Error: {e}")),
                })?;
                if !validation.is_empty() {
                    return Ok(validation);
                }
            }
            with_db(|db| match db.update_task(task_id, &update) {
                Ok(true) => Ok(format!("Task {task_id} updated.")),
                Ok(false) => Ok(format!("Task {task_id} not found.")),
                Err(e) => Ok(format!("Error: {e}")),
            })
        }
        "pause" => {
            let task_id = require_str_param(args, "task_id")?;
            update_task_status(task_id, "paused", "paused")
        }
        "resume" => {
            let task_id = require_str_param(args, "task_id")?;
            update_task_status(task_id, "active", "resumed")
        }
        "cancel" => {
            let task_id = require_str_param(args, "task_id")?;
            update_task_status(task_id, "cancelled", "cancelled")
        }
        "delete" => {
            let task_id = require_str_param(args, "task_id")?;
            with_db(|db| match db.delete_task(task_id) {
                Ok(true) => Ok(format!("Task {task_id} deleted.")),
                Ok(false) => Ok(format!("Task {task_id} not found.")),
                Err(e) => Ok(format!("Error: {e}")),
            })
        }
        "runs" => {
            let task_id = require_str_param(args, "task_id")?;
            let limit = args["limit"].as_u64().unwrap_or(5) as usize;
            with_db(|db| match db.task_run_history(task_id, limit) {
                Ok(runs) if runs.is_empty() => Ok("No runs recorded.".to_string()),
                Ok(runs) => {
                    let mut out = String::new();
                    for run in &runs {
                        let when = chrono::DateTime::from_timestamp(run.started_at, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| run.started_at.to_string());
                        let status = if run.error.is_some() { "FAIL" } else { "OK" };
                        out.push_str(&format!("  {when} [{status}] {}ms", run.duration_ms));
                        if let Some(ref e) = run.error {
                            out.push_str(&format!(
                                "\n    Error: {}",
                                &e[..e.len().min(200)]
                            ));
                        }
                        out.push('\n');
                    }
                    Ok(out)
                }
                Err(e) => Ok(format!("Error: {e}")),
            })
        }
        "run_now" => {
            let task_id = require_str_param(args, "task_id")?;
            with_db(|db| match db.get_task_by_id(task_id) {
                Ok(Some(_)) => {
                    let now = chrono::Utc::now().timestamp();
                    if let Err(e) = db.update_task_next_run(task_id, Some(now)) {
                        return Ok(format!("Error: {e}"));
                    }
                    let _ = db.clear_task_retry(task_id);
                    Ok(format!("Task {task_id} queued for immediate execution."))
                }
                Ok(None) => Ok(format!("Task {task_id} not found.")),
                Err(e) => Ok(format!("Error: {e}")),
            })
        }
        other => Ok(format!(
            "Unknown action: {other}. Use: create, list, get, update, pause, resume, cancel, delete, runs, run_now."
        )),
    }
}

pub fn handle_read_pdf(args: &serde_json::Value) -> Result<String> {
    let file_path = require_str_param(args, "file_path")?;
    let max_chars = args["max_chars"].as_u64().unwrap_or(50000) as usize;
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        return Ok(format!("File not found: {file_path}"));
    }
    match pdf_extract::extract_text(path) {
        Ok(text) => {
            if text.len() > max_chars {
                let truncated: String = text.chars().take(max_chars).collect();
                Ok(format!(
                    "{truncated}\n\n[truncated — {max_chars}/{} chars shown]",
                    text.len()
                ))
            } else {
                Ok(text)
            }
        }
        Err(e) => Ok(format!("Error reading PDF: {e}")),
    }
}

/// Image file extensions that should be returned as multimodal content.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "heic", "heif", "svg",
];

/// Check whether `path` falls under any of the configured blocked paths.
/// Check if a path falls within any of the security-blocked directories.
pub fn is_blocked_path(path: &std::path::Path, blocked: &[String]) -> bool {
    let Some(home) = dirs::home_dir() else {
        // Can't verify — deny by default
        return true;
    };
    for entry in blocked {
        let blocked_abs = home.join(entry);
        if path.starts_with(&blocked_abs) {
            return true;
        }
    }
    false
}

pub fn handle_read_file(args: &serde_json::Value, config: &Config) -> Result<ToolOutput> {
    let raw_path = require_str_param(args, "path")?;
    let offset = args["offset"].as_u64().unwrap_or(1).max(1) as usize;
    let limit = args["limit"].as_u64().unwrap_or(0) as usize;
    let max_chars = args["max_chars"].as_u64().unwrap_or(50000) as usize;

    // Resolve path: expand ~ and resolve relative paths
    let expanded = shellexpand::tilde(raw_path).to_string();
    let resolved = if std::path::Path::new(&expanded).is_absolute() {
        std::path::PathBuf::from(&expanded)
    } else {
        std::env::current_dir().unwrap_or_default().join(&expanded)
    };

    // Canonicalize to prevent traversal
    let canonical = match resolved.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return Ok(ToolOutput::Text(format!("File not found: {raw_path}")));
        }
    };

    if !canonical.exists() {
        return Ok(ToolOutput::Text(format!("File not found: {raw_path}")));
    }

    if canonical.is_dir() {
        return Ok(ToolOutput::Text(format!(
            "Path is a directory, not a file: {raw_path}. Use run_shell with ls to list directory contents."
        )));
    }

    // Security: check blocked paths
    if is_blocked_path(&canonical, &config.security.blocked_paths) {
        return Ok(ToolOutput::Text(format!(
            "Access denied: {raw_path} is in a blocked path."
        )));
    }

    // Dispatch by extension
    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext == "pdf" {
        // Delegate to existing PDF handler
        let pdf_args =
            serde_json::json!({"file_path": canonical.to_string_lossy(), "max_chars": max_chars});
        return Ok(ToolOutput::Text(handle_read_pdf(&pdf_args)?));
    }

    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        // Guard against huge images (50MB max)
        if let Ok(meta) = std::fs::metadata(&canonical) {
            if meta.len() > 50 * 1024 * 1024 {
                return Ok(ToolOutput::Text(format!(
                    "Image too large ({} MB). Max 50 MB.",
                    meta.len() / (1024 * 1024)
                )));
            }
        }

        // Read image bytes, compress, return as multimodal
        let raw_bytes = match std::fs::read(&canonical) {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolOutput::Text(format!("Error reading file: {e}")));
            }
        };

        let engine = base64::engine::general_purpose::STANDARD;
        let b64 = engine.encode(&raw_bytes);
        let mime = match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "heic" | "heif" => "image/heic",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        };

        // Compress if needed (1MB threshold)
        let (final_b64, final_mime) =
            crate::media::compress_image(&b64, mime, 1_048_576).unwrap_or((b64, mime.to_string()));

        let summary = format!(
            "Image: {} ({} bytes)",
            canonical.file_name().unwrap_or_default().to_string_lossy(),
            raw_bytes.len()
        );

        return Ok(ToolOutput::Multimodal {
            text: summary.clone(),
            parts: vec![
                ContentPart::Text(summary),
                ContentPart::ImageBase64 {
                    media: MediaData {
                        mime_type: final_mime,
                        data: final_b64,
                        filename: canonical
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string()),
                    },
                },
            ],
        });
    }

    // Text file: read with line numbers
    let content = match std::fs::read_to_string(&canonical) {
        Ok(c) => c,
        Err(e) => {
            return Ok(ToolOutput::Text(format!(
                "Error reading file: {e}. The file may be binary."
            )));
        }
    };

    if content.is_empty() {
        return Ok(ToolOutput::Text(format!("[File is empty: {raw_path}]")));
    }

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Apply offset (1-based) and limit
    let start = (offset - 1).min(total_lines);
    let end = if limit > 0 {
        (start + limit).min(total_lines)
    } else {
        total_lines
    };

    let mut output = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        let line_no = start + i + 1;
        output.push_str(&format!("{line_no:>6}\t{line}\n"));
    }

    // Truncate if too long (safe for multi-byte UTF-8)
    if output.len() > max_chars {
        let truncate_at = output
            .char_indices()
            .take_while(|(i, _)| *i <= max_chars)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        output.truncate(truncate_at);
        output.push_str(&format!(
            "\n\n[truncated — {max_chars} chars shown, {total_lines} total lines]"
        ));
    } else if end < total_lines {
        output.push_str(&format!(
            "\n[showing lines {offset}–{end} of {total_lines}]"
        ));
    }

    Ok(ToolOutput::Text(output))
}

pub fn handle_security_audit(args: &serde_json::Value, config: &Config) -> Result<String> {
    if !config.security.host_audit {
        return Ok(
            "Host audit is disabled. Enable it in config: security.host_audit = true".to_string(),
        );
    }
    use crate::doctor::DiagnosticReport;
    use crate::host_audit;

    let mut audit_checks = Vec::new();
    match args.get("category").and_then(|v| v.as_str()) {
        Some("firewall") => host_audit::check_firewall(&mut audit_checks),
        Some("ports") => host_audit::check_listening_ports(&mut audit_checks),
        Some("ssh") => host_audit::check_ssh_config(&mut audit_checks),
        Some("permissions") => host_audit::check_sensitive_permissions(&mut audit_checks),
        Some("encryption") => host_audit::check_disk_encryption(&mut audit_checks),
        Some("updates") => host_audit::check_os_updates(&mut audit_checks),
        Some("services") => host_audit::check_running_services(&mut audit_checks),
        Some(other) => {
            return Ok(format!(
                "Unknown audit category: {other}. Valid: firewall, ports, ssh, permissions, encryption, updates, services"
            ))
        }
        None => host_audit::run_host_security_checks(&mut audit_checks),
    }
    let report = DiagnosticReport {
        checks: audit_checks,
    };
    Ok(report.format())
}

pub async fn handle_generate_image(args: &serde_json::Value, config: &Config) -> Result<String> {
    if !config.image_gen.enabled {
        return Ok(
            "Image generation is disabled. Enable it in config: image_gen.enabled = true"
                .to_string(),
        );
    }

    let prompt = args
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if prompt.is_empty() {
        return Ok("Error: prompt is required".to_string());
    }

    let count = args
        .get("count")
        .and_then(serde_json::Value::as_u64)
        .map(|n| n as u32);
    let size = args.get("size").and_then(serde_json::Value::as_str);

    let provider = match crate::image_gen::ImageGenProvider::from_config(&config.image_gen) {
        Some(p) => p,
        None => {
            return Ok(
                "No image generation provider available. Set OPENAI_API_KEY or FAL_KEY environment variable, or configure [image_gen] in config.toml"
                    .to_string(),
            );
        }
    };

    match crate::image_gen::generate_image(&provider, prompt, size, count).await {
        Ok(results) if results.is_empty() => Ok("Image generation returned no results".to_string()),
        Ok(results) => {
            let count = results.len();
            let mut output = format!("Generated {count} image(s).\n");
            for (i, img) in results.iter().enumerate() {
                if let Some(ref revised) = img.revised_prompt {
                    output.push_str(&format!("Image {}: revised prompt: {revised}\n", i + 1));
                }
                // Truncate base64 for text output — full data available for channel delivery
                let preview_len = img.base64_data.len().min(100);
                output.push_str(&format!(
                    "Image {}: {} bytes (base64: {}...)\n",
                    i + 1,
                    img.base64_data.len() * 3 / 4, // approximate decoded size
                    &img.base64_data[..preview_len]
                ));
            }
            Ok(output)
        }
        Err(e) => Ok(format!("Image generation failed: {e}")),
    }
}

pub async fn handle_text_to_speech(
    args: &serde_json::Value,
    synthesizer: &crate::tts::TtsSynthesizer,
) -> ToolOutput {
    let text = match require_str_param(args, "text") {
        Ok(t) => t,
        Err(e) => return ToolOutput::Text(format!("Error: {e}")),
    };
    let voice = args.get("voice").and_then(|v| v.as_str());
    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .and_then(crate::tts::AudioFormat::from_str_lossy);

    match synthesizer.synthesize(text, voice, format).await {
        Ok((audio_bytes, fmt, _attempts)) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&audio_bytes);
            ToolOutput::Multimodal {
                text: format!(
                    "Generated {} audio ({} bytes, {})",
                    fmt.extension(),
                    audio_bytes.len(),
                    fmt.mime_type()
                ),
                parts: vec![ContentPart::AudioBase64 {
                    media: MediaData {
                        mime_type: fmt.mime_type().to_string(),
                        data: b64,
                        filename: Some(format!("speech.{}", fmt.extension())),
                    },
                }],
            }
        }
        Err(e) => ToolOutput::Text(format!("TTS error: {e}")),
    }
}

pub async fn handle_user_tool(
    name: &str,
    args_json: &str,
    config: &Config,
    registry: &ToolRegistry,
    event_tx: &mpsc::Sender<AgentEvent>,
) -> Result<String> {
    let cred_names = registry.tool_credentials(name);
    let extra_env: Vec<(String, String)> = cred_names
        .iter()
        .filter_map(|cred_name| {
            config.credentials.get(cred_name).and_then(|cred_val| {
                cred_val
                    .resolve()
                    .ok()
                    .map(|val| (cred_name.to_uppercase(), val))
            })
        })
        .collect();
    let tool_name = name.to_string();
    let tx = event_tx.clone();
    let on_output = move |line: &str, is_stderr: bool| {
        let _ = tx.try_send(AgentEvent::ToolOutputDelta {
            name: tool_name.clone(),
            delta: line.to_string(),
            is_stderr,
        });
    };
    match registry
        .execute_tool_streaming(
            name,
            args_json,
            &extra_env,
            &config.security.blocked_paths,
            on_output,
        )
        .await
    {
        Ok(result) => Ok(result),
        Err(e) => Ok(format!("Error executing tool '{name}': {e}")),
    }
}

pub fn update_task_status(task_id: &str, status: &str, verb: &str) -> Result<String> {
    with_db(|db| match db.update_task_status(task_id, status) {
        Ok(true) => Ok(format!("Task {task_id} {verb}.")),
        Ok(false) => Ok(format!("Task {task_id} not found.")),
        Err(e) => Ok(format!("Error: {e}")),
    })
}

pub async fn handle_browser(
    args: &serde_json::Value,
    config: &Config,
    session: &mut Option<BrowserSession>,
) -> Result<ToolOutput> {
    if !config.browser.enabled {
        return Ok(ToolOutput::Text(
            "Browser automation is disabled. Enable it in config: [browser] enabled = true"
                .to_string(),
        ));
    }

    let action = require_str_param(args, "action")?;

    if let Some(err_msg) = validate_browser_args(action, args) {
        return Ok(ToolOutput::Text(format!("Error: {err_msg}")));
    }

    // Handle close without needing a session
    if action == "close" {
        if let Some(s) = session.take() {
            s.close().await.ok();
            return Ok(ToolOutput::Text("Browser closed.".to_string()));
        }
        return Ok(ToolOutput::Text("No browser session to close.".to_string()));
    }

    // Lazy-launch browser session
    if session.is_none() {
        match BrowserSession::launch(&config.browser).await {
            Ok(s) => *session = Some(s),
            Err(e) => return Ok(ToolOutput::Text(format!("Error launching browser: {e}"))),
        }
    }

    let browser = session.as_ref().context("Browser session not available")?;
    let timeout = Duration::from_millis(config.browser.timeout_ms);

    /// Wrap a browser action result into a ToolOutput.
    fn browser_result(result: anyhow::Result<String>) -> Result<ToolOutput> {
        match result {
            Ok(msg) => Ok(ToolOutput::Text(msg)),
            Err(e) => Ok(ToolOutput::Text(format!("Error: {e}"))),
        }
    }

    match action {
        "navigate" => {
            let url = require_str_param(args, "url")?;
            browser_result(browser.navigate(url, timeout).await)
        }
        "click" => {
            let selector = require_str_param(args, "selector")?;
            browser_result(browser.click(selector, timeout).await)
        }
        "type" => {
            let selector = require_str_param(args, "selector")?;
            let text = require_str_param(args, "text")?;
            browser_result(browser.type_text(selector, text, timeout).await)
        }
        "screenshot" => {
            let selector = args.get("selector").and_then(|v| v.as_str());
            match browser.screenshot(selector, timeout).await {
                Ok((desc, png_bytes)) => {
                    // Save to disk
                    let saved_path = Config::data_dir().ok().and_then(|data_dir| {
                        let dir = data_dir.join("screenshots");
                        std::fs::create_dir_all(&dir).ok()?;
                        let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S%3f");
                        let path = dir.join(format!("screenshot_{ts}.png"));
                        std::fs::write(&path, &png_bytes).ok()?;
                        Some(path)
                    });

                    let text = match &saved_path {
                        Some(p) => format!("{desc}\nSaved to: {}", p.display()),
                        None => desc.clone(),
                    };

                    let b64 = base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        &png_bytes,
                    );
                    Ok(ToolOutput::Multimodal {
                        text,
                        parts: vec![
                            ContentPart::Text(desc),
                            ContentPart::ImageBase64 {
                                media: MediaData {
                                    mime_type: "image/png".to_string(),
                                    data: b64,
                                    filename: Some("screenshot.png".to_string()),
                                },
                            },
                        ],
                    })
                }
                Err(e) => Ok(ToolOutput::Text(format!("Error: {e}"))),
            }
        }
        "get_text" => {
            let selector = args.get("selector").and_then(|v| v.as_str());
            browser_result(browser.get_text(selector, timeout).await)
        }
        "evaluate_js" => {
            let expression = require_str_param(args, "expression")?;
            browser_result(browser.evaluate_js(expression, timeout).await)
        }
        _ => Ok(ToolOutput::Text(format!(
            "Unknown browser action: {action}"
        ))),
    }
}

pub fn core_tool_definitions(config: &Config) -> Vec<ToolDefinition> {
    let mut defs = vec![
        ToolDefinition::new("write_memory", "Write or append to a memory file. Use filename 'IDENTITY.md' to update personality, 'MEMORY.md' for the index, or any other name for topic-specific memories. Use scope='local' to write to project-local memory (.borg/ in CWD).", serde_json::json!({"type":"object","properties":{"filename":{"type":"string","description":"Name of the memory file"},"content":{"type":"string","description":"Content to write"},"append":{"type":"boolean","description":"Append instead of overwriting","default":false},"scope":{"type":"string","enum":["global","local"],"description":"Memory scope: 'global' (default, ~/.borg/) or 'local' (CWD/.borg/)","default":"global"}},"required":["filename","content"]})),
        ToolDefinition::new("read_memory", "Read a memory file.", serde_json::json!({"type":"object","properties":{"filename":{"type":"string","description":"Name of the memory file to read"}},"required":["filename"]})),
        ToolDefinition::new("memory_search", "Search memory files semantically. Use before answering questions about prior work, decisions, preferences, or anything previously discussed.", serde_json::json!({"type":"object","properties":{"query":{"type":"string","description":"Search query"},"max_results":{"type":"integer","description":"Maximum results to return (default: 5)","default":5},"min_score":{"type":"number","description":"Minimum relevance score 0-1 (default: 0.2)","default":0.2}},"required":["query"]})),
        ToolDefinition::new("list", "List resources. Specify what to list: tools, skills, channels, or agents.", serde_json::json!({"type":"object","properties":{"what":{"type":"string","enum":["tools","skills","channels","agents"],"description":"What to list"}},"required":["what"]})),
        ToolDefinition::new("apply_patch", "Create, update, or delete files using the patch DSL. Use target to choose location: cwd (default), tools (~/.borg/tools/), skills (~/.borg/skills/), channels (~/.borg/channels/).", serde_json::json!({"type":"object","properties":{"patch":{"type":"string","description":"The patch content in the patch DSL format"},"target":{"type":"string","enum":["cwd","tools","skills","channels"],"description":"Where to apply the patch (default: cwd)","default":"cwd"}},"required":["patch"]})),
        ToolDefinition::new("run_shell", "Execute a shell command. Requires user confirmation before execution.", serde_json::json!({"type":"object","properties":{"command":{"type":"string","description":"Shell command to execute"}},"required":["command"]})),
        ToolDefinition::new("read_pdf", "Read and extract text from a PDF file.", serde_json::json!({"type":"object","properties":{"file_path":{"type":"string","description":"Path to the PDF file"},"max_chars":{"type":"integer","description":"Maximum characters to return (default: 50000)","default":50000}},"required":["file_path"]})),
        ToolDefinition::new("read_file", "Read a file's contents. Returns text with line numbers for code files, renders images visually, and extracts text from PDFs. Use offset/limit to read specific line ranges of large files.", serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path (relative to cwd or absolute)"},"offset":{"type":"integer","description":"Start line, 1-based (default: 1)"},"limit":{"type":"integer","description":"Max lines to read (default: all, truncated at max_chars)"},"max_chars":{"type":"integer","description":"Max characters to return (default: 50000)"}},"required":["path"]})),
    ];

    if config.web.enabled {
        defs.push(ToolDefinition::new("web_fetch", "Fetch a URL and return its text content. HTML pages are automatically converted to plain text.", serde_json::json!({"type":"object","properties":{"url":{"type":"string","description":"The URL to fetch"},"max_chars":{"type":"integer","description":"Maximum characters to return (default: 50000)","default":50000}},"required":["url"]})));
        defs.push(ToolDefinition::new("web_search", "Search the web and return results with titles, URLs, and snippets.", serde_json::json!({"type":"object","properties":{"query":{"type":"string","description":"The search query"}},"required":["query"]})));
    }

    defs.push(ToolDefinition::new("manage_tasks", "Manage scheduled tasks. Actions: create, list, get, update, pause, resume, cancel, delete, runs, run_now.", serde_json::json!({"type":"object","properties":{"action":{"type":"string","enum":["create","list","get","update","pause","resume","cancel","delete","runs","run_now"],"description":"Action to perform"},"task_id":{"type":"string","description":"Task ID (required for get/update/pause/resume/cancel/delete/runs/run_now)"},"name":{"type":"string","description":"Task name (required for create, optional for update)"},"prompt":{"type":"string","description":"Prompt to execute (required for create, optional for update)"},"schedule_type":{"type":"string","enum":["cron","interval","once"],"description":"Schedule type (required for create, optional for update)"},"schedule_expr":{"type":"string","description":"Cron expression or interval (required for create, optional for update)"},"timezone":{"type":"string","description":"Timezone (default: local)"},"max_retries":{"type":"integer","description":"Max retry attempts for transient failures (default: 3)"},"timeout_ms":{"type":"integer","description":"Timeout in milliseconds (default: 300000)"},"delivery_channel":{"type":"string","description":"Channel to deliver results to (telegram, slack, discord)"},"delivery_target":{"type":"string","description":"Target chat/channel ID for delivery"},"limit":{"type":"integer","description":"Number of runs to return (for runs action, default: 5)"}},"required":["action"]})));

    if config.browser.enabled {
        defs.push(ToolDefinition::new(
            "browser",
            "Control a headless Chrome browser. Actions: navigate (go to URL), click (CSS selector), type (type text into element), screenshot (capture page or element), get_text (extract text), evaluate_js (run JavaScript), close (shut down browser).",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["navigate", "click", "type", "screenshot", "get_text", "evaluate_js", "close"],
                        "description": "Browser action to perform"
                    },
                    "url": { "type": "string", "description": "URL to navigate to (for navigate)" },
                    "selector": { "type": "string", "description": "CSS selector (for click, type, get_text, screenshot)" },
                    "text": { "type": "string", "description": "Text to type (for type action)" },
                    "expression": { "type": "string", "description": "JavaScript expression (for evaluate_js)" }
                },
                "required": ["action"]
            }),
        ));
    }

    if config.tts.enabled {
        defs.push(ToolDefinition::new(
            "text_to_speech",
            "Convert text to speech audio. Returns base64-encoded audio data. Use for voice messages, audio responses, or accessibility.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to convert to speech (max 4096 characters)"
                    },
                    "voice": {
                        "type": "string",
                        "description": "Voice name/ID (optional, uses default if omitted)"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["mp3", "opus", "aac", "flac", "wav"],
                        "description": "Audio output format (optional, default: mp3)"
                    }
                },
                "required": ["text"]
            }),
        ));
    }

    if config.image_gen.enabled {
        defs.push(ToolDefinition::new(
            "generate_image",
            "Generate images from a text description using AI. Returns base64-encoded image data.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Text description of the image to generate"
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of images to generate (1-4, default: 1)"
                    },
                    "size": {
                        "type": "string",
                        "description": "Image size (e.g. 1024x1024, 1792x1024, 1024x1792)"
                    }
                },
                "required": ["prompt"]
            }),
        ));
    }

    if config.security.host_audit {
        defs.push(ToolDefinition::new(
            "security_audit",
            "Run a host security audit. Returns diagnostic findings about firewall, open ports, SSH config, file permissions, disk encryption, OS updates, and running services. Review findings and suggest fixes — the user must approve each change.",
            serde_json::json!({"type":"object","properties":{"category":{"type":"string","description":"Run only a specific check (omit for all)","enum":["firewall","ports","ssh","permissions","encryption","updates","services"]}}}),
        ));
    }

    defs
}

/// Chunk metadata: (snippet, start_line, end_line).
type ChunkMeta<'a> = std::collections::HashMap<(String, i64), (&'a str, Option<i64>, Option<i64>)>;

/// Execute hybrid memory search (FTS + vector) across global and local scopes.
pub async fn handle_memory_search(args: &serde_json::Value, config: &Config) -> Result<String> {
    let query = require_str_param(args, "query")?;
    let max_results = args["max_results"].as_u64().unwrap_or(5) as usize;
    let min_score = args["min_score"].as_f64().unwrap_or(0.2) as f32;
    let vector_weight = config.memory.embeddings.vector_weight;
    let bm25_weight = config.memory.embeddings.bm25_weight;
    let db = Database::open()?;
    let mut all_results = Vec::new();

    // Pre-compute query embedding once for all scopes
    let query_embedding = crate::embeddings::generate_query_embedding(config, query)
        .await
        .map(|(_prov, emb)| emb)
        .ok();

    if query_embedding.is_none() {
        tracing::debug!("memory_search: no embedding provider, falling back to FTS-only");
    }

    for scope in &["global", "local", "extra", "sessions"] {
        // FTS search
        let fts_rows = db
            .fts_search(scope, query, max_results * 4)
            .unwrap_or_default();
        let fts_owned: Vec<(String, i64, f32)> = fts_rows
            .iter()
            .map(|(c, score)| (c.filename.clone(), c.chunk_index, *score))
            .collect();

        // Build metadata maps from FTS results
        let fts_meta: ChunkMeta<'_> = fts_rows
            .iter()
            .map(|(c, _)| {
                (
                    (c.filename.clone(), c.chunk_index),
                    (c.content.as_str(), c.start_line, c.end_line),
                )
            })
            .collect();

        // Vector search across chunks
        let chunks = db.get_all_chunks(scope).unwrap_or_default();
        let vec_owned: Vec<(String, i64, f32)> = if let Some(ref query_emb) = query_embedding {
            chunks
                .iter()
                .filter_map(|c| {
                    c.embedding.as_ref().map(|emb_bytes| {
                        let stored = crate::embeddings::bytes_to_embedding(emb_bytes);
                        let sim = crate::embeddings::cosine_similarity(query_emb, &stored);
                        (c.filename.clone(), c.chunk_index, sim)
                    })
                })
                .filter(|(_f, _ci, sim)| *sim >= min_score * 0.5)
                .collect()
        } else {
            Vec::new()
        };

        // Build metadata map from chunk rows (snippet, line numbers)
        let chunk_meta: ChunkMeta<'_> = chunks
            .iter()
            .map(|c| {
                (
                    (c.filename.clone(), c.chunk_index),
                    (c.content.as_str(), c.start_line, c.end_line),
                )
            })
            .collect();

        // Merge hybrid scores
        let fts_refs: Vec<(&str, i64, f32)> = fts_owned
            .iter()
            .map(|(f, ci, s)| (f.as_str(), *ci, *s))
            .collect();
        let vec_refs: Vec<(&str, i64, f32)> = vec_owned
            .iter()
            .map(|(f, ci, s)| (f.as_str(), *ci, *s))
            .collect();
        let merged = crate::embeddings::merge_search_scores(
            &vec_refs,
            &fts_refs,
            vector_weight,
            bm25_weight,
        );

        for (filename, chunk_index, score) in merged {
            if score < min_score {
                continue;
            }
            let key = (filename.clone(), chunk_index);
            let (snippet, start_line, end_line) = fts_meta
                .get(&key)
                .or_else(|| chunk_meta.get(&key))
                .map(|(s, sl, el)| (s.to_string(), *sl, *el))
                .unwrap_or_default();
            all_results.push(crate::embeddings::SearchResult {
                filename,
                chunk_index,
                start_line,
                end_line,
                score,
                snippet,
            });
        }
    }

    // If no results, try a looser FTS search with individual terms
    if all_results.is_empty() {
        let terms: Vec<&str> = query.split_whitespace().collect();
        if terms.len() > 1 {
            let mut seen: std::collections::HashSet<(String, i64)> =
                std::collections::HashSet::new();
            for scope in &["global", "local", "extra", "sessions"] {
                for term in &terms {
                    let fts_rows = db.fts_search(scope, term, max_results).unwrap_or_default();
                    for (c, score) in fts_rows {
                        let key = (c.filename.clone(), c.chunk_index);
                        if score >= min_score && seen.insert(key) {
                            all_results.push(crate::embeddings::SearchResult {
                                filename: c.filename,
                                chunk_index: c.chunk_index,
                                start_line: c.start_line,
                                end_line: c.end_line,
                                score,
                                snippet: c.content,
                            });
                        }
                    }
                }
            }
        }
    }

    all_results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Pre-truncate before MMR to limit O(n^2) work
    all_results.truncate(max_results * 3);

    // Apply MMR diversity re-ranking if enabled
    if config.memory.embeddings.mmr_enabled && all_results.len() > 1 {
        let items: Vec<(usize, f32, &str)> = all_results
            .iter()
            .enumerate()
            .map(|(i, r)| (i, r.score, r.snippet.as_str()))
            .collect();
        let reordered =
            crate::mmr::mmr_rerank(&items, config.memory.embeddings.mmr_lambda, max_results);
        let original = all_results.clone();
        all_results = reordered.into_iter().map(|i| original[i].clone()).collect();
    }

    all_results.truncate(max_results);
    Ok(format_search_results(&all_results))
}

/// Format search results for display.
pub fn format_search_results(results: &[crate::embeddings::SearchResult]) -> String {
    if results.is_empty() {
        return "No matching memories found.".to_string();
    }
    let mut output = String::new();
    for (i, r) in results.iter().enumerate() {
        let lines = match (r.start_line, r.end_line) {
            (Some(s), Some(e)) => format!("lines {s}-{e}, "),
            _ => String::new(),
        };
        output.push_str(&format!(
            "[{}] {} ({lines}score: {:.2})\n> {}\n\n",
            i + 1,
            r.filename,
            r.score,
            r.snippet.chars().take(500).collect::<String>()
        ));
    }
    output.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- require_str_param --

    #[test]
    fn require_str_param_extracts_string() {
        let args = json!({"name": "hello"});
        assert_eq!(require_str_param(&args, "name").unwrap(), "hello");
    }

    #[test]
    fn require_str_param_missing_key_errors() {
        let args = json!({"other": "value"});
        let result = require_str_param(&args, "name");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing required"));
    }

    #[test]
    fn require_str_param_wrong_type_errors() {
        let args = json!({"name": 42});
        let result = require_str_param(&args, "name");
        assert!(result.is_err());
    }

    #[test]
    fn require_str_param_null_value_errors() {
        let args = json!({"name": null});
        let result = require_str_param(&args, "name");
        assert!(result.is_err());
    }

    #[test]
    fn require_str_param_empty_string_ok() {
        let args = json!({"name": ""});
        assert_eq!(require_str_param(&args, "name").unwrap(), "");
    }

    // -- core_tool_definitions --

    #[test]
    fn core_tool_definitions_includes_base_tools() {
        let config = Config::default();
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"write_memory"));
        assert!(names.contains(&"read_memory"));
        assert!(names.contains(&"list"));
        assert!(names.contains(&"apply_patch"));
        assert!(names.contains(&"run_shell"));
        assert!(names.contains(&"read_pdf"));
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"manage_tasks"));
        // Consolidated: no longer separate tools
        assert!(!names.contains(&"list_tools"));
        assert!(!names.contains(&"list_skills"));
        assert!(!names.contains(&"create_tool"));
        assert!(!names.contains(&"apply_skill_patch"));
    }

    #[test]
    fn core_tool_definitions_excludes_browser_when_disabled() {
        let mut config = Config::default();
        config.browser.enabled = false;
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(!names.contains(&"browser"));
    }

    #[test]
    fn core_tool_definitions_includes_browser_when_enabled() {
        let mut config = Config::default();
        config.browser.enabled = true;
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"browser"));
    }

    #[test]
    fn core_tool_definitions_excludes_tts_when_disabled() {
        let config = Config::default();
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(!names.contains(&"text_to_speech"));
    }

    #[test]
    fn core_tool_definitions_includes_tts_when_enabled() {
        let mut config = Config::default();
        config.tts.enabled = true;
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"text_to_speech"));
    }

    #[test]
    fn core_tool_definitions_excludes_security_audit_when_disabled() {
        let mut config = Config::default();
        config.security.host_audit = false;
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(!names.contains(&"security_audit"));
    }

    #[test]
    fn core_tool_definitions_includes_security_audit_when_enabled() {
        let mut config = Config::default();
        config.security.host_audit = true;
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"security_audit"));
    }

    #[test]
    fn core_tool_definitions_excludes_web_when_disabled() {
        let mut config = Config::default();
        config.web.enabled = false;
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(!names.contains(&"web_fetch"));
        assert!(!names.contains(&"web_search"));
    }

    #[test]
    fn core_tool_definitions_includes_web_when_enabled() {
        let mut config = Config::default();
        config.web.enabled = true;
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert!(names.contains(&"web_fetch"));
        assert!(names.contains(&"web_search"));
    }

    #[test]
    fn core_tool_definitions_all_have_parameters() {
        let config = Config::default();
        let defs = core_tool_definitions(&config);
        for def in &defs {
            assert!(
                def.function.parameters.is_object(),
                "Tool '{}' should have object parameters",
                def.function.name
            );
        }
    }

    #[test]
    fn core_tool_definitions_all_have_descriptions() {
        let config = Config::default();
        let defs = core_tool_definitions(&config);
        for def in &defs {
            assert!(
                !def.function.description.is_empty(),
                "Tool '{}' should have a description",
                def.function.name
            );
        }
    }

    // -- handle_security_audit --

    #[test]
    fn handle_security_audit_disabled() {
        let mut config = Config::default();
        config.security.host_audit = false;
        let result = handle_security_audit(&json!({}), &config).unwrap();
        assert!(result.contains("disabled"));
    }

    #[test]
    fn handle_security_audit_unknown_category() {
        let mut config = Config::default();
        config.security.host_audit = true;
        let result = handle_security_audit(&json!({"category": "invalid"}), &config).unwrap();
        assert!(result.contains("Unknown audit category"));
    }

    // -- handle_read_pdf --

    #[test]
    fn handle_read_pdf_missing_file() {
        let result = handle_read_pdf(&json!({"file_path": "/nonexistent/path.pdf"})).unwrap();
        assert!(result.contains("not found"));
    }

    #[test]
    fn handle_read_pdf_missing_param() {
        let result = handle_read_pdf(&json!({}));
        assert!(result.is_err());
    }

    // -- handle_read_file --

    #[test]
    fn handle_read_file_missing_file() {
        let config = Config::default();
        let result = handle_read_file(&json!({"path": "/nonexistent/file.txt"}), &config).unwrap();
        match result {
            ToolOutput::Text(s) => assert!(s.contains("not found")),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn handle_read_file_missing_param() {
        let config = Config::default();
        let result = handle_read_file(&json!({}), &config);
        assert!(result.is_err());
    }

    #[test]
    fn handle_read_file_text_with_line_numbers() {
        let config = Config::default();
        // Read this source file itself
        let result = handle_read_file(&json!({"path": "Cargo.toml", "limit": 3}), &config).unwrap();
        match result {
            ToolOutput::Text(s) => {
                assert!(s.contains("     1\t"), "should have line numbers");
                assert!(s.contains("     2\t"));
                assert!(s.contains("     3\t"));
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn handle_read_file_offset_limit() {
        let config = Config::default();
        let result = handle_read_file(
            &json!({"path": "Cargo.toml", "offset": 2, "limit": 2}),
            &config,
        )
        .unwrap();
        match result {
            ToolOutput::Text(s) => {
                assert!(!s.contains("     1\t"), "should not include line 1");
                assert!(s.contains("     2\t"), "should start at line 2");
                assert!(s.contains("     3\t"), "should include line 3");
                assert!(!s.contains("     4\t"), "should stop at limit");
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn handle_read_file_blocked_path() {
        let config = Config::default();
        // Default blocked_paths includes .ssh
        let home = dirs::home_dir().unwrap();
        let blocked = home.join(".ssh/id_rsa");
        let result =
            handle_read_file(&json!({"path": blocked.to_string_lossy()}), &config).unwrap();
        match result {
            ToolOutput::Text(s) => assert!(s.contains("denied") || s.contains("not found")),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn handle_read_file_directory_rejected() {
        let config = Config::default();
        let result = handle_read_file(&json!({"path": "."}), &config).unwrap();
        match result {
            ToolOutput::Text(s) => assert!(s.contains("directory")),
            _ => panic!("expected Text"),
        }
    }

    // -- handle_manage_tasks --

    #[test]
    fn handle_manage_tasks_unknown_action() {
        let result = handle_manage_tasks(&json!({"action": "nope"}), &Config::default()).unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[test]
    fn handle_manage_tasks_missing_action() {
        let result = handle_manage_tasks(&json!({}), &Config::default());
        assert!(result.is_err());
    }

    fn empty_registry() -> ToolRegistry {
        let dir = std::env::temp_dir().join(format!("borg_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        ToolRegistry::with_dir(dir).unwrap()
    }

    // -- consolidated apply_patch --

    #[test]
    fn apply_patch_unified_unknown_target() {
        let mut registry = empty_registry();
        let args = json!({"patch": "*** Begin Patch\n*** End Patch", "target": "invalid"});
        let result = handle_apply_patch_unified(&args, &mut registry).unwrap();
        assert!(result.contains("Unknown target"));
    }

    #[test]
    fn apply_patch_unified_missing_patch() {
        let mut registry = empty_registry();
        let args = json!({"target": "cwd"});
        let result = handle_apply_patch_unified(&args, &mut registry);
        assert!(result.is_err());
    }

    #[test]
    fn apply_patch_unified_default_target_is_cwd() {
        let mut registry = empty_registry();
        // Empty patch is still valid
        let args = json!({"patch": "*** Begin Patch\n*** End Patch"});
        let result = handle_apply_patch_unified(&args, &mut registry);
        assert!(result.is_ok());
    }

    // -- consolidated list --

    #[test]
    fn list_unknown_what() {
        let registry = empty_registry();
        let config = Config::default();
        let args = json!({"what": "unknown"});
        let result = handle_list(&args, &registry, &config, None).unwrap();
        assert!(result.contains("Unknown list target"));
    }

    #[test]
    fn list_missing_what() {
        let registry = empty_registry();
        let config = Config::default();
        let args = json!({});
        let result = handle_list(&args, &registry, &config, None);
        assert!(result.is_err());
    }

    #[test]
    fn list_tools_returns_no_tools() {
        let registry = empty_registry();
        let config = Config::default();
        let args = json!({"what": "tools"});
        let result = handle_list(&args, &registry, &config, None).unwrap();
        assert!(result.contains("No user tools"));
    }

    #[test]
    fn list_agents_without_control() {
        let registry = empty_registry();
        let config = Config::default();
        let args = json!({"what": "agents"});
        let result = handle_list(&args, &registry, &config, None).unwrap();
        assert!(result.contains("not enabled"));
    }

    // -- core_tool_definitions consolidated --

    #[test]
    fn core_tool_definitions_has_apply_patch_with_target() {
        let config = Config::default();
        let defs = core_tool_definitions(&config);
        let ap = defs
            .iter()
            .find(|d| d.function.name == "apply_patch")
            .expect("should have apply_patch");
        let params = &ap.function.parameters;
        assert!(
            params["properties"]["target"].is_object(),
            "apply_patch should have 'target' parameter"
        );
    }

    #[test]
    fn core_tool_definitions_has_list_with_what() {
        let config = Config::default();
        let defs = core_tool_definitions(&config);
        let list = defs
            .iter()
            .find(|d| d.function.name == "list")
            .expect("should have list");
        let params = &list.function.parameters;
        assert!(
            params["properties"]["what"].is_object(),
            "list should have 'what' parameter"
        );
    }

    #[test]
    fn core_tool_definitions_count_reduced() {
        // With all defaults enabled (web, browser, security_audit):
        // write_memory, read_memory, memory_search, list, apply_patch, run_shell, read_pdf,
        // read_file, web_fetch, web_search, manage_tasks, browser, security_audit = 13
        let config = Config::default();
        let defs = core_tool_definitions(&config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert_eq!(
            names.len(),
            13,
            "expected 13 core tools (all enabled), got: {names:?}"
        );

        // With everything disabled: 8 base tools
        let mut minimal_config = Config::default();
        minimal_config.web.enabled = false;
        minimal_config.browser.enabled = false;
        minimal_config.security.host_audit = false;
        let defs = core_tool_definitions(&minimal_config);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert_eq!(names.len(), 9, "expected 9 base tools, got: {names:?}");
    }

    #[test]
    fn format_memory_search_results() {
        let results = vec![
            crate::embeddings::SearchResult {
                filename: "notes.md".into(),
                chunk_index: 0,
                start_line: Some(1),
                end_line: Some(10),
                score: 0.87,
                snippet: "Important decision about architecture".into(),
            },
            crate::embeddings::SearchResult {
                filename: "daily/2026-03-19.md".into(),
                chunk_index: 2,
                start_line: Some(15),
                end_line: Some(22),
                score: 0.65,
                snippet: "Met with team about API design".into(),
            },
        ];
        let output = format_search_results(&results);
        assert!(output.contains("[1]"));
        assert!(output.contains("notes.md"));
        assert!(output.contains("0.87"));
        assert!(output.contains("Important decision"));
        assert!(output.contains("[2]"));
        assert!(output.contains("daily/2026-03-19.md"));
    }

    #[test]
    fn format_empty_search_results() {
        let results: Vec<crate::embeddings::SearchResult> = vec![];
        let output = format_search_results(&results);
        assert!(output.contains("No matching memories found"));
    }

    /// Mutex to prevent env-var–mutating channel tests from racing each other.
    static CHANNEL_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn list_channels_includes_native_telegram() {
        let _lock = CHANNEL_ENV_MUTEX.lock().unwrap();
        std::env::set_var("TELEGRAM_BOT_TOKEN", "test-token-for-list-test");
        let config = Config::default();
        let result = handle_list_channels(&config).unwrap();
        assert!(
            result.contains("telegram"),
            "Should list native Telegram channel, got: {result}"
        );
        assert!(
            result.contains("native"),
            "Should indicate it's native, got: {result}"
        );
        std::env::remove_var("TELEGRAM_BOT_TOKEN");
    }

    #[test]
    fn list_channels_no_native_when_no_credentials() {
        let _lock = CHANNEL_ENV_MUTEX.lock().unwrap();
        let keys = [
            "TELEGRAM_BOT_TOKEN",
            "SLACK_BOT_TOKEN",
            "DISCORD_BOT_TOKEN",
            "TWILIO_ACCOUNT_SID",
            "TEAMS_APP_ID",
            "GOOGLE_CHAT_SERVICE_TOKEN",
        ];
        let saved: Vec<_> = keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        for k in &keys {
            std::env::remove_var(k);
        }
        let config = Config::default();
        let result = handle_list_channels(&config).unwrap();
        assert!(
            !result.contains("native"),
            "Should not list native channels without credentials, got: {result}"
        );
        // Restore
        for (k, v) in saved {
            if let Some(val) = v {
                std::env::set_var(k, val);
            }
        }
    }

    // -- handle_list_tools (enhanced with built-ins) --

    #[test]
    fn handle_list_tools_includes_builtins() {
        let registry = empty_registry();
        let config = Config::default();
        let result = handle_list_tools(&registry, &config).unwrap();
        assert!(result.contains("Memory"), "should include Memory group");
        assert!(
            result.contains("Filesystem"),
            "should include Filesystem group"
        );
        assert!(
            result.contains("write_memory"),
            "should list write_memory tool"
        );
        assert!(
            result.contains("apply_patch"),
            "should list apply_patch tool"
        );
    }

    #[test]
    fn handle_list_tools_shows_profile() {
        let registry = empty_registry();
        let config = Config::default();
        let result = handle_list_tools(&registry, &config).unwrap();
        assert!(result.contains("Full"), "should show current profile name");
    }

    #[test]
    fn handle_list_tools_has_user_tools_section() {
        let registry = empty_registry();
        let config = Config::default();
        let result = handle_list_tools(&registry, &config).unwrap();
        assert!(
            result.contains("User Tools"),
            "should have User Tools section"
        );
    }

    // -- is_blocked_path --

    #[test]
    fn is_blocked_path_matches_blocked_dir() {
        let home = dirs::home_dir().unwrap();
        let path = home.join(".ssh/id_rsa");
        let blocked = vec![".ssh".to_string()];
        assert!(is_blocked_path(&path, &blocked));
    }

    #[test]
    fn is_blocked_path_rejects_non_blocked() {
        let home = dirs::home_dir().unwrap();
        let path = home.join("Documents/safe.txt");
        let blocked = vec![".ssh".to_string(), ".aws".to_string()];
        assert!(!is_blocked_path(&path, &blocked));
    }

    #[test]
    fn is_blocked_path_nested_blocked() {
        let home = dirs::home_dir().unwrap();
        let path = home.join(".aws/credentials/secret");
        let blocked = vec![".aws".to_string()];
        assert!(is_blocked_path(&path, &blocked));
    }

    #[test]
    fn is_blocked_path_empty_blocked_list() {
        let home = dirs::home_dir().unwrap();
        let path = home.join(".ssh/id_rsa");
        let blocked: Vec<String> = vec![];
        assert!(!is_blocked_path(&path, &blocked));
    }

    #[test]
    fn is_blocked_path_outside_home() {
        let blocked = vec![".ssh".to_string()];
        let path = std::path::Path::new("/tmp/.ssh/id_rsa");
        assert!(!is_blocked_path(path, &blocked));
    }

    // -- handle_read_file (additional) --

    #[test]
    fn handle_read_file_empty_file() {
        let tmp = std::env::temp_dir().join(format!("borg_empty_{}", std::process::id()));
        std::fs::write(&tmp, "").unwrap();
        let config = Config::default();
        let result =
            handle_read_file(&json!({"path": tmp.to_string_lossy().as_ref()}), &config).unwrap();
        match result {
            ToolOutput::Text(s) => assert!(s.contains("empty"), "expected 'empty' in: {s}"),
            _ => panic!("expected Text"),
        }
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn handle_read_file_tilde_expansion() {
        let config = Config::default();
        let result = handle_read_file(
            &json!({"path": "~/nonexistent_borg_test_file_xyz.txt"}),
            &config,
        )
        .unwrap();
        match result {
            ToolOutput::Text(s) => {
                assert!(s.contains("not found"), "expected 'not found' in: {s}")
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn handle_read_file_truncation() {
        let tmp = std::env::temp_dir().join(format!("borg_trunc_{}", std::process::id()));
        let content = "x\n".repeat(1000);
        std::fs::write(&tmp, &content).unwrap();
        let config = Config::default();
        let result = handle_read_file(
            &json!({"path": tmp.to_string_lossy().as_ref(), "max_chars": 100}),
            &config,
        )
        .unwrap();
        match result {
            ToolOutput::Text(s) => {
                assert!(s.contains("truncated"), "expected 'truncated' in: {s}")
            }
            _ => panic!("expected Text"),
        }
        std::fs::remove_file(&tmp).ok();
    }

    // -- disabled feature guards --

    #[tokio::test]
    async fn handle_web_fetch_disabled() {
        let mut config = Config::default();
        config.web.enabled = false;
        let result = handle_web_fetch(&json!({"url": "https://example.com"}), &config)
            .await
            .unwrap();
        assert!(
            result.contains("disabled"),
            "expected 'disabled' in: {result}"
        );
    }

    #[tokio::test]
    async fn handle_web_search_disabled() {
        let mut config = Config::default();
        config.web.enabled = false;
        let result = handle_web_search(&json!({"query": "test"}), &config)
            .await
            .unwrap();
        assert!(
            result.contains("disabled"),
            "expected 'disabled' in: {result}"
        );
    }

    #[tokio::test]
    async fn handle_generate_image_disabled() {
        let mut config = Config::default();
        config.image_gen.enabled = false;
        let result = handle_generate_image(&json!({"prompt": "a cat"}), &config)
            .await
            .unwrap();
        assert!(
            result.contains("disabled"),
            "expected 'disabled' in: {result}"
        );
    }

    #[tokio::test]
    async fn handle_generate_image_empty_prompt() {
        let mut config = Config::default();
        config.image_gen.enabled = true;
        let result = handle_generate_image(&json!({"prompt": ""}), &config)
            .await
            .unwrap();
        assert!(
            result.contains("required"),
            "expected 'required' in: {result}"
        );
    }

    // -- handle_list dispatch --

    #[test]
    fn list_skills_dispatches() {
        let registry = empty_registry();
        let config = Config::default();
        let args = json!({"what": "skills"});
        let result = handle_list(&args, &registry, &config, None);
        assert!(result.is_ok());
    }

    #[test]
    fn list_channels_dispatches() {
        let registry = empty_registry();
        let config = Config::default();
        let args = json!({"what": "channels"});
        let result = handle_list(&args, &registry, &config, None);
        assert!(result.is_ok());
    }

    // -- handle_manage_tasks (additional) --

    #[test]
    fn handle_manage_tasks_create_invalid_schedule() {
        let args = json!({
            "action": "create",
            "name": "test",
            "prompt": "do stuff",
            "schedule_type": "cron",
            "schedule_expr": "not a cron"
        });
        let result = handle_manage_tasks(&args, &Config::default()).unwrap();
        assert!(
            result.contains("Invalid schedule") || result.contains("Error"),
            "expected schedule error in: {result}"
        );
    }

    #[test]
    fn handle_manage_tasks_create_missing_name() {
        let args = json!({
            "action": "create",
            "prompt": "do stuff",
            "schedule_expr": "30m"
        });
        let result = handle_manage_tasks(&args, &Config::default());
        assert!(result.is_err());
    }

    // -- handle_list_tools profile filtering --

    #[test]
    fn handle_list_tools_minimal_profile_disables_groups() {
        let registry = empty_registry();
        let mut config = Config::default();
        config.tools.policy.profile = "minimal".to_string();
        let result = handle_list_tools(&registry, &config).unwrap();
        assert!(
            result.contains("(disabled)"),
            "minimal profile should mark most groups as disabled, got: {result}"
        );
        assert!(
            result.contains("Minimal"),
            "should show Minimal profile name"
        );
    }
}
