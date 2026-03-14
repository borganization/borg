use anyhow::Result;
use std::io::{self, Write};
use tokio::sync::mpsc;

use tamagotchi_core::agent::{Agent, AgentEvent};
use tamagotchi_core::config::Config;

pub async fn run() -> Result<()> {
    crate::tui::run().await
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
                AgentEvent::ShellConfirmation { respond, .. } => {
                    // In one-shot mode, deny shell commands by default
                    let _ = respond.send(false);
                    eprintln!("Shell command denied in one-shot mode. Use --yes to allow.");
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
