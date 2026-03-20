use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

mod credentials;
mod logo;
mod onboarding;
mod onboarding_tui;
mod plugins;
mod repl;
mod service;
mod tui;

#[derive(Parser)]
#[command(name = "borg", about = "AI Personal Assistant Agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start Borg — interactive TUI with auto-gateway (default)
    Start,
    /// Start interactive chat (alias for start)
    Chat,
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
    },
    /// Run diagnostics to check configuration, connectivity, and dependencies
    Doctor,
    /// Start the webhook gateway server for messaging channels
    Gateway,
    /// Run as a background daemon (executes scheduled tasks and heartbeat)
    Daemon,
    /// Manage the daemon as a system service
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// List built-in and user-created tools
    Tools,
    /// List skills with availability status
    Skills,
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
    /// List agent roles or show role details
    Agents {
        #[command(subcommand)]
        action: Option<AgentsAction>,
    },
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
    /// Permanently delete all Borg data and uninstall the service
    Uninstall,
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
}

#[derive(Subcommand)]
enum AgentsAction {
    /// Show details of a specific role
    Show {
        /// Role name
        name: String,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Uninstall the daemon service
    Uninstall,
    /// Show the daemon service status
    Status,
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

    let tui_mode = matches!(
        cli.command,
        Some(Commands::Start) | Some(Commands::Chat) | None
    );

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

    match cli.command {
        Some(Commands::Start) | Some(Commands::Chat) | None => {
            ensure_onboarded()?;
            repl::run().await?;
        }
        Some(Commands::Stop) => service::stop_service()?,
        Some(Commands::Restart) => service::restart_service()?,
        Some(Commands::Init) => init_data_dir()?,
        Some(Commands::Ask { message, yes, json }) => repl::one_shot(&message, yes, json).await?,
        Some(Commands::Doctor) => run_doctor()?,
        Some(Commands::Gateway) => run_gateway(shutdown).await?,
        Some(Commands::Daemon) => service::run_daemon(shutdown).await?,
        Some(Commands::Service { action }) => match action {
            ServiceAction::Uninstall => service::uninstall_service()?,
            ServiceAction::Status => service::service_status()?,
        },
        Some(Commands::Tools) => run_tools()?,
        Some(Commands::Skills) => run_skills()?,
        Some(Commands::Add { name }) => plugins::add_plugin(&name)?,
        Some(Commands::Remove { name }) => plugins::remove_plugin(&name)?,
        Some(Commands::Plugins) => plugins::list_plugins()?,
        Some(Commands::Agents { action }) => match action {
            Some(AgentsAction::Show { name }) => run_agents_show(&name)?,
            None => run_agents_list()?,
        },
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
            }) => run_tasks_create(&name, &prompt, &schedule, &r#type)?,
            Some(TasksAction::Delete { id }) => run_tasks_delete(&id)?,
            Some(TasksAction::Pause { id }) => run_tasks_update_status(&id, "paused")?,
            Some(TasksAction::Resume { id }) => run_tasks_update_status(&id, "active")?,
        },
        Some(Commands::Usage) => run_usage()?,
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
    // Auto-start daemon service (non-fatal)
    if let Err(e) = service::ensure_service_running() {
        tracing::warn!("Auto-start service: {e}");
    }
    Ok(())
}

