use std::io::IsTerminal;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

mod credentials;
mod logo;
mod migrate_tui;
mod onboarding;
mod onboarding_tui;
mod plugins;
mod repl;
mod service;
mod tui;

/// Format a Unix timestamp for display. Returns "?" if invalid.
fn format_ts(ts: i64, fmt: &str) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format(fmt).to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// Return the first 8 characters of an ID for compact display.
fn short_id(id: &str) -> &str {
    &id[..8.min(id.len())]
}

#[derive(Parser)]
#[command(name = "borg", about = "AI Personal Assistant Agent", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start Borg — interactive TUI with auto-gateway (default)
    Start,
    /// Stop the background daemon service
    Stop,
    /// Restart the background daemon service
    Restart,
    /// Run interactive setup wizard for ~/.borg
    Init,
    /// Send a single message and exit
    Ask {
        /// The message to send
        message: String,
        /// Auto-approve all tool/shell calls (no user prompts)
        #[arg(long, short)]
        yes: bool,
        /// Output raw JSON instead of streaming text
        #[arg(long, short)]
        json: bool,
        /// Collaboration mode: default, execute, plan
        #[arg(long, short)]
        mode: Option<String>,
    },
    /// Show agent vitals and status
    Status {
        #[command(subcommand)]
        action: Option<StatusAction>,
    },
    /// Show bond status and trust metrics
    Bond {
        #[command(subcommand)]
        action: Option<BondAction>,
    },
    /// Run diagnostics to check configuration, connectivity, and dependencies
    Doctor,
    /// Run as a background daemon (executes scheduled tasks and heartbeat)
    Daemon,
    /// Manage the daemon as a system service
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Set up an integration (e.g. borg add telegram)
    Add {
        /// Integration name
        name: String,
    },
    /// Remove an integration's credentials
    Remove {
        /// Integration name
        name: String,
    },
    /// List all available integrations and their status
    Plugins,
    /// Show or update configuration settings
    Settings {
        #[command(subcommand)]
        action: Option<SettingsAction>,
    },
    /// Show recent conversation history
    Logs {
        /// Number of log entries to show
        #[arg(long, short, default_value_t = 50)]
        count: usize,
        /// Show full untruncated content
        #[arg(long, short)]
        verbose: bool,
    },
    /// Manage scheduled tasks
    Tasks {
        #[command(subcommand)]
        action: Option<TasksAction>,
    },
    /// Show token usage and cost breakdown for the current month
    Usage,
    /// Manage sender pairing and access control for messaging channels
    Pairing {
        #[command(subcommand)]
        action: Option<PairingAction>,
    },
    /// Trigger an immediate heartbeat check-in
    Wake,
    /// Set the agent to away mode (auto-replies to messages)
    Away {
        /// Custom away message (uses config default if omitted)
        message: Option<String>,
    },
    /// Set the agent back to available mode (stops auto-replying)
    Available,
    /// Migrate settings from Hermes Agent or OpenClaw
    Migrate,
    /// Permanently delete all Borg data and uninstall the service
    Uninstall,
}

#[derive(Subcommand)]
enum BondAction {
    /// Show recent bond event history
    History {
        /// Number of events to show
        #[arg(long, short, default_value_t = 20)]
        count: usize,
    },
}

#[derive(Subcommand)]
enum StatusAction {
    /// Show evolution history timeline
    History,
    /// Show archetype score breakdown
    Archetypes,
}

#[derive(Subcommand)]
enum SettingsAction {
    /// Update a configuration setting (writes to DB, not config.toml)
    Set {
        /// Setting key (e.g. temperature, model, sandbox.enabled)
        key: String,
        /// New value
        value: String,
    },
    /// Show the effective value and source for a setting
    Get {
        /// Setting key
        key: String,
    },
    /// Remove a DB override, reverting to config.toml or default
    Unset {
        /// Setting key
        key: String,
    },
}

