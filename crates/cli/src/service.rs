use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

use borg_core::config::Config;

const LAUNCHD_LABEL: &str = "com.borg.daemon";

const LAUNCHD_PLIST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.borg.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{{BINARY_PATH}}</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{{LOG_DIR}}/daemon.log</string>
    <key>StandardErrorPath</key>
    <string>{{LOG_DIR}}/daemon.err</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{{HOME}}</string>
    </dict>
</dict>
</plist>"#;

const SYSTEMD_UNIT_TEMPLATE: &str = r#"[Unit]
Description=Borg AI Assistant Daemon
After=network.target

[Service]
Type=simple
ExecStart={{BINARY_PATH}} daemon
Restart=on-failure
RestartSec=5
Environment=HOME={{HOME}}

[Install]
WantedBy=default.target
"#;

/// Run the daemon loop: executes scheduled tasks and heartbeat without a TUI.
pub async fn run_daemon(shutdown: CancellationToken) -> Result<()> {
    let config = Config::load()?;

    println!("Borg daemon starting...");

    // Open database for task scheduling
    let db = borg_core::db::Database::open()?;

    // Validate that LLM client can be constructed
    let _ = borg_core::llm::LlmClient::new(config.clone())?;

    let max_concurrent = config.tasks.max_concurrent;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrent));

    // Start gateway server
    {
        let gw_config = config.clone();
        let gw_shutdown = shutdown.clone();
        tokio::spawn(async move {
            match borg_gateway::GatewayServer::new(
                gw_config,
                gw_shutdown,
                borg_core::telemetry::BorgMetrics::noop(),
            ) {
                Ok(gateway) => {
                    if let Err(e) = gateway.run().await {
                        let msg = e.to_string();
                        if msg.contains("address already in use") || msg.contains("AddrInUse") {
                            tracing::warn!("Gateway: {e}");
                        } else {
                            tracing::error!("Gateway server error: {e}");
                        }
                    }
                }
                Err(e) => tracing::error!("Failed to create gateway server: {e}"),
            }
        });
        println!("Gateway server started.");
    }

    // Start native iMessage monitor if channel is installed (macOS only)
    #[cfg(target_os = "macos")]
    {
        let imessage_dir = Config::data_dir()?.join("channels/imessage");
        if imessage_dir.join("channel.toml").exists() {
            let probe = borg_gateway::imessage::probe::probe_imessage();
            match probe.status {
                borg_gateway::imessage::probe::ProbeStatus::Ok => {
                    let im_config = config.clone();
                    let im_shutdown = shutdown.clone();
                    tokio::spawn(async move {
                        match borg_gateway::imessage::start_imessage_monitor(im_config, im_shutdown).await {
                            Ok(_handle) => tracing::info!("iMessage monitor started"),
                            Err(e) => tracing::warn!("iMessage monitor failed: {e}"),
                        }
                    });
                }
                borg_gateway::imessage::probe::ProbeStatus::NoDiskAccess => {
                    tracing::warn!("iMessage: Full Disk Access required (System Settings > Privacy & Security). Skipping monitor.");
                }
                other => {
                    tracing::warn!("iMessage probe: {other}. Skipping monitor.");
                }
            }
        }
    }

    println!("Daemon running. Press Ctrl+C to stop.");

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                println!("Daemon shutting down gracefully...");
                // Wait for in-progress tasks to finish (acquire all permits)
                for _ in 0..max_concurrent {
                    let _ = semaphore.acquire().await;
                }
                println!("All tasks drained. Goodbye.");
                return Ok(());
            }
            _ = interval.tick() => {}
        }

        let now = chrono::Utc::now().timestamp();
        match db.get_due_tasks(now) {
            Ok(tasks) => {
                for task in tasks {
                    // Advance next_run immediately to prevent re-execution
                    if let Err(e) = borg_core::tasks::advance_next_run(&task, &db) {
                        tracing::warn!("Failed to advance task next_run: {e}");
                    }

                    let permit = semaphore.clone().acquire_owned().await;
                    let task_config = config.clone();
                    let task_name = task.name.clone();
                    let task_id = task.id.clone();
                    let task_prompt = task.prompt.clone();
                    let task_timeout = std::time::Duration::from_secs(300); // 5 min per task

                    tokio::spawn(async move {
                        let _permit = permit;
                        tracing::info!("Executing scheduled task: {task_name} ({task_id})");
                        let started_at = chrono::Utc::now().timestamp();

                        let identity = borg_core::identity::load_identity().unwrap_or_default();
                        let memory =
                            borg_core::memory::load_memory_context(4000).unwrap_or_default();
                        let time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");

                        let system = format!(
                            "{identity}\n\n# Current Time\n{time}\n\n{memory}\n\n\
                             # Scheduled Task\nYou are executing a scheduled task: \"{task_name}\"\n\
                             Respond with the task result. Be concise."
                        );

                        let messages = vec![
                            borg_core::types::Message::system(system),
                            borg_core::types::Message::user(&task_prompt),
                        ];

                        let llm = match borg_core::llm::LlmClient::new(task_config) {
                            Ok(l) => l,
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to create LLM client for task '{task_name}': {e}"
                                );
                                return;
                            }
                        };
                        let result =
                            tokio::time::timeout(task_timeout, llm.chat(&messages, None)).await;

                        if let Ok(db) = borg_core::db::Database::open() {
                            match result {
                                Ok(Ok(response)) => {
                                    let duration_ms =
                                        (chrono::Utc::now().timestamp() - started_at) * 1000;
                                    let result_text = response.text_content().unwrap_or("");
                                    let _ = db.record_task_run(
                                        &task_id,
                                        started_at,
                                        duration_ms,
                                        Some(result_text),
                                        None,
                                    );
                                    tracing::info!(
                                        "Task '{task_name}' completed: {}",
                                        &result_text[..result_text.len().min(100)]
                                    );
                                }
                                Ok(Err(e)) => {
                                    let duration_ms =
                                        (chrono::Utc::now().timestamp() - started_at) * 1000;
                                    let err_str = format!("{e}");
                                    let _ = db.record_task_run(
                                        &task_id,
                                        started_at,
                                        duration_ms,
                                        None,
                                        Some(&err_str),
                                    );
                                    tracing::warn!("Task '{task_name}' failed: {e}");
                                }
                                Err(_) => {
                                    let duration_ms =
                                        (chrono::Utc::now().timestamp() - started_at) * 1000;
                                    let _ = db.record_task_run(
                                        &task_id,
                                        started_at,
                                        duration_ms,
                                        None,
                                        Some("Task timed out"),
                                    );
                                    tracing::warn!("Task '{task_name}' timed out");
                                }
                            }
                        }
                    });
                }
            }
            Err(e) => {
                tracing::warn!("Failed to check due tasks: {e}");
            }
        }
    }
}