async fn run_gateway(shutdown: CancellationToken) -> Result<()> {
    let config = borg_core::config::Config::load()?;
    let metrics = borg_core::telemetry::BorgMetrics::from_config(&config);
    let gateway = borg_gateway::GatewayServer::new(config, shutdown, metrics)?;
    gateway.run().await
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

fn run_tools() -> Result<()> {
    let config = borg_core::config::Config::load().unwrap_or_default();

    println!("Built-in tools:");
    let builtins = [
        ("write_memory", "Write/append to memory files"),
        ("read_memory", "Read a memory file"),
        ("list_tools", "List user-created tools"),
        ("apply_patch", "Create/update/delete files via patch DSL"),
        ("create_tool", "Create/modify user tools via patch DSL"),
        ("run_shell", "Execute a shell command"),
        ("list_skills", "List skills with status"),
        (
            "apply_skill_patch",
            "Create/modify skill files via patch DSL",
        ),
        ("read_pdf", "Extract text from a PDF file"),
        ("create_channel", "Create/modify channel integrations"),
        ("list_channels", "List messaging channels"),
        ("manage_tasks", "Manage scheduled tasks"),
    ];
    for (name, desc) in &builtins {
        println!("  {name:20} {desc}");
    }
    if config.web.enabled {
        println!("  {:20} Fetch a URL", "web_fetch");
        println!("  {:20} Search the web", "web_search");
    }
    if config.security.host_audit {
        println!("  {:20} Run host security audit", "security_audit");
    }

    if let Ok(registry) = borg_tools::registry::ToolRegistry::new() {
        let user_tools = registry.list_tools();
        if !user_tools.is_empty() {
            println!();
            println!("User tools:");
            for tool in &user_tools {
                println!("  {tool}");
            }
        }
    }
    Ok(())
}

fn run_skills() -> Result<()> {
    let config = borg_core::config::Config::load().unwrap_or_default();
    let creds = config.resolve_credentials();
    let skills = borg_core::skills::load_all_skills(&creds)?;

    if skills.is_empty() {
        println!("No skills found.");
    } else {
        println!("Skills:");
        for skill in &skills {
            println!("  {}", skill.summary_line());
        }
    }
    Ok(())
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
                .map(|ts| {
                    chrono::DateTime::from_timestamp(ts, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "?".to_string())
                })
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:8}  {:20}  {:8}  {:8}  {:20}  {}",
                &task.id[..8.min(task.id.len())],
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

fn run_tasks_create(name: &str, prompt: &str, schedule: &str, schedule_type: &str) -> Result<()> {
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
    })?;

    println!("Created task {} ({})", &id[..8.min(id.len())], name);
    Ok(())
}

fn run_tasks_delete(id: &str) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    if db.delete_task(id)? {
        println!("Deleted task {}", &id[..8.min(id.len())]);
    } else {
        println!("Task not found: {id}");
    }
    Ok(())
}

fn run_tasks_update_status(id: &str, status: &str) -> Result<()> {
    let db = borg_core::db::Database::open()?;
    if db.update_task_status(id, status)? {
        println!("Task {} status: {status}", &id[..8.min(id.len())]);
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

fn run_agents_list() -> Result<()> {
    let roles = borg_core::multi_agent::roles::list_all_roles();
    if roles.is_empty() {
        println!("No agent roles configured.");
    } else {
        println!("{:15} {:50} {:6} TOOLS", "NAME", "DESCRIPTION", "TEMP");
        for role in &roles {
            let temp = role
                .temperature
                .map(|t| format!("{t:.1}"))
                .unwrap_or_else(|| "-".to_string());
            let tools = role
                .tools_allowed
                .as_ref()
                .map(|t| t.join(", "))
                .unwrap_or_else(|| "all".to_string());
            let desc = truncate_str(&role.description, 50);
            println!("{:15} {:50} {:6} {}", role.name, desc, temp, tools);
        }
    }
    Ok(())
}

fn run_agents_show(name: &str) -> Result<()> {
    match borg_core::multi_agent::roles::load_role(name) {
        Some(role) => {
            println!("Role: {}", role.name);
            println!("Description: {}", role.description);
            if let Some(model) = &role.model {
                println!("Model: {model}");
            }
            if let Some(temp) = role.temperature {
                println!("Temperature: {temp}");
            }
            if let Some(tools) = &role.tools_allowed {
                println!("Tools: {}", tools.join(", "));
            } else {
                println!("Tools: all");
            }
            if let Some(instructions) = &role.system_instructions {
                println!("Instructions: {instructions}");
            }
        }
        None => println!("Role '{name}' not found."),
    }
    Ok(())
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
    for sub in &["memory", "tools", "skills", "logs", "cache"] {
        std::fs::create_dir_all(data_dir.join(sub))?;
    }

    let config_path = data_dir.join("config.toml");
    if !config_path.exists() {
        let config_content = onboarding::generate_config(
            "anthropic/claude-sonnet-4",
            "openrouter",
            "",
            "",
            0,
            &onboarding::KeyStorage::EnvFile,
        )?;
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
    fn test_parse_tools_command() {
        let cli = Cli::try_parse_from(["borg", "tools"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Tools)));
    }

    #[test]
    fn test_parse_skills_command() {
        let cli = Cli::try_parse_from(["borg", "skills"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Skills)));
    }

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
            Some(Commands::Ask { message, yes, json }) => {
                assert_eq!(message, "hello");
                assert!(yes);
                assert!(json);
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