#[derive(Subcommand)]
enum TasksAction {
    /// List all scheduled tasks
    List,
    /// Create a new scheduled task
    Create {
        /// Task name
        #[arg(long, short)]
        name: String,
        /// Prompt to send to the agent
        #[arg(long, short)]
        prompt: String,
        /// Schedule expression (cron or interval)
        #[arg(long, short)]
        schedule: String,
        /// Schedule type: cron, interval, or once
        #[arg(long, short = 't', default_value = "cron")]
        r#type: String,
        /// Max retry attempts for transient failures (default: 3)
        #[arg(long)]
        max_retries: Option<i32>,
        /// Timeout in seconds (default: 300)
        #[arg(long)]
        timeout: Option<u64>,
        /// Delivery channel for results (telegram, slack, discord)
        #[arg(long)]
        delivery_channel: Option<String>,
        /// Delivery target (chat_id or channel_id)
        #[arg(long)]
        delivery_target: Option<String>,
    },
    /// Delete a scheduled task
    Delete {
        /// Task ID (or prefix)
        id: String,
    },
    /// Pause a scheduled task
    Pause {
        /// Task ID (or prefix)
        id: String,
    },
    /// Resume a paused task
    Resume {
        /// Task ID (or prefix)
        id: String,
    },
    /// Trigger a task to run immediately
    Run {
        /// Task ID (or prefix)
        id: String,
    },
    /// Show execution history for a task
    Runs {
        /// Task ID (or prefix)
        id: String,
        /// Number of runs to show
        #[arg(long, short, default_value_t = 10)]
        count: usize,
    },
    /// Show detailed task status
    Status {
        /// Task ID (or prefix)
        id: String,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Uninstall the daemon service
    Uninstall,
    /// Show the daemon service status
    Status,
}

#[derive(Subcommand)]
enum PairingAction {
    /// List pending pairing requests
    List {
        /// Filter by channel name
        channel: Option<String>,
    },
    /// Approve a pairing request by code
    Approve {
        /// Channel name (e.g. telegram, slack, discord)
        channel: String,
        /// Pairing code (8 characters)
        code: String,
    },
    /// Revoke an approved sender
    Revoke {
        /// Channel name
        channel: String,
        /// Sender ID to revoke
        sender_id: String,
    },
    /// List all approved senders
    Approved {
        /// Filter by channel name
        channel: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    // Also load .env from the data directory (~/.borg/.env)
    if let Ok(data_dir) = borg_core::config::Config::data_dir() {
        let _ = dotenvy::from_path(data_dir.join(".env"));
        // Harden data directory permissions (owner-only access)
        harden_data_dir(&data_dir);
    }

    let cli = Cli::parse();

    let tui_mode = matches!(cli.command, Some(Commands::Start) | None);

    // _guard and _telemetry_guard must live for the program's duration to flush logs
    let _guard;
    let _telemetry_guard;

    {
        let config = borg_core::config::Config::load().unwrap_or_default();
        let (otel_layer, tg) = borg_core::telemetry::init_telemetry(&config.telemetry)
            .unwrap_or_else(|e| {
                eprintln!("Warning: OpenTelemetry init failed: {e}");
                (None, borg_core::telemetry::TelemetryGuard::noop())
            });
        _telemetry_guard = tg;

        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("warn,chromiumoxide=error,tungstenite=error"));

        if tui_mode {
            let (non_blocking, guard) = match borg_core::config::Config::logs_dir() {
                Ok(log_dir) => {
                    let _ = std::fs::create_dir_all(&log_dir);
                    let mut opts = std::fs::OpenOptions::new();
                    opts.create(true).append(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        opts.mode(0o600);
                    }
                    match opts.open(log_dir.join("tui.log")) {
                        Ok(f) => tracing_appender::non_blocking(f),
                        Err(e) => {
                            eprintln!("Warning: could not open log file: {e}");
                            tracing_appender::non_blocking(std::io::sink())
                        }
                    }
                }
                Err(_) => tracing_appender::non_blocking(std::io::sink()),
            };
            _guard = Some(guard);
            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(non_blocking);
            tracing_subscriber::registry()
                .with(otel_layer)
                .with(env_filter)
                .with(fmt_layer)
                .init();
        } else {
            _guard = None;
            let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);
            tracing_subscriber::registry()
                .with(otel_layer)
                .with(env_filter)
                .with(fmt_layer)
                .init();
        }
    }

