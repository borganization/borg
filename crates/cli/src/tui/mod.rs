mod app;
mod command_popup;
mod composer;
mod customize_popup;
mod external_editor;
mod history;
mod layout;
mod markdown;
mod settings_popup;
mod spinner;
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
    let mut agent = Agent::new(config.clone())?;

    // Try to resume the last session
    let mut resumed_info: Option<(String, usize)> = None;
    if let Ok(Some(session)) = tamagotchi_core::session::load_last_session() {
        if !session.messages.is_empty() {
            let title = session.meta.title.clone();
            let count = session.meta.message_count;
            if agent.load_session(&session.meta.id).is_ok() {
                resumed_info = Some((title, count));
            }
        }
    }

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
    if let Some((title, count)) = resumed_info {
        app.push_system_message(format!("Resumed session: {title} ({count} messages)"));
    }
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

/// If the app just became idle and has a queued message, auto-submit it.
fn drain_queued_if_idle(app: &mut App<'_>) -> Result<AppAction> {
    if matches!(app.state, app::AppState::Idle) {
        if let Some(queued) = app.take_queued_message() {
            return app.handle_queued_submit(&queued);
        }
    }
    Ok(AppAction::Continue)
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
                        drain_queued_if_idle(app)?
                    }
                    None => {
                        // Channel closed (agent task finished or panicked)
                        app.handle_agent_channel_closed();
                        drain_queued_if_idle(app)?
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
            AppAction::SendMessage {
                input,
                event_tx,
                cancel,
            } => {
                let agent_clone = Arc::clone(agent);
                tokio::spawn(async move {
                    let mut agent = agent_clone.lock().await;
                    if let Err(e) = agent
                        .send_message_with_cancel(&input, event_tx.clone(), cancel)
                        .await
                    {
                        let _ = event_tx.send(AgentEvent::Error(e.to_string())).await;
                    }
                });
            }
            AppAction::CompactHistory => {
                let mut agent = agent.lock().await;
                let (before, after) = agent.compact().await;
                let freed = before.saturating_sub(after);
                app.push_system_message(format!(
                    "Compacted: {before} → {after} tokens ({freed} freed)"
                ));
            }
            AppAction::ClearHistory => {
                let mut agent = agent.lock().await;
                agent.clear_history();
                app.push_system_message("Conversation cleared.".to_string());
            }
            AppAction::ShowUsage => {
                let agent = agent.lock().await;
                let (msg_count, token_count) = agent.conversation_stats();
                drop(agent);

                let prompt_tok = app.session_prompt_tokens;
                let completion_tok = app.session_completion_tokens;
                let total_tok = prompt_tok + completion_tok;

                let mut text = format!(
                    "Session: {msg_count} messages, ~{token_count} estimated tokens\n\
                     LLM usage: {prompt_tok} prompt + {completion_tok} completion = {total_tok} total tokens\n"
                );

                for (label, days) in [("24h", 1), ("7d", 7), ("30d", 30)] {
                    match tamagotchi_core::logging::count_messages_for_period(days) {
                        Ok(stats) => {
                            text.push_str(&format!(
                                "{label}: {} user, {} assistant, {} tool calls\n",
                                stats.user_messages, stats.assistant_messages, stats.tool_calls
                            ));
                        }
                        Err(e) => {
                            text.push_str(&format!("{label}: error reading logs: {e}\n"));
                        }
                    }
                }

                // Show monthly budget info
                let budget_limit = app.config.budget.monthly_token_limit;
                if budget_limit > 0 {
                    if let Ok(db) = tamagotchi_core::db::Database::open() {
                        if let Ok(used) = db.monthly_token_total() {
                            let pct = if budget_limit > 0 {
                                (used as f64 / budget_limit as f64 * 100.0) as u64
                            } else {
                                0
                            };
                            text.push_str(&format!(
                                "Budget: {used}/{budget_limit} tokens ({pct}%) used this month\n"
                            ));
                        }
                    }
                }

                app.push_system_message(text.trim_end().to_string());
            }
            AppAction::UndoLastTurn => {
                let mut agent = agent.lock().await;
                let removed = agent.undo();
                if removed > 0 {
                    app.push_system_message(format!(
                        "Undid last turn ({removed} messages removed)."
                    ));
                } else {
                    app.push_system_message("Nothing to undo.".to_string());
                }
            }
            AppAction::UpdateSetting { key, value } => {
                let mut agent = agent.lock().await;
                if let Err(e) = agent.config_mut().apply_setting(&key, &value) {
                    app.push_system_message(format!("Warning: failed to sync agent config: {e}"));
                }
            }
            AppAction::SaveSession => {
                let mut agent = agent.lock().await;
                agent.auto_save();
                let session = agent.session();
                app.push_system_message(format!(
                    "Session saved: {} ({})",
                    session.meta.title, session.meta.id
                ));
            }
            AppAction::NewSession => {
                let mut agent = agent.lock().await;
                agent.new_session();
                app.push_system_message("New session started.".to_string());
            }
            AppAction::LoadSession { id } => {
                // Support partial ID matching (prefix)
                let full_id = match tamagotchi_core::session::list_sessions() {
                    Ok(sessions) => {
                        let matches: Vec<_> =
                            sessions.iter().filter(|s| s.id.starts_with(&id)).collect();
                        match matches.len() {
                            0 => None,
                            1 => Some(matches[0].id.clone()),
                            _ => {
                                app.push_system_message(format!(
                                    "Ambiguous session ID '{id}' — matches {} sessions. Be more specific.",
                                    matches.len()
                                ));
                                continue;
                            }
                        }
                    }
                    Err(_) => None,
                };
                let load_id = full_id.as_deref().unwrap_or(&id);

                let mut agent = agent.lock().await;
                match agent.load_session(load_id) {
                    Ok(()) => {
                        let session = agent.session();
                        let title = session.meta.title.clone();
                        let count = session.meta.message_count;
                        app.push_system_message(format!(
                            "Loaded session: {title} ({count} messages)"
                        ));
                    }
                    Err(e) => {
                        app.push_system_message(format!("Failed to load session: {e}"));
                    }
                }
            }
            AppAction::LaunchExternalEditor => {
                let current_text = app.composer.text();
                // Leave alternate screen and disable raw mode for editor
                let _ = disable_raw_mode();
                let _ = stdout().execute(LeaveAlternateScreen);

                let result = external_editor::open_external_editor(&current_text);

                // Restore terminal
                let _ = enable_raw_mode();
                let _ = stdout().execute(EnterAlternateScreen);
                terminal.clear()?;

                match result {
                    Ok(text) => {
                        let trimmed = text.trim_end();
                        app.composer.set_text(trimmed);
                    }
                    Err(e) => {
                        app.push_system_message(format!("Editor error: {e}"));
                    }
                }
            }
            AppAction::RunCustomize { actions } => {
                let data_dir = tamagotchi_core::config::Config::data_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("~/.tamagotchi"));
                let mut results: Vec<String> = Vec::new();

                for action in actions {
                    match action {
                        customize_popup::CustomizeAction::Install { id } => {
                            if let Some(def) = tamagotchi_customizations::catalog::find_by_id(&id) {
                                // For now, install without interactive credential prompts.
                                // Credentials can be set up via `tamagotchi init` or env vars.
                                match tokio::task::block_in_place(|| {
                                    tokio::runtime::Handle::current().block_on(
                                        tamagotchi_customizations::installer::install(
                                            def,
                                            &data_dir,
                                            &[],
                                            None,
                                        ),
                                    )
                                }) {
                                    Ok(()) => {
                                        // Record in DB
                                        if let Ok(db) = tamagotchi_core::db::Database::open() {
                                            let _ = db.insert_customization(
                                                def.id,
                                                def.name,
                                                &def.kind.to_string(),
                                                &def.category.to_string(),
                                            );
                                        }
                                        results.push(format!("Installed {}", def.name));
                                    }
                                    Err(e) => {
                                        results
                                            .push(format!("Failed to install {}: {e}", def.name));
                                    }
                                }
                            }
                        }
                        customize_popup::CustomizeAction::Uninstall { id } => {
                            if let Some(def) = tamagotchi_customizations::catalog::find_by_id(&id) {
                                match tamagotchi_customizations::installer::uninstall(
                                    def, &data_dir,
                                ) {
                                    Ok(()) => {
                                        if let Ok(db) = tamagotchi_core::db::Database::open() {
                                            let _ = db.delete_customization(def.id);
                                        }
                                        results.push(format!("Removed {}", def.name));
                                    }
                                    Err(e) => {
                                        results.push(format!("Failed to remove {}: {e}", def.name));
                                    }
                                }
                            }
                        }
                    }
                }

                if !results.is_empty() {
                    app.push_system_message(results.join("\n"));
                }
            }
            AppAction::ListSessions => match tamagotchi_core::session::list_sessions() {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        app.push_system_message("No saved sessions.".to_string());
                    } else {
                        let mut text = String::from("Saved sessions:\n");
                        for (i, s) in sessions.iter().take(20).enumerate() {
                            text.push_str(&format!(
                                "  {}. {} ({} msgs) - {}\n     /load {}\n",
                                i + 1,
                                s.title,
                                s.message_count,
                                &s.updated_at[..19.min(s.updated_at.len())],
                                &s.id[..8.min(s.id.len())],
                            ));
                        }
                        app.push_system_message(text.trim_end().to_string());
                    }
                }
                Err(e) => {
                    app.push_system_message(format!("Error listing sessions: {e}"));
                }
            },
            AppAction::Continue => {}
        }
    }
}
