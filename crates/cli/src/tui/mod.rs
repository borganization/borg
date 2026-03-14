mod app;
mod composer;
mod history;
mod layout;
mod markdown;
mod theme;

use std::io::stdout;
use std::panic;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::{mpsc, Mutex};

use tamagotchi_core::agent::{Agent, AgentEvent};
use tamagotchi_core::config::Config;
use tamagotchi_heartbeat::scheduler::{HeartbeatEvent, HeartbeatScheduler};

use app::{App, AppAction};

/// Guard that restores terminal state on drop (both normal exit and early error return).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
    }
}

pub async fn run() -> Result<()> {
    let config = Config::load()?;
    let agent = Agent::new(config.clone())?;
    let agent = Arc::new(Mutex::new(agent));

    // Start heartbeat if enabled
    let heartbeat_rx = if config.heartbeat.enabled {
        let (hb_tx, hb_rx) = mpsc::channel::<HeartbeatEvent>(32);
        let llm = tamagotchi_core::llm::LlmClient::new(config.clone())?;
        let scheduler = HeartbeatScheduler::new(config.heartbeat.clone(), llm);
        tokio::spawn(async move {
            scheduler.run(hb_tx).await;
        });
        Some(hb_rx)
    } else {
        None
    };

    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    // Guard ensures terminal is restored on any exit path (error or normal)
    let _guard = TerminalGuard;

    // Install panic hook that restores terminal before printing panic
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config, heartbeat_rx);
    let mut event_stream = EventStream::new();
    let tick_rate = Duration::from_millis(100);

    run_event_loop(
        &mut terminal,
        &mut app,
        &agent,
        &mut event_stream,
        tick_rate,
    )
    .await
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App<'_>,
    agent: &Arc<Mutex<Agent>>,
    event_stream: &mut EventStream,
    tick_rate: Duration,
) -> Result<()> {
    let mut tick_interval = tokio::time::interval(tick_rate);

    loop {
        terminal.draw(|frame| app.render(frame))?;

        let action = tokio::select! {
            biased;

            // Agent events (check first for responsiveness)
            event = async {
                if let Some(rx) = &mut app.event_rx {
                    rx.recv().await
                } else {
                    std::future::pending().await
                }
            } => {
                match event {
                    Some(ev) => {
                        app.process_agent_event(ev);
                        AppAction::Continue
                    }
                    None => {
                        // Channel closed (agent task finished or panicked)
                        app.handle_agent_channel_closed();
                        AppAction::Continue
                    }
                }
            }

            // Heartbeat events
            Some(event) = async {
                if let Some(rx) = &mut app.heartbeat_rx {
                    rx.recv().await
                } else {
                    std::future::pending().await
                }
            } => {
                app.process_heartbeat(event);
                AppAction::Continue
            }

            // Terminal events
            maybe_event = event_stream.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) => app.handle_key(key)?,
                    Some(Ok(Event::Resize(_, _))) => AppAction::Continue,
                    Some(Err(_)) | None => AppAction::Quit,
                    _ => AppAction::Continue,
                }
            }

            // Tick for status bar elapsed time
            _ = tick_interval.tick() => {
                AppAction::Continue
            }
        };

        match action {
            AppAction::Quit => return Ok(()),
            AppAction::SendMessage { input, event_tx } => {
                let agent_clone = Arc::clone(agent);
                tokio::spawn(async move {
                    let mut agent = agent_clone.lock().await;
                    if let Err(e) = agent.send_message(&input, event_tx.clone()).await {
                        let _ = event_tx.send(AgentEvent::Error(e.to_string())).await;
                    }
                });
            }
            AppAction::Continue => {}
        }
    }
}