    // Set up a global cancellation token for graceful shutdown
    let shutdown = CancellationToken::new();
    {
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let Ok(mut sigterm) = signal(SignalKind::terminate()) else {
                    // If SIGTERM handler fails, just listen for ctrl_c
                    let _ = ctrl_c.await;
                    shutdown.cancel();
                    return;
                };
                tokio::select! {
                    _ = ctrl_c => {},
                    _ = sigterm.recv() => {},
                }
            }
            #[cfg(not(unix))]
            {
                let _ = ctrl_c.await;
            }
            shutdown.cancel();
        });
    }

    // Ensure daemon service is installed and running on every command
    // (skip commands that manage the service themselves)
    if !matches!(
        cli.command,
        Some(Commands::Daemon)
            | Some(Commands::Init)
            | Some(Commands::Uninstall)
            | Some(Commands::Service { .. })
    ) {
        if let Err(e) = service::ensure_service_running() {
            tracing::warn!("Auto-start service: {e}");
        }
    }

    match cli.command {
        Some(Commands::Start) | None => {
            ensure_onboarded()?;
            repl::run().await?;
        }
        Some(Commands::Stop) => service::stop_service()?,
        Some(Commands::Restart) => service::restart_service()?,
        Some(Commands::Init) => init_data_dir()?,
        Some(Commands::Ask {
            message,
            yes,
            json,
            mode,
        }) => repl::one_shot(&message, yes, json, mode.as_deref()).await?,
        Some(Commands::Status { action }) => match action {
            None => run_status()?,
            Some(StatusAction::History) => run_status_history()?,
            Some(StatusAction::Archetypes) => run_status_archetypes()?,
        },
        Some(Commands::Bond { action }) => match action {
            Some(BondAction::History { count }) => run_bond_history(count)?,
            None => run_bond_status()?,
        },
        Some(Commands::Doctor) => run_doctor()?,
        Some(Commands::Daemon) => service::run_daemon(shutdown).await?,
        Some(Commands::Service { action }) => match action {
            ServiceAction::Uninstall => service::uninstall_service()?,
            ServiceAction::Status => service::service_status()?,
        },
        Some(Commands::Add { name }) => plugins::add_plugin(&name)?,
        Some(Commands::Remove { name }) => plugins::remove_plugin(&name)?,
        Some(Commands::Plugins) => plugins::list_plugins()?,
        Some(Commands::Settings { action }) => match action {
            Some(SettingsAction::Set { key, value }) => run_settings_set(&key, &value)?,
            Some(SettingsAction::Get { key }) => run_settings_get(&key)?,
            Some(SettingsAction::Unset { key }) => run_settings_unset(&key)?,
            None => run_settings_show()?,
        },
        Some(Commands::Logs { count, verbose }) => run_logs(count, verbose)?,
        Some(Commands::Tasks { action }) => match action {
            Some(TasksAction::List) | None => run_tasks_list()?,
            Some(TasksAction::Create {
                name,
                prompt,
                schedule,
                r#type,
                max_retries,
                timeout,
                delivery_channel,
                delivery_target,
            }) => run_tasks_create(
                &name,
                &prompt,
                &schedule,
                &r#type,
                max_retries,
                timeout.map(|s| s as i64 * 1000),
                delivery_channel.as_deref(),
                delivery_target.as_deref(),
            )?,
            Some(TasksAction::Delete { id }) => run_tasks_delete(&id)?,
            Some(TasksAction::Pause { id }) => run_tasks_update_status(&id, "paused")?,
            Some(TasksAction::Resume { id }) => run_tasks_update_status(&id, "active")?,
            Some(TasksAction::Run { id }) => run_tasks_run(&id)?,
            Some(TasksAction::Runs { id, count }) => run_tasks_runs(&id, count)?,
            Some(TasksAction::Status { id }) => run_tasks_status(&id)?,
        },
        Some(Commands::Usage) => run_usage()?,
        Some(Commands::Pairing { action }) => match action {
            Some(PairingAction::List { channel }) => run_pairing_list(channel.as_deref())?,
            Some(PairingAction::Approve { channel, code }) => run_pairing_approve(&channel, &code)?,
            Some(PairingAction::Revoke { channel, sender_id }) => {
                run_pairing_revoke(&channel, &sender_id)?
            }
            Some(PairingAction::Approved { channel }) => run_pairing_approved(channel.as_deref())?,
            None => run_pairing_list(None)?,
        },
        Some(Commands::Wake) => run_wake().await?,
        Some(Commands::Away { message }) => run_away(message).await?,
        Some(Commands::Available) => run_available().await?,
        Some(Commands::Migrate) => migrate_tui::run()?,
        Some(Commands::Uninstall) => run_uninstall()?,
    }

    Ok(())
}

fn ensure_onboarded() -> Result<()> {
    let data_dir = borg_core::config::Config::data_dir()?;
    let config_path = data_dir.join("config.toml");
    if !config_path.exists() {
        init_data_dir()?;
    }
    Ok(())
}

async fn run_wake() -> Result<()> {
    let config = borg_core::config::Config::load()?;
    let url = format!(
        "http://{}:{}/internal/wake",
        config.gateway.host, config.gateway.port
    );
    let client = reqwest::Client::new();
    match client
        .post(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => println!("Wake signal sent."),
        Ok(r) => println!("Wake failed: {}", r.status()),
        Err(_) => {
            println!("Could not reach daemon. Is it running?");
            println!("Start it with: borg service start");
        }
    }
    Ok(())
}

async fn run_away(message: Option<String>) -> Result<()> {
    let config = borg_core::config::Config::load()?;
    let url = format!(
        "http://{}:{}/internal/away",
        config.gateway.host, config.gateway.port
    );
    let client = reqwest::Client::new();
    let mut req = client.post(&url).timeout(std::time::Duration::from_secs(5));
    if let Some(msg) = &message {
        req = req
            .header("Content-Type", "application/json")
            .body(serde_json::json!({"message": msg}).to_string());
    }
    match req.send().await {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            let msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("(default)");
            println!("Agent set to away: {msg}");
        }
        Ok(r) => println!("Away failed: {}", r.status()),
        Err(_) => {
            println!("Could not reach gateway. Is it running?");
        }
    }
    Ok(())
}

async fn run_available() -> Result<()> {
    let config = borg_core::config::Config::load()?;
    let url = format!(
        "http://{}:{}/internal/available",
        config.gateway.host, config.gateway.port
    );
    let client = reqwest::Client::new();
    match client
        .post(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => println!("Agent set to available."),
        Ok(r) => println!("Available failed: {}", r.status()),
        Err(_) => {
            println!("Could not reach gateway. Is it running?");
        }
    }
    Ok(())
}