/// Ensure the daemon service is installed and running.
pub fn ensure_service_running() -> Result<()> {
    ensure_service_installed()?;

    let is_running = if cfg!(target_os = "macos") {
        std::process::Command::new("launchctl")
            .args(["list", LAUNCHD_LABEL])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", "borg.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        return Ok(());
    };

    if !is_running {
        if cfg!(target_os = "macos") {
            if let Ok(plist) = launchd_plist_path() {
                let _ = std::process::Command::new("launchctl")
                    .args(["load", &plist.to_string_lossy()])
                    .status();
            }
        } else if cfg!(target_os = "linux") {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "start", "borg.service"])
                .status();
        }
    }

    Ok(())
}

/// Ensure the daemon service is installed, installing silently if needed.
pub fn ensure_service_installed() -> Result<()> {
    let already_installed = if cfg!(target_os = "macos") {
        launchd_plist_path().map(|p| p.exists()).unwrap_or(false)
    } else if cfg!(target_os = "linux") {
        systemd_unit_path().map(|p| p.exists()).unwrap_or(false)
    } else {
        return Ok(());
    };

    if !already_installed {
        install_service_inner()?;
    }
    Ok(())
}

fn install_service_inner() -> Result<()> {
    let binary_path = find_binary_path()?;
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    let log_dir = Config::logs_dir()?;
    std::fs::create_dir_all(&log_dir)?;

    if cfg!(target_os = "macos") {
        install_launchd(&binary_path, &home, &log_dir)
    } else if cfg!(target_os = "linux") {
        install_systemd(&binary_path, &home)
    } else {
        anyhow::bail!("Service installation is only supported on macOS and Linux")
    }
}

/// Uninstall the daemon service.
pub fn uninstall_service() -> Result<()> {
    if cfg!(target_os = "macos") {
        uninstall_launchd()
    } else if cfg!(target_os = "linux") {
        uninstall_systemd()
    } else {
        anyhow::bail!("Service management is only supported on macOS and Linux")
    }
}

/// Stop the daemon service without uninstalling it.
pub fn stop_service() -> Result<()> {
    if cfg!(target_os = "macos") {
        stop_launchd()
    } else if cfg!(target_os = "linux") {
        stop_systemd()
    } else {
        anyhow::bail!("Service management is only supported on macOS and Linux")
    }
}

/// Restart the daemon service.
pub fn restart_service() -> Result<()> {
    if cfg!(target_os = "macos") {
        restart_launchd()
    } else if cfg!(target_os = "linux") {
        restart_systemd()
    } else {
        anyhow::bail!("Service management is only supported on macOS and Linux")
    }
}

/// Show the daemon service status.
pub fn service_status() -> Result<()> {
    if cfg!(target_os = "macos") {
        status_launchd()
    } else if cfg!(target_os = "linux") {
        status_systemd()
    } else {
        anyhow::bail!("Service management is only supported on macOS and Linux")
    }
}

