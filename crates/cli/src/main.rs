use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

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
    /// Initialize ~/.tamagotchi directory with default config
    Init,
    /// Send a single message and exit
    Ask {
        /// The message to send
        message: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
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
    std::fs::create_dir_all(data_dir.join("memory"))?;
    std::fs::create_dir_all(data_dir.join("tools"))?;
    std::fs::create_dir_all(data_dir.join("skills"))?;
    std::fs::create_dir_all(data_dir.join("logs"))?;
    std::fs::create_dir_all(data_dir.join("cache"))?;

    let config_path = data_dir.join("config.toml");
    if !config_path.exists() {
        std::fs::write(
            &config_path,
            r#"[llm]
api_key_env = "OPENROUTER_API_KEY"
model = "anthropic/claude-sonnet-4"
temperature = 0.7
max_tokens = 4096

[heartbeat]
enabled = false
interval = "30m"
quiet_hours_start = "23:00"
quiet_hours_end = "07:00"

[tools]
default_timeout_ms = 30000

[sandbox]
enabled = true
mode = "strict"

[memory]
max_context_tokens = 8000

[skills]
enabled = true
max_context_tokens = 4000
"#,
        )?;
        println!("Created {}", config_path.display());
    }

    let soul_path = data_dir.join("SOUL.md");
    if !soul_path.exists() {
        // load_soul() returns the default; save it
        let soul = tamagotchi_core::soul::load_soul()?;
        tamagotchi_core::soul::save_soul(&soul)?;
        println!("Created {}", soul_path.display());
    }

    let memory_path = data_dir.join("MEMORY.md");
    if !memory_path.exists() {
        std::fs::write(&memory_path, "# Memory Index\n\nNo memories yet.\n")?;
        println!("Created {}", memory_path.display());
    }

    println!("Initialized {}", data_dir.display());
    Ok(())
}