fn run_pairing_list(channel: Option<&str>) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let requests = db.list_pairings(channel)?;
    if requests.is_empty() {
        println!("No pending pairing requests.");
        return Ok(());
    }
    println!(
        "{:<12} {:<20} {:<10} {:<20}",
        "Channel", "Sender ID", "Code", "Expires"
    );
    println!("{}", "─".repeat(64));
    for r in &requests {
        let expires = format_ts(r.expires_at, "%Y-%m-%d %H:%M UTC");
        println!(
            "{:<12} {:<20} {:<10} {:<20}",
            r.channel_name, r.sender_id, r.code, expires
        );
    }
    println!();
    println!("Approve with: borg pairing approve <channel> <code>");
    Ok(())
}

fn run_pairing_approve(channel: &str, code: &str) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let request = db.approve_pairing(channel, code)?;
    println!(
        "Approved {} sender {}.",
        request.channel_name, request.sender_id
    );
    Ok(())
}

fn run_pairing_revoke(channel: &str, sender_id: &str) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    if db.revoke_sender(channel, sender_id)? {
        println!("Revoked {channel} sender {sender_id}.");
    } else {
        println!("No approved sender found for {channel} with ID {sender_id}.");
    }
    Ok(())
}

fn run_pairing_approved(channel: Option<&str>) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let senders = db.list_approved_senders(channel)?;
    if senders.is_empty() {
        println!("No approved senders.");
        return Ok(());
    }
    println!(
        "{:<12} {:<20} {:<16} {:<20}",
        "Channel", "Sender ID", "Display Name", "Approved At"
    );
    println!("{}", "─".repeat(70));
    for s in &senders {
        let approved = format_ts(s.approved_at, "%Y-%m-%d %H:%M UTC");
        let name = s.display_name.as_deref().unwrap_or("—");
        println!(
            "{:<12} {:<20} {:<16} {:<20}",
            s.channel_name, s.sender_id, name, approved
        );
    }
    Ok(())
}

fn run_usage() -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let total_tokens = db.monthly_token_total()?;
    let total_cost = db.monthly_total_cost()?;
    let by_model = db.monthly_usage_by_model()?;

    println!("Token usage for current month");
    println!("────────────────────────────────────────");
    println!("Total tokens: {total_tokens}");
    if let Some(cost) = total_cost {
        println!("Estimated cost: ${cost:.4}");
    }

    if !by_model.is_empty() {
        println!();
        println!(
            "{:<40} {:>10} {:>10} {:>10} {:>10}",
            "Model", "Input", "Output", "Total", "Cost"
        );
        println!("{}", "─".repeat(84));
        for row in &by_model {
            let label = if row.model.is_empty() {
                "(unknown)".to_string()
            } else {
                row.model.clone()
            };
            let cost_str = match row.total_cost_usd {
                Some(c) => format!("${c:.4}"),
                None => "—".to_string(),
            };
            println!(
                "{:<40} {:>10} {:>10} {:>10} {:>10}",
                label, row.prompt_tokens, row.completion_tokens, row.total_tokens, cost_str
            );
        }
    }

    let config = borg_core::config::Config::load().unwrap_or_default();
    let budget_limit = config.budget.monthly_token_limit;
    if budget_limit > 0 {
        let pct = total_tokens as f64 / budget_limit as f64 * 100.0;
        println!();
        println!("Budget: {total_tokens}/{budget_limit} tokens ({pct:.1}%) used");
    }

    Ok(())
}

fn run_status() -> Result<()> {
    let now = chrono::Utc::now();
    let db = borg_core::db::Database::open()?;

    // Evolution header
    if let Ok(evo_state) = db.get_evolution_state() {
        println!("Borg Status");
        println!("\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
        println!();
        print!(
            "{}",
            borg_core::evolution::format_status_section(&evo_state)
        );
        println!();
    }

    // Vitals
    let state = db.get_vitals_state()?;
    let state = borg_core::vitals::apply_decay(&state, now);
    let mut drift = borg_core::vitals::detect_drift(&state, now);
    let since = (now - chrono::Duration::days(7)).timestamp();
    let events = db.vitals_events_since(since)?;
    if borg_core::vitals::detect_failure_drift(&events) {
        drift.push(borg_core::vitals::DriftFlag::RepeatedFailures);
    }
    println!(
        "{}",
        borg_core::vitals::format_status(&state, &events, &drift)
    );

    // Bond
    if let Ok(bond_events) = db.get_all_bond_events() {
        let bond_state = borg_core::bond::replay_events(&bond_events);
        println!("Bond");
        println!(
            "  score        {}  {} ({})",
            format_bar(bond_state.score as usize, 10),
            bond_state.score,
            bond_state.level
        );
        println!();
    }

    Ok(())
}

fn format_bar(value: usize, width: usize) -> String {
    let filled = (value * width) / 100;
    format!(
        "{}{}",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(width.saturating_sub(filled))
    )
}

fn run_status_history() -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let events = db.evolution_events_since(0)?;
    // Reverse to chronological for display
    let mut events = events;
    events.reverse();
    println!("{}", borg_core::evolution::format_history(&events));
    Ok(())
}

