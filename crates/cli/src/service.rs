use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use borg_core::agent::{Agent, AgentEvent};
use borg_core::config::Config;
use borg_heartbeat::scheduler::{HeartbeatEvent, HeartbeatScheduler};

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

    // Set up heartbeat scheduler
    let (hb_tx, mut hb_rx) = mpsc::channel::<HeartbeatEvent>(32);
    let (wake_tx, wake_rx) = mpsc::channel::<()>(8);

    if config.heartbeat.enabled {
        let tz = config.user_timezone();
        let scheduler = HeartbeatScheduler::new(config.heartbeat.clone(), tz, wake_rx);
        let hb_cancel = shutdown.clone();
        tokio::spawn(async move {
            scheduler.run(hb_tx, hb_cancel).await;
        });
        println!("Heartbeat scheduler started.");
    }

    // Start gateway server (with wake channel for /internal/wake endpoint)
    {
        let gw_config = config.clone();
        let gw_shutdown = shutdown.clone();
        let gw_wake_tx = if config.heartbeat.enabled {
            Some(wake_tx)
        } else {
            None
        };
        tokio::spawn(async move {
            match borg_gateway::GatewayServer::new(
                gw_config,
                gw_shutdown,
                borg_core::telemetry::BorgMetrics::noop(),
                gw_wake_tx,
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
                        match borg_gateway::imessage::start_imessage_monitor(im_config, im_shutdown)
                            .await
                        {
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

    // Missed job catch-up: skip tasks overdue by more than 7 days
    {
        let now = chrono::Utc::now().timestamp();
        if let Ok(stale_tasks) = db.get_due_tasks(now) {
            for task in &stale_tasks {
                let age = now - task.next_run.unwrap_or(0);
                if age > 7 * 86400 {
                    tracing::warn!(
                        "Skipping stale task '{}' ({}d overdue), advancing to next run",
                        task.name,
                        age / 86400
                    );
                    let _ = borg_core::tasks::advance_next_run(task, &db);
                }
            }
        }
    }

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => {
                println!("Daemon shutting down gracefully...");
                for _ in 0..max_concurrent {
                    let _ = semaphore.acquire().await;
                }
                println!("All tasks drained. Goodbye.");
                return Ok(());
            }
            Some(HeartbeatEvent::Fire) = hb_rx.recv() => {
                let hb_config = config.clone();
                tokio::spawn(async move {
                    daemon_heartbeat_turn(hb_config).await;
                });
                continue;
            }
            _ = interval.tick() => {}
        }

        let now = chrono::Utc::now().timestamp();

        // Process due tasks (normal scheduled runs)
        match db.get_due_tasks(now) {
            Ok(tasks) => {
                for task in tasks {
                    // Advance next_run immediately to prevent re-execution
                    if let Err(e) = borg_core::tasks::advance_next_run(&task, &db) {
                        tracing::warn!("Failed to advance task next_run: {e}");
                    }
                    spawn_task_execution(&task, false, semaphore.clone(), config.clone()).await;
                }
            }
            Err(e) => tracing::warn!("Failed to check due tasks: {e}"),
        }

        // Process tasks pending retry
        match db.get_tasks_pending_retry(now) {
            Ok(retries) => {
                for task in retries {
                    spawn_task_execution(&task, true, semaphore.clone(), config.clone()).await;
                }
            }
            Err(e) => tracing::warn!("Failed to check retry tasks: {e}"),
        }
    }
}

async fn spawn_task_execution(
    task: &borg_core::db::ScheduledTaskRow,
    is_retry: bool,
    semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    config: Config,
) {
    let permit = semaphore.acquire_owned().await;
    let task_name = task.name.clone();
    let task_id = task.id.clone();
    let task_prompt = task.prompt.clone();
    let task_timeout = std::time::Duration::from_millis(task.timeout_ms as u64);
    let max_retries = task.max_retries;
    let retry_count = task.retry_count;
    let delivery_channel = task.delivery_channel.clone();
    let delivery_target = task.delivery_target.clone();
    let schedule_type = task.schedule_type.clone();

    tokio::spawn(async move {
        let _permit = permit;
        let attempt_label = if is_retry {
            format!(" (retry {retry_count})")
        } else {
            String::new()
        };
        tracing::info!("Executing scheduled task: {task_name} ({task_id}){attempt_label}");
        let started_at = chrono::Utc::now().timestamp();

        let identity = borg_core::identity::load_identity().unwrap_or_default();
        let memory = borg_core::memory::load_memory_context(4000).unwrap_or_default();
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

        let llm = match borg_core::llm::LlmClient::new(config.clone()) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("Failed to create LLM client for task '{task_name}': {e}");
                return;
            }
        };
        let result = tokio::time::timeout(task_timeout, llm.chat(&messages, None)).await;

        match result {
            Ok(Ok(response)) => {
                let duration_ms = (chrono::Utc::now().timestamp() - started_at) * 1000;
                let result_text = response.text_content().unwrap_or("");
                if let Ok(db) = borg_core::db::Database::open() {
                    if let Err(e) = db.record_task_run(
                        &task_id,
                        started_at,
                        duration_ms,
                        Some(result_text),
                        None,
                    ) {
                        tracing::warn!("Failed to record task run for '{task_name}': {e}");
                    }
                    if let Err(e) = db.clear_task_retry(&task_id) {
                        tracing::warn!("Failed to clear retry state for '{task_name}': {e}");
                    }
                }
                tracing::info!(
                    "Task '{task_name}' completed: {}",
                    &result_text[..result_text.len().min(100)]
                );
                if let (Some(ch), Some(tgt)) = (&delivery_channel, &delivery_target) {
                    deliver_task_result(ch, tgt, &task_name, result_text, &config).await;
                }
            }
            Ok(Err(e)) => {
                let duration_ms = (chrono::Utc::now().timestamp() - started_at) * 1000;
                let err_str = format!("{e}");
                handle_task_failure(
                    &task_id,
                    &task_name,
                    &err_str,
                    started_at,
                    duration_ms,
                    retry_count,
                    max_retries,
                    &schedule_type,
                    &delivery_channel,
                    &delivery_target,
                    &config,
                )
                .await;
            }
            Err(_) => {
                let duration_ms = (chrono::Utc::now().timestamp() - started_at) * 1000;
                handle_task_failure(
                    &task_id,
                    &task_name,
                    "Task timed out",
                    started_at,
                    duration_ms,
                    retry_count,
                    max_retries,
                    &schedule_type,
                    &delivery_channel,
                    &delivery_target,
                    &config,
                )
                .await;
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn handle_task_failure(
    task_id: &str,
    task_name: &str,
    error: &str,
    started_at: i64,
    duration_ms: i64,
    retry_count: i32,
    max_retries: i32,
    schedule_type: &str,
    delivery_channel: &Option<String>,
    delivery_target: &Option<String>,
    config: &Config,
) {
    if borg_core::tasks::is_transient_error(error) && retry_count < max_retries {
        let next_attempt = retry_count + 1;
        let delay = borg_core::tasks::retry_delay_secs(next_attempt);
        let retry_at = chrono::Utc::now().timestamp() + delay;
        if let Ok(db) = borg_core::db::Database::open() {
            if let Err(e) = db.set_task_retry(task_id, next_attempt, error, retry_at) {
                tracing::warn!("Failed to set retry state for '{task_name}': {e}");
            }
        }
        tracing::warn!(
            "Task '{task_name}' failed (transient), retry {next_attempt}/{max_retries} in {delay}s: {error}"
        );
    } else {
        if let Ok(db) = borg_core::db::Database::open() {
            if let Err(e) = db.record_task_run(task_id, started_at, duration_ms, None, Some(error))
            {
                tracing::warn!("Failed to record task failure for '{task_name}': {e}");
            }
            if let Err(e) = db.clear_task_retry(task_id) {
                tracing::warn!("Failed to clear retry state for '{task_name}': {e}");
            }
            if schedule_type == "once" {
                if let Err(e) = db.update_task_status(task_id, "completed") {
                    tracing::warn!("Failed to mark task '{task_name}' completed: {e}");
                }
            }
        }
        tracing::warn!("Task '{task_name}' failed: {error}");

        if let (Some(ch), Some(tgt)) = (delivery_channel, delivery_target) {
            let msg = format!("Error: {error}");
            deliver_task_result(ch, tgt, &format!("{task_name} [FAILED]"), &msg, config).await;
        }
    }
}

async fn deliver_task_result(
    channel: &str,
    target: &str,
    task_name: &str,
    text: &str,
    config: &Config,
) {
    let msg = format!("[Task: {task_name}]\n{text}");
    let result = match channel {
        "telegram" => {
            let token = config.resolve_credential_or_env("TELEGRAM_BOT_TOKEN");
            match (token, target.parse::<i64>()) {
                (Some(t), Ok(chat_id)) => {
                    match borg_gateway::telegram::api::TelegramClient::new(&t) {
                        Ok(client) => client.send_message(chat_id, &msg, None, None, None).await,
                        Err(e) => Err(e),
                    }
                }
                _ => {
                    tracing::warn!("Telegram delivery: missing token or invalid chat_id");
                    return;
                }
            }
        }
        "slack" => {
            let token = config.resolve_credential_or_env("SLACK_BOT_TOKEN");
            match token {
                Some(t) => match borg_gateway::slack::api::SlackClient::new(&t) {
                    Ok(client) => client.post_message(target, &msg, None).await.map(|_| ()),
                    Err(e) => Err(e),
                },
                None => {
                    tracing::warn!("Slack delivery: missing SLACK_BOT_TOKEN");
                    return;
                }
            }
        }
        "discord" => {
            let token = config.resolve_credential_or_env("DISCORD_BOT_TOKEN");
            match token {
                Some(t) => match borg_gateway::discord::api::DiscordClient::new(&t) {
                    Ok(client) => client.send_message(target, &msg).await,
                    Err(e) => Err(e),
                },
                None => {
                    tracing::warn!("Discord delivery: missing DISCORD_BOT_TOKEN");
                    return;
                }
            }
        }
        _ => {
            tracing::warn!("Unknown delivery channel: {channel}");
            return;
        }
    };
    if let Err(e) = result {
        tracing::warn!("Failed to deliver task result via {channel}: {e}");
    }
}

/// Run a heartbeat agent turn in the daemon and deliver to configured channels.
/// Shared heartbeat turn: creates a temporary agent, sends the heartbeat message
/// (with HEARTBEAT.md checklist if present), deduplicates, and returns the response.
pub async fn execute_heartbeat_turn(config: &Config) -> Option<String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static LAST_HASH: AtomicU64 = AtomicU64::new(0);

    let metrics = borg_core::telemetry::BorgMetrics::noop();
    let mut agent = match Agent::new(config.clone(), metrics) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("Heartbeat: failed to create agent: {e}");
            return None;
        }
    };

    let checklist = borg_core::memory::load_heartbeat_checklist();
    let mut user_msg = "*heartbeat tick*".to_string();
    if let Some(ref cl) = checklist {
        user_msg.push_str("\n\n# Heartbeat Checklist\n");
        user_msg.push_str(cl);
    }

    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(256);
    let cancel = CancellationToken::new();

    if let Err(e) = agent
        .send_message_with_cancel(&user_msg, event_tx, cancel)
        .await
    {
        tracing::warn!("Heartbeat: agent error: {e}");
        return None;
    }

    let mut response = String::new();
    while let Some(event) = event_rx.recv().await {
        if let AgentEvent::TextDelta(delta) = event {
            response.push_str(&delta);
        }
    }

    let trimmed = response.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }

    // Dedup: skip if identical to last heartbeat response
    let mut hasher = DefaultHasher::new();
    trimmed.hash(&mut hasher);
    let hash = hasher.finish();
    let prev = LAST_HASH.swap(hash, Ordering::Relaxed);
    if prev == hash {
        tracing::debug!("Heartbeat: duplicate response, suppressing");
        return None;
    }

    Some(trimmed)
}

/// Run a heartbeat turn in the daemon and deliver to configured channels.
async fn daemon_heartbeat_turn(config: Config) {
    if let Some(text) = execute_heartbeat_turn(&config).await {
        tracing::info!("Heartbeat: {}", &text[..text.len().min(100)]);
        deliver_heartbeat_to_channels(&config, &text).await;
    }
}

/// Send a heartbeat message to all configured channels.
async fn deliver_heartbeat_to_channels(config: &Config, text: &str) {
    for channel_name in &config.heartbeat.channels {
        if let Err(e) = deliver_to_channel(config, channel_name, text).await {
            tracing::warn!("Heartbeat delivery to {channel_name} failed: {e}");
        }
    }
}

/// Deliver a heartbeat message to a single channel using native clients.
async fn deliver_to_channel(config: &Config, channel_name: &str, text: &str) -> Result<()> {
    let db = borg_core::db::Database::open()?;

    // Find the owner's sender_id from approved_senders for this channel
    let approved = db.list_approved_senders(Some(channel_name))?;
    if approved.is_empty() {
        tracing::debug!("Heartbeat: no approved senders for {channel_name}, skipping");
        return Ok(());
    }

    let sender_id = &approved[0].sender_id;

    match channel_name {
        "telegram" => {
            let token = config
                .resolve_credential_or_env("TELEGRAM_BOT_TOKEN")
                .ok_or_else(|| anyhow::anyhow!("TELEGRAM_BOT_TOKEN not configured"))?;
            let client = borg_gateway::telegram::api::TelegramClient::new(&token)?;
            let chat_id: i64 = sender_id
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid Telegram chat_id: {sender_id}"))?;
            client.send_message(chat_id, text, None, None, None).await?;
            tracing::info!("Heartbeat delivered to telegram:{sender_id}");
        }
        "slack" => {
            let token = config
                .resolve_credential_or_env("SLACK_BOT_TOKEN")
                .ok_or_else(|| anyhow::anyhow!("SLACK_BOT_TOKEN not configured"))?;
            let client = borg_gateway::slack::api::SlackClient::new(&token)?;
            client.post_message(sender_id, text, None).await?;
            tracing::info!("Heartbeat delivered to slack:{sender_id}");
        }
        other => {
            tracing::debug!("Heartbeat: channel '{other}' not supported for native delivery");
        }
    }

    Ok(())
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
