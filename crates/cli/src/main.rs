use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

mod logo;
mod onboarding;
mod repl;
mod service;
mod tui;

#[derive(Parser)]
#[command(name = "tamagotchi", about = "AI Personal Assistant Agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start Tamagotchi — interactive TUI with auto-gateway (default)
    Start,
    /// Start interactive chat (alias for start)
    Chat,
    /// Stop the background daemon service
    Stop,
    /// Restart the background daemon service
    Restart,
    /// Run interactive setup wizard for ~/.tamagotchi
    Init,
    /// Send a single message and exit
    Ask {
        /// The message to send
        message: String,
        /// Auto-approve all tool/shell calls (no user prompts)
        #[arg(long)]
        yes: bool,
        /// Output raw JSON instead of streaming text
        #[arg(long)]
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
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Install the daemon as a system service (launchd on macOS, systemd on Linux)
    Install,
    /// Uninstall the daemon service
    Uninstall,
    /// Show the daemon service status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    // Also load .env from the data directory (~/.tamagotchi/.env)
    if let Ok(data_dir) = tamagotchi_core::config::Config::data_dir() {
        let _ = dotenvy::from_path(data_dir.join(".env"));
        // Harden data directory permissions (owner-only access)
        harden_data_dir(&data_dir);
    }

    let cli = Cli::parse();

    let tui_mode = matches!(
        cli.command,
        Some(Commands::Start) | Some(Commands::Chat) | None
    );

    // _guard must live for the program's duration to flush logs
    let _guard;

    if tui_mode {
        let (non_blocking, guard) = match tamagotchi_core::config::Config::logs_dir() {
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
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
            )
            .with_target(false)
            .with_ansi(false)
            .with_writer(non_blocking)
            .init();
    } else {
        _guard = None;
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
            )
            .with_target(false)
            .init();
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
            ServiceAction::Install => service::install_service()?,
            ServiceAction::Uninstall => service::uninstall_service()?,
            ServiceAction::Status => service::service_status()?,
        },
    }

    Ok(())
}

fn ensure_onboarded() -> Result<()> {
    let data_dir = tamagotchi_core::config::Config::data_dir()?;
    let config_path = data_dir.join("config.toml");
    if !config_path.exists() {
        init_data_dir()?;
    }
    Ok(())
}

async fn run_gateway(shutdown: CancellationToken) -> Result<()> {
    let config = tamagotchi_core::config::Config::load()?;
    let gateway = tamagotchi_gateway::GatewayServer::new(config, shutdown)?;
    gateway.run().await
}

fn run_doctor() -> Result<()> {
    let config = tamagotchi_core::config::Config::load().unwrap_or_default();
    let report = tamagotchi_core::doctor::run_diagnostics(&config);
    println!("{}", report.format());
    let (_pass, _warn, fail) = report.counts();
    if fail > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn init_data_dir() -> Result<()> {
    let data_dir = tamagotchi_core::config::Config::data_dir()?;
    std::fs::create_dir_all(&data_dir)?;

    // Run interactive onboarding wizard
    match onboarding::run_onboarding()? {
        Some(result) => {
            let has_api_key = result.api_key.is_some();
            let provider: tamagotchi_core::provider::Provider = result.provider.parse()?;
            let env_var = provider.default_env_var();
            onboarding::apply_onboarding(&result)?;
            println!();
            println!("Initialized {}", data_dir.display());
            println!();
            if has_api_key {
                println!("You're all set! Run `tamagotchi` to start chatting.");
            } else {
                println!("You're all set! Run `tamagotchi` to start chatting.");
                println!();
                println!("Note: You'll need to set {env_var} before chatting.");
                println!("  Add it to {}", data_dir.join(".env").display());
            }
        }
        None => {
            // User cancelled — fall back to defaults so the directory is still usable
            init_data_dir_defaults(&data_dir)?;
            println!("Initialized {} with defaults", data_dir.display());
        }
    }

    Ok(())
}

/// Set `~/.tamagotchi/` to mode 0700 so only the owner can access it.
/// This protects SOUL.md, memory, config, conversation logs, and API key env files.
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

    let soul_path = data_dir.join("SOUL.md");
    if !soul_path.exists() {
        let soul = tamagotchi_core::soul::load_soul()?;
        tamagotchi_core::soul::save_soul(&soul)?;
        println!("  Created {}", soul_path.display());
    }

    let memory_path = data_dir.join("MEMORY.md");
    if !memory_path.exists() {
        std::fs::write(&memory_path, "# Memory Index\n\nNo memories yet.\n")?;
        println!("  Created {}", memory_path.display());
    }

    Ok(())
}