fn run_status_archetypes() -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let state = db.get_evolution_state()?;
    println!("{}", borg_core::evolution::format_archetype_scores(&state));
    Ok(())
}

fn run_bond_status() -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let events = db.get_all_bond_events()?;
    let state = borg_core::bond::replay_events(&events);
    let correction_rate = borg_core::bond::compute_correction_rate(&db);
    let routine_rate = borg_core::bond::compute_routine_success_rate(&db);
    let pref_count = borg_core::bond::compute_preference_learning_count(&db);
    let since = (chrono::Utc::now() - chrono::Duration::days(7)).timestamp();
    let recent = db.bond_events_since(since)?;
    println!(
        "{}",
        borg_core::bond::format_status(&state, correction_rate, routine_rate, pref_count, &recent)
    );
    Ok(())
}

fn run_bond_history(count: usize) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let events = db.bond_events_recent(count)?;
    println!("{}", borg_core::bond::format_history(&events));
    Ok(())
}

fn run_doctor() -> Result<()> {
    let config = borg_core::config::Config::load().unwrap_or_default();
    let report = borg_core::doctor::run_diagnostics(&config);
    println!("{}", report.format());
    let (_pass, _warn, fail) = report.counts();
    if fail > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn init_data_dir() -> Result<()> {
    let data_dir = borg_core::config::Config::data_dir()?;
    std::fs::create_dir_all(&data_dir)?;

    // Run interactive onboarding wizard
    match onboarding::run_onboarding()? {
        Some(result) => {
            onboarding::apply_onboarding(&result)?;

            // Auto-start daemon service after successful onboarding (non-fatal)
            if let Err(e) = service::ensure_service_running() {
                tracing::warn!("Auto-start service during onboarding: {e}");
            }

            println!();
            println!("Initialized {}", data_dir.display());

            // Auto-launch TUI for the first conversation
            let setup_path = data_dir.join("SETUP.md");
            if setup_path.exists() && std::io::stdin().is_terminal() {
                println!();
                println!("Launching your agent for the first time...");
                // The TUI will detect SETUP.md and inject it into the system prompt
                return Ok(());
            }

            println!();
            println!("You're all set! Run `borg` to start chatting.");
        }
        None => {
            // User cancelled — fall back to defaults so the directory is still usable
            init_data_dir_defaults(&data_dir)?;
            println!("Initialized {} with defaults", data_dir.display());
        }
    }

    Ok(())
}

/// Set `~/.borg/` to mode 0700 so only the owner can access it.
/// This protects IDENTITY.md, memory, config, conversation logs, and API key env files.
#[cfg(unix)]
fn harden_data_dir(data_dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if data_dir.exists() {
        let perms = std::fs::Permissions::from_mode(0o700);
        let _ = std::fs::set_permissions(data_dir, perms);
    }
}

#[cfg(not(unix))]
fn harden_data_dir(_data_dir: &std::path::Path) {
    // No-op on non-Unix platforms
}

fn run_settings_show() -> Result<()> {
    let resolver = borg_core::settings::SettingsResolver::load()?;
    let all = resolver.list_all()?;
    println!("Settings:");
    for info in &all {
        println!("  {:40} = {:20} [{}]", info.key, info.value, info.source);
    }
    Ok(())
}

fn run_settings_set(key: &str, value: &str) -> Result<()> {
    let resolver = borg_core::settings::SettingsResolver::load()?;
    let confirmation = resolver.set(key, value)?;
    println!("Updated: {confirmation}");
    Ok(())
}

fn run_settings_get(key: &str) -> Result<()> {
    let resolver = borg_core::settings::SettingsResolver::load()?;
    let (value, source) = resolver.get_with_source(key)?;
    println!("{key} = {value} [{source}]");
    Ok(())
}

fn run_settings_unset(key: &str) -> Result<()> {
    let resolver = borg_core::settings::SettingsResolver::load()?;
    resolver.unset(key)?;
    let (value, source) = resolver.get_with_source(key)?;
    println!("Unset {key} — effective value: {value} [{source}]");
    Ok(())
}

fn run_logs(count: usize, verbose: bool) -> Result<()> {
    let lines = borg_core::logging::read_history_formatted(count, verbose)?;
    if lines.is_empty() {
        println!("No conversation history.");
    } else {
        for line in &lines {
            println!("{line}");
        }
    }
    Ok(())
}

fn run_tasks_list() -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let tasks = db.list_tasks()?;

    if tasks.is_empty() {
        println!("No scheduled tasks.");
    } else {
        println!(
            "{:8}  {:20}  {:8}  {:8}  {:20}  NEXT RUN",
            "ID", "NAME", "TYPE", "STATUS", "SCHEDULE"
        );
        for task in &tasks {
            let next_run = task
                .next_run
                .map(|ts| format_ts(ts, "%Y-%m-%d %H:%M:%S"))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:8}  {:20}  {:8}  {:8}  {:20}  {}",
                short_id(&task.id),
                truncate_str(&task.name, 20),
                task.schedule_type,
                task.status,
                truncate_str(&task.schedule_expr, 20),
                next_run,
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_tasks_create(
    name: &str,
    prompt: &str,
    schedule: &str,
    schedule_type: &str,
    max_retries: Option<i32>,
    timeout_ms: Option<i64>,
    delivery_channel: Option<&str>,
    delivery_target: Option<&str>,
) -> Result<()> {
    borg_core::tasks::validate_schedule(schedule_type, schedule)?;
    let next_run = borg_core::tasks::calculate_next_run(schedule_type, schedule)?;
    let id = Uuid::new_v4().to_string();
    let tz = chrono::Local::now().offset().to_string();

    let db = borg_core::db::Database::open()?;
    db.create_task(&borg_core::db::NewTask {
        id: &id,
        name,
        prompt,
        schedule_type,
        schedule_expr: schedule,
        timezone: &tz,
        next_run,
        max_retries,
        timeout_ms,
        delivery_channel,
        delivery_target,
        allowed_tools: None, // TODO: Add --tools CLI flag
    })?;

    println!("Created task {} ({})", short_id(&id), name);
    Ok(())
}

fn run_tasks_run(id: &str) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    match db.get_task_by_id(id)? {
        Some(_task) => {
            let now = chrono::Utc::now().timestamp();
            db.update_task_next_run(id, Some(now))?;
            db.clear_task_retry(id)?;
            println!("Task {} queued for immediate execution.", short_id(id));
        }
        None => println!("Task not found: {id}"),
    }
    Ok(())
}

fn run_tasks_runs(id: &str, count: usize) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    let runs = db.task_run_history(id, count)?;
    if runs.is_empty() {
        println!("No runs recorded for task {}", short_id(id));
        return Ok(());
    }
    println!("{:<20} {:<8} {:<10} Details", "Time", "Status", "Duration");
    println!("{}", "-".repeat(70));
    for run in &runs {
        let when = format_ts(run.started_at, "%Y-%m-%d %H:%M");
        let status = borg_core::tasks::format_run_status(&run.status);
        let duration = format!("{}ms", run.duration_ms);
        let details = run
            .error
            .as_deref()
            .or(run.result.as_deref())
            .unwrap_or("")
            .chars()
            .take(40)
            .collect::<String>();
        println!("{when:<20} {status:<8} {duration:<10} {details}");
    }
    Ok(())
}

