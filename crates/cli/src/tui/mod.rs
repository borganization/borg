mod app;
mod colors;
mod command_popup;
mod composer;
mod external_editor;
mod file_popup;
mod goodbye;
mod history;
mod layout;
mod markdown;
pub(crate) mod migrate_popup;
mod paste_burst;
mod plan_overlay;
mod plugins_popup;
mod popup_utils;
mod schedule_popup;
mod settings_popup;
mod skills_popup;
mod status_popup;
pub(crate) mod theme;
mod tool_display;

use std::io::stdout;
use std::panic;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste, Event, EventStream};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;

// ============================================================================
// IMPORTANT: Custom mouse capture — DO NOT replace with EnableMouseCapture!
// ============================================================================
// crossterm's EnableMouseCapture enables ?1003h (any-event tracking) which
// captures ALL mouse events (including movement), breaking native text
// selection in terminals. We ONLY enable scroll-wheel tracking:
//   - ?1000h: Normal tracking (button press/release) — for scroll wheel
//   - ?1006h: SGR mouse mode — for coordinates >223
//
// EXCLUDED modes (DO NOT ADD):
//   - ?1002h: Button-event tracking (drag) — breaks native text selection
//             because click+drag events go to the app instead of the terminal.
//             Scrollbar drag is sacrificed to preserve text selection.
//   - ?1003h: Any-event tracking — captures ALL mouse events including bare
//             movement, completely breaks text selection.
//
// Native text selection (click+drag) MUST work in the transcript area.
// This has regressed multiple times. See CLAUDE.md "Mouse Interaction".
// ============================================================================

/// Enable mouse capture for scroll wheel only.
/// Does NOT enable drag (?1002h) or any-event (?1003h) tracking — both break
/// native text selection. Scrollbar click-to-jump still works via ?1000h.
struct EnableScrollMouseCapture;

impl crossterm::Command for EnableScrollMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        // ?1000h: Normal tracking — button press/release (includes scroll wheel
        //         and single clicks, e.g. scrollbar click-to-jump)
        // ?1006h: SGR extended mode — coordinates >223
        //
        // DO NOT add ?1002h — it captures drag events, breaking text selection.
        // DO NOT add ?1003h — it captures all mouse events, breaking text selection.
        f.write_str("\x1b[?1000h\x1b[?1006h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        // On Windows, fall back to crossterm's built-in mouse capture
        crossterm::event::EnableMouseCapture.execute_winapi()
    }
}

/// Disable the mouse capture modes enabled by EnableScrollMouseCapture.
struct DisableScrollMouseCapture;

impl crossterm::Command for DisableScrollMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        // Only disable the modes we enabled: ?1000h and ?1006h
        f.write_str("\x1b[?1006l\x1b[?1000l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        crossterm::event::DisableMouseCapture.execute_winapi()
    }
}
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

use borg_core::agent::{Agent, AgentEvent};
use borg_core::config::Config;
use borg_core::telemetry::BorgMetrics;
use borg_heartbeat::scheduler::{HeartbeatEvent, HeartbeatScheduler};

use app::{App, AppAction};
use history::HistoryCell;