// ── macOS launchd ──

fn launchd_plist_path() -> Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist")))
}

fn install_launchd(
    binary_path: &str,
    home: &std::path::Path,
    log_dir: &std::path::Path,
) -> Result<()> {
    let plist_path = launchd_plist_path()?;
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = LAUNCHD_PLIST_TEMPLATE
        .replace("{{BINARY_PATH}}", binary_path)
        .replace("{{LOG_DIR}}", &log_dir.to_string_lossy())
        .replace("{{HOME}}", &home.to_string_lossy());

    std::fs::write(&plist_path, &content)
        .with_context(|| format!("Failed to write plist to {}", plist_path.display()))?;

    let _ = std::process::Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .status()
        .context("Failed to run launchctl load")?;

    Ok(())
}

fn uninstall_launchd() -> Result<()> {
    let plist_path = launchd_plist_path()?;

    if plist_path.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .status();
        std::fs::remove_file(&plist_path)?;
        println!("Service uninstalled: {}", plist_path.display());
    } else {
        println!("Service not installed (no plist found).");
    }
    Ok(())
}

fn status_launchd() -> Result<()> {
    let output = std::process::Command::new("launchctl")
        .args(["list", LAUNCHD_LABEL])
        .output()
        .context("Failed to run launchctl list")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("Service status: running");
        println!("{stdout}");
    } else {
        println!("Service status: not running");
    }
    Ok(())
}

fn stop_launchd() -> Result<()> {
    ensure_service_installed()?;
    let plist_path = launchd_plist_path()?;
    let status = std::process::Command::new("launchctl")
        .args(["unload", &plist_path.to_string_lossy()])
        .status()
        .context("Failed to run launchctl unload")?;
    if status.success() {
        println!("Service stopped.");
    } else {
        println!("Failed to stop service (it may not be running).");
    }
    Ok(())
}

fn restart_launchd() -> Result<()> {
    ensure_service_installed()?;
    let plist_path = launchd_plist_path()?;
    let _ = std::process::Command::new("launchctl")
        .args(["unload", &plist_path.to_string_lossy()])
        .status();
    let status = std::process::Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .status()
        .context("Failed to run launchctl load")?;
    if status.success() {
        println!("Service restarted.");
    } else {
        println!("Failed to restart service.");
    }
    Ok(())
}

// ── Linux systemd ──

fn systemd_unit_path() -> Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join("borg.service"))
}

fn install_systemd(binary_path: &str, home: &std::path::Path) -> Result<()> {
    let unit_path = systemd_unit_path()?;
    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = SYSTEMD_UNIT_TEMPLATE
        .replace("{{BINARY_PATH}}", binary_path)
        .replace("{{HOME}}", &home.to_string_lossy());

    std::fs::write(&unit_path, &content)
        .with_context(|| format!("Failed to write unit to {}", unit_path.display()))?;

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", "borg.service"])
        .status()
        .context("Failed to enable service")?;

    Ok(())
}

fn uninstall_systemd() -> Result<()> {
    let unit_path = systemd_unit_path()?;

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", "borg.service"])
        .status();

    if unit_path.exists() {
        std::fs::remove_file(&unit_path)?;
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        println!("Service uninstalled: {}", unit_path.display());
    } else {
        println!("Service not installed.");
    }
    Ok(())
}

fn status_systemd() -> Result<()> {
    let output = std::process::Command::new("systemctl")
        .args(["--user", "status", "borg.service"])
        .output()
        .context("Failed to run systemctl status")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("{stdout}");
    Ok(())
}

fn stop_systemd() -> Result<()> {
    ensure_service_installed()?;
    let status = std::process::Command::new("systemctl")
        .args(["--user", "stop", "borg.service"])
        .status()
        .context("Failed to run systemctl stop")?;
    if status.success() {
        println!("Service stopped.");
    } else {
        println!("Failed to stop service (it may not be running).");
    }
    Ok(())
}

fn restart_systemd() -> Result<()> {
    ensure_service_installed()?;
    let status = std::process::Command::new("systemctl")
        .args(["--user", "restart", "borg.service"])
        .status()
        .context("Failed to run systemctl restart")?;
    if status.success() {
        println!("Service restarted.");
    } else {
        println!("Failed to restart service.");
    }
    Ok(())
}

fn find_binary_path() -> Result<String> {
    // Prefer current executable to avoid finding a different `borg` binary (e.g., BorgBackup)
    if let Ok(exe) = std::env::current_exe() {
        return Ok(exe.to_string_lossy().to_string());
    }

    // Fall back to PATH lookup
    if let Ok(path) = which::which("borg") {
        return Ok(path.to_string_lossy().to_string());
    }

    anyhow::bail!("Could not determine binary path")
}