fn run_tasks_status(id: &str) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    match db.get_task_by_id(id)? {
        Some(task) => {
            println!("{}", borg_core::tasks::format_task(&task));
            println!("    Max retries: {}", task.max_retries);
            println!("    Timeout: {}ms", task.timeout_ms);
            if let Some(ref ch) = task.delivery_channel {
                println!(
                    "    Delivery: {} -> {}",
                    ch,
                    task.delivery_target.as_deref().unwrap_or("?")
                );
            }
            if task.retry_count > 0 {
                println!(
                    "    Retry state: attempt {}/{}",
                    task.retry_count, task.max_retries
                );
                if let Some(ref err) = task.last_error {
                    println!("    Last error: {}", &err[..err.len().min(100)]);
                }
                if let Some(retry_at) = task.retry_after {
                    let when = format_ts(retry_at, "%Y-%m-%d %H:%M UTC");
                    println!("    Next retry: {when}");
                }
            }
            if let Ok(Some(run)) = db.last_task_run(id) {
                let when = format_ts(run.started_at, "%Y-%m-%d %H:%M UTC");
                println!(
                    "    Last run: {} at {when} ({}ms)",
                    run.status, run.duration_ms
                );
            }
        }
        None => println!("Task not found: {id}"),
    }
    Ok(())
}

fn run_tasks_delete(id: &str) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    if db.delete_task(id)? {
        println!("Deleted task {}", short_id(id));
    } else {
        println!("Task not found: {id}");
    }
    Ok(())
}

fn run_tasks_update_status(id: &str, status: &str) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    if db.update_task_status(id, status)? {
        println!("Task {} status: {status}", short_id(id));
    } else {
        println!("Task not found: {id}");
    }
    Ok(())
}

