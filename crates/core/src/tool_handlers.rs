use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::{mpsc, oneshot};

use crate::agent::AgentEvent;
use crate::config::Config;
use crate::db::Database;
use crate::memory::{read_memory, write_memory_scoped, WriteMode};
use crate::policy::ExecutionPolicy;
use crate::skills::{load_all_skills, Skill};
use crate::tasks;
use crate::types::ToolDefinition;
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

pub fn handle_list_tools(registry: &ToolRegistry) -> Result<String> {
    let tool_list = registry.list_tools();
    Ok(if tool_list.is_empty() {
        "No user tools installed.".to_string()
    } else {
        tool_list.join("\n")
    })
}

pub fn handle_list_skills(config: &Config) -> Result<String> {
    let resolved_creds = config.resolve_credentials();
    let skills = load_all_skills(&resolved_creds)?;
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

pub fn handle_apply_skill_patch(args: &serde_json::Value) -> Result<String> {
    let patch = require_str_param(args, "patch")?;
    let base_dir = Config::skills_dir()?;
    std::fs::create_dir_all(&base_dir)?;
    match apply_patch_to_dir(patch, &base_dir) {
        Ok(_) => Ok("Skill patch applied successfully.".to_string()),
        Err(e) => Ok(format!("Error applying skill patch: {e}")),
    }
}

pub fn handle_apply_patch(args: &serde_json::Value) -> Result<String> {
    let patch = require_str_param(args, "patch")?;
    let base_dir =
        std::env::current_dir().context("Failed to determine current working directory")?;
    match apply_patch_to_dir(patch, &base_dir) {
        Ok(affected) => Ok(format!(
            "Patch applied successfully. Files affected: {}",
            affected.join(", ")
        )),
        Err(e) => Ok(format!("Error applying patch: {e}")),
    }
}

pub fn handle_create_tool(args: &serde_json::Value, registry: &mut ToolRegistry) -> Result<String> {
    let patch = require_str_param(args, "patch")?;
    let base_dir = Config::tools_dir()?;
    std::fs::create_dir_all(&base_dir)?;
    match apply_patch_to_dir(patch, &base_dir) {
        Ok(_) => {
            *registry = ToolRegistry::new()?;
            Ok("Patch applied successfully. Tool registry reloaded.".to_string())
        }
        Err(e) => Ok(format!("Error applying patch: {e}")),
    }
}

pub fn handle_create_channel(args: &serde_json::Value) -> Result<String> {
    let patch = require_str_param(args, "patch")?;
    let base_dir = Config::channels_dir()?;
    std::fs::create_dir_all(&base_dir)?;
    match apply_patch_to_dir(patch, &base_dir) {
        Ok(_) => Ok("Channel patch applied successfully.".to_string()),
        Err(e) => Ok(format!("Error applying channel patch: {e}")),
    }
}

pub fn handle_list_channels() -> Result<String> {
    let channels_dir = Config::channels_dir()?;
    if !channels_dir.exists() {
        return Ok("No channels directory found.".to_string());
    }
    let mut channels = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&channels_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let manifest_path = path.join("channel.toml");
                if manifest_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                        if let Ok(manifest) = toml::from_str::<toml::Value>(&content) {
                            let name = manifest.get("name").and_then(|v| v.as_str()).unwrap_or("?");
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
    Ok(if channels.is_empty() {
        "No channels installed.".to_string()
    } else {
        channels.join("\n")
    })
}

pub async fn handle_run_shell(
    args: &serde_json::Value,
    config: &Config,
    policy: &ExecutionPolicy,
    event_tx: &mpsc::Sender<AgentEvent>,
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

    let resolved_creds = config.resolve_credentials();
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    for (key, val) in &resolved_creds {
        cmd.env(key, val);
    }
    let child = cmd.output();

    match tokio::time::timeout(timeout_dur, child).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let status = output.status.code().unwrap_or(-1);
            Ok(format!(
                "Exit code: {status}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
            ))
        }
        Ok(Err(e)) => Err(anyhow::anyhow!("Failed to execute shell command: {e}")),
        Err(_) => Ok(format!(
            "Error: command timed out after {timeout_ms}ms\nCommand: {command}"
        )),
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

pub fn handle_manage_tasks(args: &serde_json::Value, config: &Config) -> Result<String> {
    if !config.tasks.enabled {
        return Ok(
            "Task scheduling is disabled. Enable it in config: tasks.enabled = true".to_string(),
        );
    }
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
            match Database::open() {
                Ok(db) => match db.create_task(&crate::db::NewTask {
                    id: &id,
                    name: task_name,
                    prompt,
                    schedule_type,
                    schedule_expr,
                    timezone,
                    next_run,
                }) {
                    Ok(()) => Ok(format!(
                        "Scheduled task created: {task_name} (id: {})",
                        &id[..8]
                    )),
                    Err(e) => Ok(format!("Error creating task: {e}")),
                },
                Err(e) => Ok(format!("Error opening database: {e}")),
            }
        }
        "list" => match Database::open() {
            Ok(db) => match db.list_tasks() {
                Ok(tl) if tl.is_empty() => Ok("No scheduled tasks.".to_string()),
                Ok(tl) => Ok(tl
                    .iter()
                    .map(tasks::format_task)
                    .collect::<Vec<_>>()
                    .join("\n\n")),
                Err(e) => Ok(format!("Error listing tasks: {e}")),
            },
            Err(e) => Ok(format!("Error opening database: {e}")),
        },
        "get" => {
            let task_id = require_str_param(args, "task_id")?;
            match Database::open() {
                Ok(db) => match db.get_task_by_id(task_id) {
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
                },
                Err(e) => Ok(format!("Error opening database: {e}")),
            }
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
                match Database::open() {
                    Ok(db) => match db.get_task_by_id(task_id) {
                        Ok(Some(existing)) => {
                            if let Err(e) =
                                tasks::validate_schedule(&existing.schedule_type, expr)
                            {
                                return Ok(format!("Error: Invalid schedule: {e}"));
                            }
                        }
                        Ok(None) => return Ok(format!("Task {task_id} not found.")),
                        Err(e) => return Ok(format!("Error: {e}")),
                    },
                    Err(e) => return Ok(format!("Error opening database: {e}")),
                }
            }
            match Database::open() {
                Ok(db) => match db.update_task(task_id, &update) {
                    Ok(true) => Ok(format!("Task {task_id} updated.")),
                    Ok(false) => Ok(format!("Task {task_id} not found.")),
                    Err(e) => Ok(format!("Error: {e}")),
                },
                Err(e) => Ok(format!("Error opening database: {e}")),
            }
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
            match Database::open() {
                Ok(db) => match db.delete_task(task_id) {
                    Ok(true) => Ok(format!("Task {task_id} deleted.")),
                    Ok(false) => Ok(format!("Task {task_id} not found.")),
                    Err(e) => Ok(format!("Error: {e}")),
                },
                Err(e) => Ok(format!("Error opening database: {e}")),
            }
        }
        other => Ok(format!(
            "Unknown action: {other}. Use: create, list, get, update, pause, resume, cancel, delete."
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

pub async fn handle_user_tool(
    name: &str,
    args_json: &str,
    config: &Config,
    registry: &ToolRegistry,
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
    match registry
        .execute_tool_full(name, args_json, &extra_env, &config.security.blocked_paths)
        .await
    {
        Ok(result) => Ok(result),
        Err(e) => Ok(format!("Error executing tool '{name}': {e}")),
    }
}

pub fn update_task_status(task_id: &str, status: &str, verb: &str) -> Result<String> {
    match Database::open() {
        Ok(db) => match db.update_task_status(task_id, status) {
            Ok(true) => Ok(format!("Task {task_id} {verb}.")),
            Ok(false) => Ok(format!("Task {task_id} not found.")),
            Err(e) => Ok(format!("Error: {e}")),
        },
        Err(e) => Ok(format!("Error opening database: {e}")),
    }
}

pub fn core_tool_definitions(config: &Config) -> Vec<ToolDefinition> {
    let mut defs = vec![
        ToolDefinition::new("write_memory", "Write or append to a memory file. Use filename 'SOUL.md' to update personality, 'MEMORY.md' for the index, or any other name for topic-specific memories. Use scope='local' to write to project-local memory (.borg/ in CWD).", serde_json::json!({"type":"object","properties":{"filename":{"type":"string","description":"Name of the memory file"},"content":{"type":"string","description":"Content to write"},"append":{"type":"boolean","description":"Append instead of overwriting","default":false},"scope":{"type":"string","enum":["global","local"],"description":"Memory scope: 'global' (default, ~/.borg/) or 'local' (CWD/.borg/)","default":"global"}},"required":["filename","content"]})),
        ToolDefinition::new("read_memory", "Read a memory file.", serde_json::json!({"type":"object","properties":{"filename":{"type":"string","description":"Name of the memory file to read"}},"required":["filename"]})),
        ToolDefinition::new("list_tools", "List all available user-created tools.", serde_json::json!({"type":"object","properties":{}})),
        ToolDefinition::new("apply_patch", "Create, update, or delete files in the current working directory using the patch DSL.", serde_json::json!({"type":"object","properties":{"patch":{"type":"string","description":"The patch content in the patch DSL format"}},"required":["patch"]})),
        ToolDefinition::new("create_tool", "Create or modify user tools in ~/.borg/tools/ using the patch DSL.", serde_json::json!({"type":"object","properties":{"patch":{"type":"string","description":"The patch content in the patch DSL format"}},"required":["patch"]})),
        ToolDefinition::new("run_shell", "Execute a shell command. Requires user confirmation before execution.", serde_json::json!({"type":"object","properties":{"command":{"type":"string","description":"Shell command to execute"}},"required":["command"]})),
        ToolDefinition::new("list_skills", "List all available skills with their status and source.", serde_json::json!({"type":"object","properties":{}})),
        ToolDefinition::new("apply_skill_patch", "Create or modify skill files in the skills directory using the patch DSL.", serde_json::json!({"type":"object","properties":{"patch":{"type":"string","description":"The patch content in the patch DSL format"}},"required":["patch"]})),
        ToolDefinition::new("read_pdf", "Read and extract text from a PDF file.", serde_json::json!({"type":"object","properties":{"file_path":{"type":"string","description":"Path to the PDF file"},"max_chars":{"type":"integer","description":"Maximum characters to return (default: 50000)","default":50000}},"required":["file_path"]})),
        ToolDefinition::new("create_channel", "Create or modify messaging channel integrations in ~/.borg/channels/ using the patch DSL. Channels receive webhooks and route messages to the agent.", serde_json::json!({"type":"object","properties":{"patch":{"type":"string","description":"The patch content in the patch DSL format"}},"required":["patch"]})),
        ToolDefinition::new("list_channels", "List all messaging channel integrations with their status and webhook paths.", serde_json::json!({"type":"object","properties":{}})),
    ];

    if config.web.enabled {
        defs.push(ToolDefinition::new("web_fetch", "Fetch a URL and return its text content. HTML pages are automatically converted to plain text.", serde_json::json!({"type":"object","properties":{"url":{"type":"string","description":"The URL to fetch"},"max_chars":{"type":"integer","description":"Maximum characters to return (default: 50000)","default":50000}},"required":["url"]})));
        defs.push(ToolDefinition::new("web_search", "Search the web and return results with titles, URLs, and snippets.", serde_json::json!({"type":"object","properties":{"query":{"type":"string","description":"The search query"}},"required":["query"]})));
    }

    defs.push(ToolDefinition::new("manage_tasks", "Manage scheduled tasks. Actions: create, list, get, update, pause, resume, cancel, delete.", serde_json::json!({"type":"object","properties":{"action":{"type":"string","enum":["create","list","get","update","pause","resume","cancel","delete"],"description":"Action to perform"},"task_id":{"type":"string","description":"Task ID (required for get/update/pause/resume/cancel/delete)"},"name":{"type":"string","description":"Task name (required for create, optional for update)"},"prompt":{"type":"string","description":"Prompt to execute (required for create, optional for update)"},"schedule_type":{"type":"string","enum":["cron","interval","once"],"description":"Schedule type (required for create, optional for update)"},"schedule_expr":{"type":"string","description":"Cron expression or interval (required for create, optional for update)"},"timezone":{"type":"string","description":"Timezone (default: local)"}},"required":["action"]})));

    if config.security.host_audit {
        defs.push(ToolDefinition::new(
            "security_audit",
            "Run a host security audit. Returns diagnostic findings about firewall, open ports, SSH config, file permissions, disk encryption, OS updates, and running services. Review findings and suggest fixes — the user must approve each change.",
            serde_json::json!({"type":"object","properties":{"category":{"type":"string","description":"Run only a specific check (omit for all)","enum":["firewall","ports","ssh","permissions","encryption","updates","services"]}}}),
        ));
    }

    defs
}
