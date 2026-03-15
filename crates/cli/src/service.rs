use anyhow::{Context, Result};
use std::path::PathBuf;

use tamagotchi_core::config::Config;

const LAUNCHD_LABEL: &str = "com.tamagotchi.daemon";

const LAUNCHD_PLIST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tamagotchi.daemon</string>
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
Description=Tamagotchi AI Assistant Daemon
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
pub async fn run_daemon() -> Result<()> {
    let config = Config::load()?;

    println!("Tamagotchi daemon starting...");

    // Open database for task scheduling
    let db = tamagotchi_core::db::Database::open()?;

    // Create LLM client for task execution
    let llm = tamagotchi_core::llm::LlmClient::new(config.clone())?;

    println!("Daemon running. Press Ctrl+C to stop.");

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));

    loop {
        interval.tick().await;

        // Check for due tasks
        let now = chrono::Utc::now().timestamp();
        match db.get_due_tasks(now) {
            Ok(tasks) => {
                for task in &tasks {
                    tracing::info!("Executing scheduled task: {} ({})", task.name, task.id);
                    let started_at = chrono::Utc::now().timestamp();

                    let soul = tamagotchi_core::soul::load_soul().unwrap_or_default();
                    let memory =
                        tamagotchi_core::memory::load_memory_context(4000).unwrap_or_default();
                    let time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");

                    let system = format!(
                        "{soul}\n\n# Current Time\n{time}\n\n{memory}\n\n\
                         # Scheduled Task\nYou are executing a scheduled task: \"{}\"\n\
                         Respond with the task result. Be concise.",
                        task.name
                    );

                    let messages = vec![
                        tamagotchi_core::types::Message::system(system),
                        tamagotchi_core::types::Message::user(&task.prompt),
                    ];

                    match llm.chat(&messages, None).await {
                        Ok(response) => {
                            let duration_ms = (chrono::Utc::now().timestamp() - started_at) * 1000;
                            let result_text = response.content.as_deref().unwrap_or("");
                            if let Err(e) = db.record_task_run(
                                &task.id,
                                started_at,
                                duration_ms,
                                Some(result_text),
                                None,
                            ) {
                                tracing::warn!("Failed to record task run: {e}");
                            }
                            tracing::info!(
                                "Task '{}' completed: {}",
                                task.name,
                                &result_text[..result_text.len().min(100)]
                            );
                        }
                        Err(e) => {
                            let duration_ms = (chrono::Utc::now().timestamp() - started_at) * 1000;
                            let err_str = format!("{e}");
                            if let Err(e2) = db.record_task_run(
                                &task.id,
                                started_at,
                                duration_ms,
                                None,
                                Some(&err_str),
                            ) {
                                tracing::warn!("Failed to record task error: {e2}");
                            }
                            tracing::warn!("Task '{}' failed: {e}", task.name);
                        }
                    }

                    // Advance next_run
                    if let Err(e) = tamagotchi_core::tasks::advance_next_run(task, &db) {
                        tracing::warn!("Failed to advance task next_run: {e}");
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to check due tasks: {e}");
            }
        }
    }
}

/// Install the daemon as a system service.
pub fn install_service() -> Result<()> {
    let binary_path = find_binary_path()?;
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    let log_dir = Config::data_dir()?.join("logs");
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

    // Load the service
    let status = std::process::Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .status()
        .context("Failed to run launchctl load")?;

    if status.success() {
        println!("Service installed and loaded: {}", plist_path.display());
        println!("The daemon will start automatically on login.");
    } else {
        println!(
            "Plist written to {} but launchctl load failed.",
            plist_path.display()
        );
        println!("Try: launchctl load {}", plist_path.display());
    }

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

// ── Linux systemd ──

fn systemd_unit_path() -> Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join("tamagotchi.service"))
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

    let status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", "tamagotchi.service"])
        .status()
        .context("Failed to enable service")?;

    if status.success() {
        println!("Service installed and started: {}", unit_path.display());
    } else {
        println!(
            "Unit written to {} but systemctl enable failed.",
            unit_path.display()
        );
    }

    Ok(())
}

fn uninstall_systemd() -> Result<()> {
    let unit_path = systemd_unit_path()?;

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", "tamagotchi.service"])
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
        .args(["--user", "status", "tamagotchi.service"])
        .output()
        .context("Failed to run systemctl status")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("{stdout}");
    Ok(())
}

fn find_binary_path() -> Result<String> {
    // Try to find the tamagotchi binary
    if let Ok(path) = which::which("tamagotchi") {
        return Ok(path.to_string_lossy().to_string());
    }

    // Fall back to current executable
    let exe = std::env::current_exe().context("Could not determine binary path")?;
    Ok(exe.to_string_lossy().to_string())
}
