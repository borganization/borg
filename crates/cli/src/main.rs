use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod logo;
mod onboarding;
mod repl;

#[derive(Parser)]
#[command(name = "tamagotchi", about = "AI Personal Assistant Agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start interactive REPL (default)
    Chat,
    /// Run interactive setup wizard for ~/.tamagotchi
    Init,
    /// Send a single message and exit
    Ask {
        /// The message to send
        message: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    // Also load .env from the data directory (~/.tamagotchi/.env)
    if let Ok(data_dir) = tamagotchi_core::config::Config::data_dir() {
        let _ = dotenvy::from_path(data_dir.join(".env"));
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init) => init_data_dir()?,
        Some(Commands::Ask { message }) => repl::one_shot(&message).await?,
        Some(Commands::Chat) | None => repl::run().await?,
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
            onboarding::apply_onboarding(&result)?;
            println!();
            println!("Initialized {}", data_dir.display());
            println!();
            if has_api_key {
                println!("You're all set! Run `tamagotchi` to start chatting.");
            } else {
                println!("You're all set! Run `tamagotchi` to start chatting.");
                println!();
                println!("Note: You'll need to set OPENROUTER_API_KEY before chatting.");
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

/// Non-interactive fallback: write default config files without the wizard.
fn init_data_dir_defaults(data_dir: &std::path::Path) -> Result<()> {
    for sub in &["memory", "tools", "skills", "logs", "cache"] {
        std::fs::create_dir_all(data_dir.join(sub))?;
    }

    let config_path = data_dir.join("config.toml");
    if !config_path.exists() {
        let config_content = onboarding::generate_config("anthropic/claude-sonnet-4")?;
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
