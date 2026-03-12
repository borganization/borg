use anyhow::Result;
use crossterm::{
    style::{Color, Print, ResetColor, SetForegroundColor},
    ExecutableCommand,
};
use std::io::{self, Write};
use tokio::sync::mpsc;

use tamagotchi_core::agent::{Agent, AgentEvent};
use tamagotchi_core::config::Config;
use tamagotchi_heartbeat::scheduler::{HeartbeatEvent, HeartbeatScheduler};

pub async fn run() -> Result<()> {
    let config = Config::load()?;
    let mut agent = Agent::new(config.clone())?;

    // Start heartbeat if enabled
    if config.heartbeat.enabled {
        let (hb_tx, mut hb_rx) = mpsc::channel::<HeartbeatEvent>(32);
        let llm = tamagotchi_core::llm::LlmClient::new(config.clone())?;
        let scheduler = HeartbeatScheduler::new(config.heartbeat.clone(), llm);
        tokio::spawn(async move {
            scheduler.run(hb_tx).await;
        });
        tokio::spawn(async move {
            while let Some(event) = hb_rx.recv().await {
                match event {
                    HeartbeatEvent::Message(msg) => {
                        let mut stdout = io::stdout();
                        let _ = stdout.execute(SetForegroundColor(Color::Cyan));
                        let _ = stdout.execute(Print(format!("\n[heartbeat] {msg}\n")));
                        let _ = stdout.execute(ResetColor);
                        let _ = stdout.execute(Print("> "));
                        let _ = stdout.flush();
                    }
                }
            }
        });
    }

    println!("Tamagotchi AI Assistant");
    println!("Type 'quit' or 'exit' to leave. Type 'help' for commands.\n");

    loop {
        print!("> ");
        io::stdout().flush()?;

        // Read input in a blocking thread so we don't block the tokio runtime
        let input = tokio::task::spawn_blocking(|| {
            let mut input = String::new();
            let bytes = io::stdin().read_line(&mut input)?;
            Ok::<(String, usize), io::Error>((input, bytes))
        })
        .await??;

        if input.1 == 0 {
            break; // EOF
        }

        let input = input.0.trim().to_string();
        if input.is_empty() {
            continue;
        }

        match input.as_str() {
            "quit" | "exit" => break,
            "help" => {
                println!("Commands:");
                println!("  quit/exit  - Exit the assistant");
                println!("  help       - Show this help");
                println!("  /tools     - List available tools");
                println!("  /memory    - Show loaded memory");
                println!();
                continue;
            }
            "/tools" => {
                let registry = tamagotchi_tools::registry::ToolRegistry::new()?;
                let tools = registry.list_tools();
                if tools.is_empty() {
                    println!("No user tools installed.");
                } else {
                    for tool in tools {
                        println!("  {tool}");
                    }
                }
                println!();
                continue;
            }
            "/memory" => {
                let memory =
                    tamagotchi_core::memory::load_memory_context(config.memory.max_context_tokens)?;
                if memory.is_empty() {
                    println!("No memories loaded.");
                } else {
                    println!("{memory}");
                }
                continue;
            }
            _ => {}
        }

        let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(256);

        // Spawn the agent work — agent is !Send because of the way we set it up,
        // so we run it in the current task and collect events
        let send_future = agent.send_message(&input, event_tx);

        // We need to drive both the send_future and the event rendering concurrently
        let render_handle = tokio::spawn(async move {
            let mut stdout = io::stdout();
            while let Some(event) = event_rx.recv().await {
                match event {
                    AgentEvent::TextDelta(delta) => {
                        print!("{delta}");
                        let _ = stdout.flush();
                    }
                    AgentEvent::ToolExecuting { name, .. } => {
                        let _ = stdout.execute(SetForegroundColor(Color::Yellow));
                        print!("\n[tool: {name}]");
                        let _ = stdout.execute(ResetColor);
                        let _ = stdout.flush();
                    }
                    AgentEvent::ToolResult { result, .. } => {
                        let _ = stdout.execute(SetForegroundColor(Color::Green));
                        let preview = if result.len() > 200 {
                            format!("{}...", &result[..200])
                        } else {
                            result
                        };
                        print!(" -> {preview}");
                        let _ = stdout.execute(ResetColor);
                        println!();
                        let _ = stdout.flush();
                    }
                    AgentEvent::TurnComplete => {
                        println!();
                    }
                    AgentEvent::Error(e) => {
                        let _ = stdout.execute(SetForegroundColor(Color::Red));
                        println!("\nError: {e}");
                        let _ = stdout.execute(ResetColor);
                    }
                }
            }
        });

        if let Err(e) = send_future.await {
            eprintln!("Error: {e}");
        }

        // Wait for rendering to finish
        let _ = render_handle.await;
    }

    println!("Goodbye!");
    Ok(())
}

pub async fn one_shot(message: &str) -> Result<()> {
    let config = Config::load()?;
    let mut agent = Agent::new(config)?;

    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(256);

    let send_future = agent.send_message(message, event_tx);

    let render_handle = tokio::spawn(async move {
        let mut stdout = io::stdout();
        while let Some(event) = event_rx.recv().await {
            match event {
                AgentEvent::TextDelta(delta) => {
                    print!("{delta}");
                    let _ = stdout.flush();
                }
                AgentEvent::TurnComplete => {
                    println!();
                }
                AgentEvent::Error(e) => {
                    eprintln!("Error: {e}");
                }
                _ => {}
            }
        }
    });

    if let Err(e) = send_future.await {
        eprintln!("Error: {e}");
    }

    let _ = render_handle.await;
    Ok(())
}