/// Spawn the gateway server and/or iMessage monitor.
/// Always starts the gateway (native channels are detected via credentials inside GatewayServer).
fn spawn_gateway(config: &Config, shutdown: CancellationToken, metrics: BorgMetrics) -> bool {
    let gw_config = config.clone();
    #[allow(clippy::redundant_clone)]
    let gw_shutdown = shutdown.clone();
    let gw_metrics = metrics;
    tokio::spawn(async move {
        match borg_gateway::GatewayServer::new(gw_config, gw_shutdown, gw_metrics, None) {
            Ok(server) => {
                if let Err(e) = server.run().await {
                    let msg = e.to_string();
                    if msg.contains("address already in use") || msg.contains("AddrInUse") {
                        tracing::warn!("Gateway: {e}");
                    } else {
                        tracing::error!("Gateway exited with error: {e}");
                    }
                }
            }
            Err(e) => tracing::error!("Failed to initialize gateway: {e}"),
        }
    });

    // Start native iMessage monitor if channel is installed (mirrors service.rs)
    #[cfg(target_os = "macos")]
    if let Ok(data_dir) = Config::data_dir() {
        let imessage_dir = data_dir.join("channels/imessage");
        if imessage_dir.join("channel.toml").exists() {
            let probe = borg_gateway::imessage::probe::probe_imessage();
            match probe.status {
                borg_gateway::imessage::probe::ProbeStatus::Ok => {
                    let im_config = config.clone();
                    let im_shutdown = shutdown;
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

    true
}

/// Restart the gateway: cancel old token, reload config, spawn fresh server.
/// Returns the new shutdown token.
fn restart_gateway(gateway_shutdown: &Arc<Mutex<CancellationToken>>) -> String {
    let shutdown = gateway_shutdown.try_lock();
    let Ok(mut shutdown) = shutdown else {
        return "Gateway restart failed: could not acquire lock".to_string();
    };

    // Cancel existing gateway
    shutdown.cancel();

    // Create new token
    let new_token = CancellationToken::new();
    *shutdown = new_token.clone();

    // Reload config and spawn
    match Config::load() {
        Ok(config) => {
            spawn_gateway(&config, new_token, BorgMetrics::noop());
            "Gateway restarted.".to_string()
        }
        Err(e) => format!("Gateway restart failed: could not reload config: {e}"),
    }
}

/// Guard that restores terminal state on drop (both normal exit and early error return).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = stdout().execute(DisableBracketedPaste);
        let _ = stdout().execute(DisableScrollMouseCapture);
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
    }
}

pub async fn run() -> Result<()> {
    let config = Config::load()?;
    let metrics = BorgMetrics::from_config(&config);
    let mut agent = Agent::new(config.clone(), metrics.clone())?;

    // Start config hot reload watcher
    let config_path = Config::data_dir()?.join("config.toml");
    let _config_watcher =
        match borg_core::config_watcher::ConfigWatcher::start(config_path, config.clone()) {
            Ok(watcher) => {
                agent.set_config_watcher(watcher.subscribe());
                Some(watcher)
            }
            Err(e) => {
                tracing::warn!("Config watcher failed to start: {e}");
                None
            }
        };

    // Register vitals hook for passive health tracking
    if let Ok(vitals_hook) = borg_core::vitals::VitalsHook::new() {
        agent.hook_registry_mut().register(Box::new(vitals_hook));
    }

    // Register activity log hook for structured event logging
    if let Ok(activity_hook) = borg_core::activity_log::ActivityHook::new() {
        agent.hook_registry_mut().register(Box::new(activity_hook));
    }

    // Register bond hook for trust tracking (after vitals so events are available)
    if let Ok(bond_hook) = borg_core::bond::BondHook::new() {
        agent.hook_registry_mut().register(Box::new(bond_hook));
    }

    // Register evolution hook for XP tracking and specialization
    if config.evolution.enabled {
        if let Ok(evolution_hook) = borg_core::evolution::EvolutionHook::new() {
            agent.hook_registry_mut().register(Box::new(evolution_hook));
        }
    }

    // Try to resume the last session
    let mut resumed_info: Option<(String, usize)> = None;
    if let Ok(Some(session)) = borg_core::session::load_last_session() {
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
    let heartbeat_cancel = CancellationToken::new();
    let _heartbeat_guard = heartbeat_cancel.clone().drop_guard();
    let (heartbeat_rx, heartbeat_event_tx) = if config.heartbeat.enabled {
        let (hb_tx, hb_rx) = mpsc::channel::<HeartbeatEvent>(32);
        // Keep wake_tx alive so the wake_rx channel doesn't close immediately
        let (wake_tx, wake_rx) = mpsc::channel::<()>(8);
        let tz = config.user_timezone();
        let scheduler = HeartbeatScheduler::new(config.heartbeat.clone(), tz, wake_rx);
        let hb_cancel = heartbeat_cancel.clone();
        let hb_tx_clone = hb_tx.clone();
        tokio::spawn(async move {
            // Move wake_tx into the task to keep it alive for the scheduler's lifetime
            let _wake_tx = wake_tx;
            scheduler.run(hb_tx, hb_cancel).await;
        });
        (Some(hb_rx), Some(hb_tx_clone))
    } else {
        (None, None)
    };

    // Auto-start gateway if enabled and any channels are installed
    let gateway_shutdown_token = CancellationToken::new();
    let _gateway_guard = gateway_shutdown_token.clone().drop_guard();
    spawn_gateway(&config, gateway_shutdown_token.clone(), metrics.clone());
    let gateway_shutdown = Arc::new(Mutex::new(gateway_shutdown_token));

    // Query terminal background before entering alt screen (some terminals
    // don't respond to the query inside alternate screen).
    colors::query_terminal_bg();

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableScrollMouseCapture)?;
    stdout().execute(EnableBracketedPaste)?;

    // Guard ensures terminal is restored on any exit path (error or normal)
    let _guard = TerminalGuard;

    // Install panic hook that restores terminal before printing panic
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = stdout().execute(DisableBracketedPaste);
        let _ = stdout().execute(DisableScrollMouseCapture);
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config, heartbeat_rx, heartbeat_event_tx);
    if let Some((title, count)) = resumed_info {
        app.push_system_message(format!("Resumed session: {title} ({count} messages)"));
    }
    // Show vitals + evolution header on session start
    if let Ok(db) = borg_core::db::Database::open() {
        if let Ok(state) = db.get_vitals_state() {
            let now = chrono::Utc::now();
            let state = borg_core::vitals::apply_decay(&state, now);
            let drift = borg_core::vitals::detect_drift(&state, now);
            if let Some(notice) = borg_core::vitals::format_drift_notice(&drift) {
                app.push_system_message(notice);
            }
        }
    }
    // Auto-trigger first conversation if SETUP.md exists (fresh onboarding)
    if let Ok(data_dir) = Config::data_dir() {
        let setup_path = data_dir.join("SETUP.md");
        if setup_path.exists() {
            app.queued_messages.push_back(app::QueuedMessage {
                text: String::new(),
                images: Vec::new(),
            });
        }
    }

    let mut event_stream = EventStream::new();
    let tick_rate = Duration::from_millis(100);

    run_event_loop(
        &mut terminal,
        &mut app,
        &agent,
        &mut event_stream,
        tick_rate,
        &gateway_shutdown,
    )
    .await
}

/// Delegate to the shared heartbeat turn implementation.
async fn run_heartbeat_turn(config: &Config) -> Option<String> {
    crate::service::execute_heartbeat_turn(config).await
}

/// If the app just became idle and has queued messages, auto-submit the next one.
/// Remaining messages stay in the queue and drain on subsequent TurnComplete events.
/// If the last turn errored, pause the queue and notify the user.
fn drain_queued_if_idle(app: &mut App<'_>) -> Result<AppAction> {
    if matches!(app.state, app::AppState::Idle) {
        if app.last_turn_errored && !app.queued_messages.is_empty() {
            if !app.queue_pause_notified {
                app.push_system_message(
                    "[queue paused — enter to resume, esc to clear]".to_string(),
                );
                app.queue_pause_notified = true;
            }
            return Ok(AppAction::Continue);
        }
        if let Some(qm) = app.pop_next_queued() {
            return app.handle_queued_submit(qm);
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
    gateway_shutdown: &Arc<Mutex<CancellationToken>>,
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
                match event {
                    HeartbeatEvent::Fire => {
                        // Run a heartbeat agent turn in the background
                        let hb_config = app.config.clone();
                        let hb_tx_clone = app.heartbeat_event_tx.clone();
                        tokio::spawn(async move {
                            if let Some(msg) = run_heartbeat_turn(&hb_config).await {
                                if let Some(tx) = hb_tx_clone {
                                    let _ = tx.send(HeartbeatEvent::Message(msg)).await;
                                }
                            }
                        });
                    }
                    _ => {
                        app.process_heartbeat(event);
                    }
                }
                AppAction::Continue
            }

            // Doctor diagnostic events
            event = async {
                if let Some(rx) = &mut app.doctor_rx {
                    rx.recv().await
                } else {
                    std::future::pending().await
                }
            } => {
                if let Some(ev) = event {
                    app.process_doctor_event(ev);
                } else {
                    app.doctor_rx = None;
                }
                AppAction::Continue
            }

            // Terminal events
            maybe_event = event_stream.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) => app.handle_key(key)?,
                    Some(Ok(Event::Paste(text))) => app.handle_paste(text),
                    Some(Ok(Event::Mouse(mouse))) => app.handle_mouse(mouse),
                    Some(Ok(Event::Resize(_, _))) => AppAction::Continue,
                    Some(Err(_)) | None => AppAction::Quit,
                    _ => AppAction::Continue,
                }
            }

            // Tick for status bar elapsed time + throbber animation + paste burst flush
            _ = tick_interval.tick() => {
                app.tick_throbber();
                app.tick_paste_burst();
                drain_queued_if_idle(app)?
            }
        };

        match action {
            AppAction::Quit => {
                agent.lock().await.close_browser().await;
                return Ok(());
            }
            AppAction::SendMessage {
                input,
                images,
                event_tx,
                cancel,
            } => {
                // Set up steer channel for mid-turn user guidance
                let (steer_tx, steer_rx) = tokio::sync::mpsc::unbounded_channel();
                app.steer_tx = Some(steer_tx);
                app.pending_steers.clear();

                let agent_clone = Arc::clone(agent);
                tokio::spawn(async move {
                    let mut agent = agent_clone.lock().await;
                    agent.set_steer_channel(steer_rx);
                    if images.is_empty() {
                        if let Err(e) = agent
                            .send_message_with_cancel(&input, event_tx.clone(), cancel)
                            .await
                        {
                            let _ = event_tx.send(AgentEvent::Error(e.to_string())).await;
                        }
                    } else {
                        // Build multimodal message with text + images
                        use base64::Engine as _;
                        use borg_core::types::{ContentPart, MediaData, Message};
                        let engine = base64::engine::general_purpose::STANDARD;
                        let mut parts = vec![ContentPart::Text(input)];
                        for img in images {
                            parts.push(ContentPart::ImageBase64 {
                                media: MediaData {
                                    mime_type: img.mime_type,
                                    data: engine.encode(&img.data),
                                    filename: None,
                                },
                            });
                        }
                        let msg = Message::user_multimodal(parts);
                        if let Err(e) = agent.send_message_raw(msg, event_tx.clone(), cancel).await
                        {
                            let _ = event_tx.send(AgentEvent::Error(e.to_string())).await;
                        }
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
                    match borg_core::logging::count_messages_for_period(days) {
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
                    if let Ok(db) = borg_core::db::Database::open() {
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
                        if let Ok(Some(cost)) = db.monthly_total_cost() {
                            text.push_str(&format!("Estimated cost: ${cost:.4}\n"));
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
            AppAction::RewindTo { nth_user_message } => {
                let mut agent = agent.lock().await;
                let removed = agent.rewind_to_nth_user_message(nth_user_message);
                if removed > 0 {
                    app.push_system_message(format!(
                        "Rewound conversation ({removed} messages removed). Edit and re-send."
                    ));
                } else {
                    app.push_system_message("Nothing to rewind.".to_string());
                }
            }
            AppAction::UpdateSetting { key, value } => {
                let mut agent = agent.lock().await;
                if let Err(e) = agent.config_mut().apply_setting(&key, &value) {
                    app.push_system_message(format!("Warning: failed to sync agent config: {e}"));
                }
            }
            AppAction::ConfigReloaded => {
                let mut agent = agent.lock().await;
                *agent.config_mut() = app.config.clone();
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
                let full_id = match borg_core::session::list_sessions() {
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
            AppAction::RunPlugins { actions } => {
                let data_dir = match borg_core::config::Config::data_dir() {
                    Ok(dir) => dir,
                    Err(e) => {
                        app.push_system_message(format!("Failed to resolve data directory: {e}"));
                        continue;
                    }
                };
                let mut results: Vec<String> = Vec::new();

                for action in actions {
                    match action {
                        plugins_popup::PluginAction::Install { id, credentials } => {
                            if let Some(def) = borg_plugins::catalog::find_by_id(&id) {
                                match borg_plugins::installer::install(
                                    def,
                                    &data_dir,
                                    &credentials,
                                    None,
                                )
                                .await
                                {
                                    Ok(install_result) => {
                                        // Record in DB + store file hashes
                                        if let Ok(db) = borg_core::db::Database::open() {
                                            if let Err(e) = db.insert_plugin(
                                                def.id,
                                                def.name,
                                                &def.kind.to_string(),
                                                &def.category.to_string(),
                                            ) {
                                                tracing::warn!("Failed to record plugin: {e}");
                                            }

                                            // Store file hashes for integrity verification
                                            for (path, hash) in &install_result.file_hashes {
                                                if let Err(e) =
                                                    db.insert_file_hash(def.id, path, hash)
                                                {
                                                    tracing::warn!(
                                                        "Failed to store file hash for {path}: {e}"
                                                    );
                                                }
                                            }

                                            // Register installed tool/channel
                                            let item_name = if def.is_native {
                                                def.id.rsplit('/').next().unwrap_or(def.id)
                                            } else if let Some(first_tmpl) = def.templates.first() {
                                                first_tmpl
                                                    .relative_path
                                                    .split('/')
                                                    .next()
                                                    .unwrap_or(def.id)
                                            } else {
                                                def.id
                                            };
                                            let runtime =
                                                if def.is_native { "native" } else { "python" };
                                            match def.kind {
                                                borg_plugins::PluginKind::Tool => {
                                                    if let Err(e) = db.insert_installed_tool(
                                                        item_name,
                                                        def.description,
                                                        runtime,
                                                        def.id,
                                                    ) {
                                                        tracing::warn!(
                                                            "Failed to register tool: {e}"
                                                        );
                                                    }
                                                }
                                                borg_plugins::PluginKind::Channel => {
                                                    let webhook = format!("/webhook/{item_name}");
                                                    if let Err(e) = db.insert_installed_channel(
                                                        item_name,
                                                        def.description,
                                                        runtime,
                                                        def.id,
                                                        &webhook,
                                                    ) {
                                                        tracing::warn!(
                                                            "Failed to register channel: {e}"
                                                        );
                                                    }
                                                }
                                            }
                                        }

                                        // Wire credential entries + gateway config in one load/save
                                        if !install_result.credential_entries.is_empty()
                                            || def.kind == borg_plugins::PluginKind::Channel
                                        {
                                            if let Ok(mut cfg) = Config::load() {
                                                for entry in &install_result.credential_entries {
                                                    cfg.credentials.insert(
                                                        entry.key.clone(),
                                                        borg_core::config::CredentialValue::Ref(
                                                            borg_core::secrets_resolve::SecretRef::Keychain {
                                                                service: entry.service.clone(),
                                                                account: entry.account.clone(),
                                                            },
                                                        ),
                                                    );
                                                }
                                                let _ = cfg.save();
                                            }
                                        }
                                        let mut msg = format!("Installed {}", def.name);
                                        for note in &install_result.notes {
                                            msg.push_str(&format!("\n  {note}"));
                                        }
                                        if def.kind == borg_plugins::PluginKind::Channel {
                                            let gw_msg = restart_gateway(gateway_shutdown);
                                            msg.push_str(&format!("\n  {gw_msg}"));
                                        }
                                        results.push(msg);
                                    }
                                    Err(e) => {
                                        results
                                            .push(format!("Failed to install {}: {e}", def.name));
                                    }
                                }
                            }
                        }
                        plugins_popup::PluginAction::Uninstall { id } => {
                            if let Some(def) = borg_plugins::catalog::find_by_id(&id) {
                                match borg_plugins::installer::uninstall(def, &data_dir) {
                                    Ok(()) => {
                                        if let Ok(db) = borg_core::db::Database::open() {
                                            let _ = db.delete_plugin(def.id);
                                        }
                                        // Remove credential entries from config
                                        if !def.required_credentials.is_empty() {
                                            if let Ok(mut cfg) = Config::load() {
                                                for cred in def.required_credentials {
                                                    cfg.credentials.remove(cred.key);
                                                }
                                                let _ = cfg.save();
                                            }
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
            AppAction::ListSessions => match borg_core::session::list_sessions() {
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
            AppAction::PlanProceed { clear_context } => {
                let proceed_msg = if clear_context {
                    // Extract plan text before clearing so the agent knows what to do
                    let plan_text = app
                        .cells
                        .iter()
                        .rev()
                        .find_map(|c| {
                            if let HistoryCell::Assistant { text, .. } = c {
                                if !text.trim().is_empty() {
                                    return Some(text.clone());
                                }
                            }
                            None
                        })
                        .unwrap_or_default();

                    let mut agent = agent.lock().await;
                    agent.clear_history();
                    app.cells.clear();
                    app.session_prompt_tokens = 0;
                    app.session_completion_tokens = 0;

                    if plan_text.is_empty() {
                        "Proceed with the plan as outlined.".to_string()
                    } else {
                        format!(
                            "Here is the plan we agreed on:\n\n{plan_text}\n\nProceed with this plan."
                        )
                    }
                } else {
                    "Proceed with the plan as outlined.".to_string()
                };
                app.queued_messages.push_front(app::QueuedMessage {
                    text: proceed_msg,
                    images: Vec::new(),
                });
            }
            AppAction::RunScheduleActions { actions } => {
                let mut results: Vec<String> = Vec::new();
                if let Ok(db) = borg_core::db::Database::open() {
                    for action in actions {
                        match action {
                            schedule_popup::ScheduleAction::ToggleStatus {
                                task_id,
                                new_status,
                            } => match db.update_task_status(&task_id, &new_status) {
                                Ok(true) => {
                                    results.push(format!(
                                        "Task {} {new_status}",
                                        &task_id[..8.min(task_id.len())]
                                    ));
                                }
                                Ok(false) => {
                                    results.push(format!(
                                        "Task {} not found",
                                        &task_id[..8.min(task_id.len())]
                                    ));
                                }
                                Err(e) => results.push(format!("Error: {e}")),
                            },
                            schedule_popup::ScheduleAction::UpdateSchedule {
                                task_id,
                                schedule_type,
                                new_expr,
                            } => {
                                let update = borg_core::db::UpdateTask {
                                    name: None,
                                    prompt: None,
                                    schedule_type: Some(&schedule_type),
                                    schedule_expr: Some(&new_expr),
                                    timezone: None,
                                };
                                match db.update_task(&task_id, &update) {
                                    Ok(true) => {
                                        results.push(format!(
                                            "Task {} schedule updated to {new_expr}",
                                            &task_id[..8.min(task_id.len())]
                                        ));
                                    }
                                    Ok(false) => {
                                        results.push(format!(
                                            "Task {} not found",
                                            &task_id[..8.min(task_id.len())]
                                        ));
                                    }
                                    Err(e) => results.push(format!("Error: {e}")),
                                }
                            }
                            schedule_popup::ScheduleAction::DeleteTask { task_id } => {
                                match db.delete_task(&task_id) {
                                    Ok(true) => {
                                        results.push(format!(
                                            "Task {} deleted",
                                            &task_id[..8.min(task_id.len())]
                                        ));
                                    }
                                    Ok(false) => {
                                        results.push(format!(
                                            "Task {} not found",
                                            &task_id[..8.min(task_id.len())]
                                        ));
                                    }
                                    Err(e) => results.push(format!("Error: {e}")),
                                }
                            }
                        }
                    }
                } else {
                    results.push("Error: could not open database".to_string());
                }
                if !results.is_empty() {
                    app.push_system_message(results.join("\n"));
                }
            }
            AppAction::RunSkillActions { actions } => {
                let mut results: Vec<String> = Vec::new();
                if let Ok(db) = borg_core::db::Database::open() {
                    for action in actions {
                        match action {
                            skills_popup::SkillAction::SetEnabled { name, enabled } => {
                                let key = format!("skills.entries.{name}.enabled");
                                let val = enabled.to_string();
                                match app.config.apply_setting(&key, &val) {
                                    Ok(_) => {
                                        let _ = db.set_setting(&key, &val);
                                        let status = if enabled { "enabled" } else { "disabled" };
                                        results.push(format!("{name}: {status}"));
                                    }
                                    Err(e) => {
                                        results.push(format!("Error updating {name}: {e}"));
                                    }
                                }
                            }
                        }
                    }
                } else {
                    results.push("Error: could not open database".to_string());
                }
                if !results.is_empty() {
                    app.push_system_message(format!("Skills updated:\n{}", results.join("\n")));
                }
            }
            AppAction::RunMigration { actions } => {
                for action in actions {
                    match action {
                        migrate_popup::MigrateAction::Apply { plan, source_data } => {
                            let borg_dir = match borg_core::config::Config::data_dir() {
                                Ok(d) => d,
                                Err(e) => {
                                    app.push_system_message(format!("Migration error: {e}"));
                                    continue;
                                }
                            };
                            match borg_core::migrate::apply::apply_plan(
                                &plan,
                                &source_data,
                                &borg_dir,
                            ) {
                                Ok(result) => {
                                    let mut msg = String::from("Migration complete:");
                                    if result.config_changes_applied > 0 {
                                        msg.push_str(&format!(
                                            "\n  {} config change(s) applied",
                                            result.config_changes_applied
                                        ));
                                    }
                                    if result.credentials_added > 0 {
                                        msg.push_str(&format!(
                                            "\n  {} credential(s) added",
                                            result.credentials_added
                                        ));
                                    }
                                    if result.memory_files_copied > 0 {
                                        msg.push_str(&format!(
                                            "\n  {} memory file(s) copied",
                                            result.memory_files_copied
                                        ));
                                    }
                                    if result.persona_copied {
                                        msg.push_str("\n  Persona copied to IDENTITY.md");
                                    }
                                    if result.skills_copied > 0 {
                                        msg.push_str(&format!(
                                            "\n  {} skill(s) copied",
                                            result.skills_copied
                                        ));
                                    }
                                    for warning in &result.warnings {
                                        msg.push_str(&format!("\n  Warning: {warning}"));
                                    }
                                    app.push_system_message(msg);
                                    // Reload config to pick up migrated settings
                                    app.config =
                                        borg_core::config::Config::load().unwrap_or_default();
                                }
                                Err(e) => {
                                    app.push_system_message(format!("Migration failed: {e}"));
                                }
                            }
                        }
                    }
                }
            }
            AppAction::RestartGateway => {
                let msg = restart_gateway(gateway_shutdown);
                app.push_system_message(msg);
            }
            AppAction::SelfUpdate { dev } => {
                app.push_system_message(format!(
                    "Checking for updates{}...",
                    if dev { " (including pre-releases)" } else { "" }
                ));
                terminal.draw(|f| app.render(f))?;
                match borg_core::update::perform_update(dev).await {
                    Ok(result) => match result.status {
                        borg_core::update::UpdateStatus::AlreadyUpToDate => {
                            app.push_system_message(format!(
                                "Already up to date ({})",
                                result.current_version
                            ));
                        }
                        borg_core::update::UpdateStatus::Updated { from, to } => {
                            app.push_system_message(format!(
                                "Updated borg: {from} → {to}\nPlease restart borg to use the new version."
                            ));
                        }
                    },
                    Err(e) => {
                        app.push_system_message(format!("Update failed: {e}"));
                    }
                }
            }
            AppAction::Uninstall => {
                // Step 1: Daemon service
                app.push_system_message("Removing daemon service...".to_string());
                terminal.draw(|f| app.render(f))?;
                if let Err(e) = crate::service::uninstall_service() {
                    tracing::debug!("Service uninstall skipped: {e}");
                }

                // Step 2: Data directory
                app.push_system_message("Removing data directory (~/.borg/)...".to_string());
                terminal.draw(|f| app.render(f))?;
                if let Ok(data_dir) = borg_core::config::Config::data_dir() {
                    if let Err(e) = std::fs::remove_dir_all(&data_dir) {
                        app.push_system_message(format!("Warning: failed to remove data dir: {e}"));
                        terminal.draw(|f| app.render(f))?;
                    }
                }

                // Step 3: Binary
                app.push_system_message("Removing binary...".to_string());
                terminal.draw(|f| app.render(f))?;
                if let Ok(exe) = std::env::current_exe() {
                    let exe = exe.canonicalize().unwrap_or(exe);
                    if let Err(e) = std::fs::remove_file(&exe) {
                        tracing::debug!("Could not remove binary: {e}");
                    }
                }

                // Close browser
                agent.lock().await.close_browser().await;

                // Fullscreen ASCII art goodbye
                goodbye::render(terminal)?;
                tokio::time::sleep(Duration::from_secs(3)).await;
                return Ok(());
            }
            AppAction::RunDoctor => {
                app.push_system_message("Borg Doctor\n───────────".to_string());
                let (tx, rx) = tokio::sync::mpsc::channel(32);
                app.doctor_rx = Some(rx);
                let config = app.config.clone();
                tokio::task::spawn_blocking(move || {
                    use borg_core::doctor::DiagnosticRunner;
                    let mut runner = DiagnosticRunner::new();
                    let mut all_checks: Vec<borg_core::doctor::DiagnosticCheck> = Vec::new();
                    while let Some(label) = runner.peek_label(&config) {
                        let _ = tx.blocking_send(app::DoctorEvent::Analyzing {
                            label: label.to_string(),
                        });
                        if let Some((_label, checks)) = runner.next_step(&config) {
                            all_checks.extend(checks.clone());
                            let _ = tx.blocking_send(app::DoctorEvent::Result {
                                label: label.to_string(),
                                checks,
                            });
                        }
                    }
                    let report = borg_core::doctor::DiagnosticReport { checks: all_checks };
                    let (pass, warn, fail) = report.counts();
                    let _ = tx.blocking_send(app::DoctorEvent::Done { pass, warn, fail });
                });
            }
            AppAction::Continue => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::Command;

    /// Verify that our custom mouse capture preserves native text selection.
    /// Only ?1000h (scroll wheel / single click) and ?1006h (SGR coords) are
    /// enabled. ?1002h (drag) and ?1003h (any-event) MUST be excluded — both
    /// break native text selection. This test prevents a recurring regression.
    #[test]
    fn enable_scroll_mouse_capture_preserves_text_selection() {
        let mut buf = String::new();
        EnableScrollMouseCapture.write_ansi(&mut buf).unwrap();

        // Must include the modes we need
        assert!(buf.contains("?1000h"), "must enable normal button tracking");
        assert!(
            buf.contains("?1006h"),
            "must enable SGR extended coordinates"
        );

        // Must NOT include drag tracking — breaks native text selection
        assert!(
            !buf.contains("?1002h"),
            "MUST NOT enable ?1002h (drag tracking) — it breaks native text selection"
        );

        // Must NOT include any-event tracking — breaks native text selection
        assert!(
            !buf.contains("?1003h"),
            "MUST NOT enable ?1003h (any-event tracking) — it breaks native text selection"
        );
    }

    #[test]
    fn disable_scroll_mouse_capture_reverses_enable() {
        let mut buf = String::new();
        DisableScrollMouseCapture.write_ansi(&mut buf).unwrap();

        assert!(buf.contains("?1000l"), "must disable normal tracking");
        assert!(buf.contains("?1006l"), "must disable SGR mode");
        // Must not reference modes we don't enable
        assert!(!buf.contains("?1002l"), "must not reference ?1002 at all");
        assert!(!buf.contains("?1003l"), "must not reference ?1003 at all");
    }
}