fn truncate_str(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max - 1;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn run_uninstall() -> Result<()> {
    let data_dir = borg_core::config::Config::data_dir()?;

    eprintln!(
        "WARNING: This will permanently delete all Borg data at {}\n\
         including config, memory, tools, skills, channels, and database.\n",
        data_dir.display()
    );
    eprint!("Continue? [y/N] ");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if confirm_uninstall(input.trim()) {
        if let Err(e) = service::uninstall_service() {
            tracing::debug!("Service uninstall skipped: {e}");
        }

        delete_data_dir(&data_dir)?;

        println!("Borg data deleted. Goodbye!");
    } else {
        println!("Aborted.");
    }

    Ok(())
}

fn confirm_uninstall(input: &str) -> bool {
    input.eq_ignore_ascii_case("y")
}

fn delete_data_dir(data_dir: &std::path::Path) -> Result<()> {
    if data_dir.exists() {
        std::fs::remove_dir_all(data_dir)?;
    }
    Ok(())
}

/// Non-interactive fallback: write default config files without the wizard.
fn init_data_dir_defaults(data_dir: &std::path::Path) -> Result<()> {
    for sub in crate::onboarding::BORG_SUBDIRS {
        std::fs::create_dir_all(data_dir.join(sub))?;
    }

    let config_path = data_dir.join("config.toml");
    if !config_path.exists() {
        let config_content =
            onboarding::generate_config("anthropic/claude-sonnet-4", "openrouter", "", "", false)?;
        std::fs::write(&config_path, config_content)?;
        println!("  Created {}", config_path.display());
    }

    // Migrate SOUL.md → IDENTITY.md for existing users
    let old_identity = data_dir.join("SOUL.md");
    let new_identity = data_dir.join("IDENTITY.md");
    if old_identity.exists() && !new_identity.exists() {
        std::fs::rename(&old_identity, &new_identity)?;
    }

    let identity_path = data_dir.join("IDENTITY.md");
    if !identity_path.exists() {
        let identity = borg_core::identity::load_identity()?;
        borg_core::identity::save_identity(&identity)?;
        println!("  Created {}", identity_path.display());
    }

    let memory_path = data_dir.join("MEMORY.md");
    if !memory_path.exists() {
        std::fs::write(&memory_path, "# Memory Index\n\nNo memories yet.\n")?;
        println!("  Created {}", memory_path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn confirm_uninstall_accepts_y() {
        assert!(confirm_uninstall("y"));
    }

    #[test]
    fn confirm_uninstall_accepts_uppercase_y() {
        assert!(confirm_uninstall("Y"));
    }

    #[test]
    fn confirm_uninstall_rejects_empty() {
        assert!(!confirm_uninstall(""));
    }

    #[test]
    fn confirm_uninstall_rejects_no() {
        assert!(!confirm_uninstall("n"));
        assert!(!confirm_uninstall("N"));
    }

    #[test]
    fn confirm_uninstall_rejects_yes_spelled_out() {
        assert!(!confirm_uninstall("yes"));
    }

    #[test]
    fn confirm_uninstall_rejects_arbitrary_input() {
        assert!(!confirm_uninstall("maybe"));
        assert!(!confirm_uninstall("yy"));
    }

    #[test]
    fn delete_data_dir_removes_directory() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("borg_test");
        fs::create_dir_all(dir.join("memory")).unwrap();
        fs::write(dir.join("config.toml"), "test").unwrap();

        assert!(dir.exists());
        delete_data_dir(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn delete_data_dir_noop_when_missing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("nonexistent");
        assert!(!dir.exists());
        delete_data_dir(&dir).unwrap();
    }

    // -- Clap parsing tests --

    #[test]
    fn test_parse_settings_show() {
        let cli = Cli::try_parse_from(["borg", "settings"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Settings { action: None })
        ));
    }

    #[test]
    fn test_parse_settings_set() {
        let cli = Cli::try_parse_from(["borg", "settings", "set", "temperature", "0.5"]).unwrap();
        match cli.command {
            Some(Commands::Settings {
                action: Some(SettingsAction::Set { key, value }),
            }) => {
                assert_eq!(key, "temperature");
                assert_eq!(value, "0.5");
            }
            _ => panic!("Expected Settings Set"),
        }
    }

    #[test]
    fn test_parse_settings_get() {
        let cli = Cli::try_parse_from(["borg", "settings", "get", "temperature"]).unwrap();
        match cli.command {
            Some(Commands::Settings {
                action: Some(SettingsAction::Get { key }),
            }) => {
                assert_eq!(key, "temperature");
            }
            _ => panic!("Expected Settings Get"),
        }
    }

    #[test]
    fn test_parse_settings_unset() {
        let cli = Cli::try_parse_from(["borg", "settings", "unset", "temperature"]).unwrap();
        match cli.command {
            Some(Commands::Settings {
                action: Some(SettingsAction::Unset { key }),
            }) => {
                assert_eq!(key, "temperature");
            }
            _ => panic!("Expected Settings Unset"),
        }
    }

    #[test]
    fn test_parse_logs_default() {
        let cli = Cli::try_parse_from(["borg", "logs"]).unwrap();
        match cli.command {
            Some(Commands::Logs { count, verbose }) => {
                assert_eq!(count, 50);
                assert!(!verbose);
            }
            _ => panic!("Expected Logs"),
        }
    }

    #[test]
    fn test_parse_logs_custom_count() {
        let cli = Cli::try_parse_from(["borg", "logs", "--count", "10"]).unwrap();
        match cli.command {
            Some(Commands::Logs { count, .. }) => assert_eq!(count, 10),
            _ => panic!("Expected Logs"),
        }
    }

    #[test]
    fn test_parse_logs_verbose() {
        let cli = Cli::try_parse_from(["borg", "logs", "--verbose"]).unwrap();
        match cli.command {
            Some(Commands::Logs { count, verbose }) => {
                assert_eq!(count, 50);
                assert!(verbose);
            }
            _ => panic!("Expected Logs"),
        }
    }

    #[test]
    fn test_parse_logs_verbose_short() {
        let cli = Cli::try_parse_from(["borg", "logs", "-v"]).unwrap();
        match cli.command {
            Some(Commands::Logs { verbose, .. }) => assert!(verbose),
            _ => panic!("Expected Logs"),
        }
    }

    #[test]
    fn test_parse_logs_verbose_with_count() {
        let cli = Cli::try_parse_from(["borg", "logs", "-v", "-c", "20"]).unwrap();
        match cli.command {
            Some(Commands::Logs { count, verbose }) => {
                assert!(verbose);
                assert_eq!(count, 20);
            }
            _ => panic!("Expected Logs"),
        }
    }

    #[test]
    fn test_parse_tasks_list() {
        let cli = Cli::try_parse_from(["borg", "tasks"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Tasks { action: None })
        ));

        let cli = Cli::try_parse_from(["borg", "tasks", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Tasks {
                action: Some(TasksAction::List)
            })
        ));
    }

    #[test]
    fn test_parse_tasks_create() {
        let cli = Cli::try_parse_from([
            "borg",
            "tasks",
            "create",
            "--name",
            "test",
            "--prompt",
            "hello",
            "--schedule",
            "*/5 * * * *",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Tasks {
                action:
                    Some(TasksAction::Create {
                        name,
                        prompt,
                        schedule,
                        r#type,
                        ..
                    }),
            }) => {
                assert_eq!(name, "test");
                assert_eq!(prompt, "hello");
                assert_eq!(schedule, "*/5 * * * *");
                assert_eq!(r#type, "cron");
            }
            _ => panic!("Expected Tasks Create"),
        }
    }

    #[test]
    fn test_parse_tasks_create_short_flags() {
        let cli = Cli::try_parse_from([
            "borg",
            "tasks",
            "create",
            "-n",
            "test",
            "-p",
            "hello",
            "-s",
            "*/5 * * * *",
            "-t",
            "interval",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Tasks {
                action:
                    Some(TasksAction::Create {
                        name,
                        prompt,
                        schedule,
                        r#type,
                        ..
                    }),
            }) => {
                assert_eq!(name, "test");
                assert_eq!(prompt, "hello");
                assert_eq!(schedule, "*/5 * * * *");
                assert_eq!(r#type, "interval");
            }
            _ => panic!("Expected Tasks Create"),
        }
    }

    #[test]
    fn test_parse_ask_short_flags() {
        let cli = Cli::try_parse_from(["borg", "ask", "-y", "-j", "hello"]).unwrap();
        match cli.command {
            Some(Commands::Ask {
                message,
                yes,
                json,
                mode,
            }) => {
                assert_eq!(message, "hello");
                assert!(yes);
                assert!(json);
                assert!(mode.is_none());
            }
            _ => panic!("Expected Ask"),
        }
    }

    #[test]
    fn test_parse_tasks_delete() {
        let cli = Cli::try_parse_from(["borg", "tasks", "delete", "abc123"]).unwrap();
        match cli.command {
            Some(Commands::Tasks {
                action: Some(TasksAction::Delete { id }),
            }) => assert_eq!(id, "abc123"),
            _ => panic!("Expected Tasks Delete"),
        }
    }

    #[test]
    fn test_parse_tasks_pause_resume() {
        let cli = Cli::try_parse_from(["borg", "tasks", "pause", "abc123"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Tasks {
                action: Some(TasksAction::Pause { .. })
            })
        ));

        let cli = Cli::try_parse_from(["borg", "tasks", "resume", "abc123"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Tasks {
                action: Some(TasksAction::Resume { .. })
            })
        ));
    }

    // -- Settings tests --

    #[test]
    fn test_settings_show_with_default_config() {
        let config = borg_core::config::Config::default();
        let output = config.display_settings();
        assert!(!output.is_empty());
        assert!(output.contains("temperature"));
    }

    #[test]
    fn test_settings_apply_valid_key() {
        let mut config = borg_core::config::Config::default();
        let result = config.apply_setting("temperature", "0.5");
        assert!(result.is_ok());
        assert!((config.llm.temperature - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_settings_apply_invalid_key() {
        let mut config = borg_core::config::Config::default();
        let result = config.apply_setting("nonexistent", "x");
        assert!(result.is_err());
    }

    #[test]
    fn test_settings_apply_invalid_value() {
        let mut config = borg_core::config::Config::default();
        let result = config.apply_setting("temperature", "not_a_number");
        assert!(result.is_err());
    }

    // -- Tasks tests (using temp DB) --

    #[test]
    fn test_tasks_validate_schedule_cron() {
        assert!(borg_core::tasks::validate_schedule("cron", "0 */5 * * * *").is_ok());
    }

    #[test]
    fn test_tasks_validate_schedule_interval() {
        assert!(borg_core::tasks::validate_schedule("interval", "30m").is_ok());
    }

    #[test]
    fn test_tasks_validate_schedule_invalid() {
        assert!(borg_core::tasks::validate_schedule("cron", "not valid").is_err());
    }

    // -- Helper tests --

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let result = truncate_str("hello world", 5);
        assert_eq!(result, "hell…");
    }
}
