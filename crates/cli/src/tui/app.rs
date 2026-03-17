use std::collections::VecDeque;
use std::time::Instant;

use anyhow::Result;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use tamagotchi_core::agent::AgentEvent;
use tamagotchi_core::config::Config;
use tamagotchi_heartbeat::scheduler::HeartbeatEvent;

use super::command_popup::CommandPopup;
use super::composer::Composer;
use super::customize_popup::{CustomizeAction, CustomizePopup};
use super::history::{ApprovalStatus, HistoryCell};
use super::layout;
use super::plan_overlay::{PlanOption, PlanOverlay};
use super::schedule_popup::{ScheduleAction, SchedulePopup};
use super::settings_popup::SettingsPopup;
use super::spinner;
use super::theme;

pub enum AppState {
    Idle,
    Streaming {
        start: Instant,
    },
    AwaitingApproval {
        respond: Option<oneshot::Sender<bool>>,
    },
    PlanReview,
}

pub enum AppAction {
    Continue,
    Quit,
    /// Request the event loop to spawn an agent call
    SendMessage {
        input: String,
        event_tx: mpsc::Sender<AgentEvent>,
        cancel: CancellationToken,
    },
    CompactHistory,
    ClearHistory,
    ShowUsage,
    UndoLastTurn,
    LaunchExternalEditor,
    UpdateSetting {
        key: String,
        value: String,
    },
    SaveSession,
    NewSession,
    LoadSession {
        id: String,
    },
    ListSessions,
    RestartGateway,
    RunCustomize {
        actions: Vec<CustomizeAction>,
    },
    PlanProceed {
        clear_context: bool,
    },
    RunScheduleActions {
        actions: Vec<ScheduleAction>,
    },
}

pub struct App<'a> {
    pub cells: Vec<HistoryCell>,
    pub state: AppState,
    pub composer: Composer<'a>,
    pub command_popup: CommandPopup,
    pub settings_popup: SettingsPopup,
    pub customize_popup: CustomizePopup,
    pub scroll_offset: usize,
    pub total_lines: usize,
    pub config: Config,
    pub event_rx: Option<mpsc::Receiver<AgentEvent>>,
    pub heartbeat_rx: Option<mpsc::Receiver<HeartbeatEvent>>,
    pub cancel_token: Option<CancellationToken>,
    auto_scroll: bool,
    /// Accumulated token usage for the current session
    pub session_prompt_tokens: u64,
    pub session_completion_tokens: u64,
    /// Messages queued by Tab during streaming, auto-submitted FIFO on turn complete
    pub queued_messages: VecDeque<String>,
    /// Whether the last agent turn ended with an error (pauses queue drain)
    pub last_turn_errored: bool,
    /// Whether the "[queue paused]" message has already been shown (prevents duplicates)
    pub queue_pause_notified: bool,
    pub plan_overlay: PlanOverlay,
    pub plan_mode: bool,
    pub schedule_popup: SchedulePopup,
}

impl<'a> App<'a> {
    pub fn new(config: Config, heartbeat_rx: Option<mpsc::Receiver<HeartbeatEvent>>) -> Self {
        Self {
            cells: Vec::new(),
            state: AppState::Idle,
            composer: Composer::new(),
            command_popup: CommandPopup::new(),
            settings_popup: SettingsPopup::new(),
            customize_popup: CustomizePopup::new(),
            scroll_offset: 0,
            total_lines: 0,
            config,
            event_rx: None,
            heartbeat_rx,
            cancel_token: None,
            auto_scroll: true,
            session_prompt_tokens: 0,
            session_completion_tokens: 0,
            queued_messages: VecDeque::new(),
            last_turn_errored: false,
            queue_pause_notified: false,
            plan_overlay: PlanOverlay::new(),
            plan_mode: false,
            schedule_popup: SchedulePopup::new(),
        }
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<AppAction> {
        use crossterm::event::{KeyCode, KeyModifiers};

        match &mut self.state {
            AppState::AwaitingApproval { respond } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(tx) = respond.take() {
                        let _ = tx.send(true);
                    }
                    if let Some(
                        HistoryCell::ShellApproval { status, .. }
                        | HistoryCell::ToolApproval { status, .. },
                    ) = self.cells.last_mut()
                    {
                        *status = ApprovalStatus::Approved;
                    }
                    self.state = AppState::Streaming {
                        start: Instant::now(),
                    };
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    if let Some(tx) = respond.take() {
                        let _ = tx.send(false);
                    }
                    if let Some(
                        HistoryCell::ShellApproval { status, .. }
                        | HistoryCell::ToolApproval { status, .. },
                    ) = self.cells.last_mut()
                    {
                        *status = ApprovalStatus::Denied;
                    }
                    self.state = AppState::Streaming {
                        start: Instant::now(),
                    };
                }
                _ => {}
            },
            AppState::PlanReview => {
                match key.code {
                    KeyCode::BackTab => {
                        self.plan_overlay.cycle();
                    }
                    KeyCode::Char('1') => {
                        self.plan_overlay.select(PlanOption::ClearAndProceed);
                    }
                    KeyCode::Char('2') => {
                        self.plan_overlay.select(PlanOption::ProceedWithContext);
                    }
                    KeyCode::Char('3') => {
                        self.plan_overlay.select(PlanOption::TypeFeedback);
                    }
                    KeyCode::Enter => {
                        let selected = self.plan_overlay.selected();
                        self.plan_overlay.dismiss();
                        self.state = AppState::Idle;
                        match selected {
                            PlanOption::ClearAndProceed => {
                                return Ok(AppAction::PlanProceed {
                                    clear_context: true,
                                });
                            }
                            PlanOption::ProceedWithContext => {
                                return Ok(AppAction::PlanProceed {
                                    clear_context: false,
                                });
                            }
                            PlanOption::TypeFeedback => {
                                // Dismiss to idle so user can type feedback
                            }
                        }
                    }
                    KeyCode::Esc => {
                        self.plan_overlay.dismiss();
                        self.state = AppState::Idle;
                    }
                    _ => {}
                }
            }
            AppState::Streaming { .. } => {
                if key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    // Cancel the in-flight agent task via token
                    if let Some(token) = self.cancel_token.take() {
                        token.cancel();
                    }
                    self.event_rx = None;
                    self.queued_messages.clear();
                    for cell in self.cells.iter_mut().rev() {
                        if let HistoryCell::Assistant { streaming, .. } = cell {
                            *streaming = false;
                            break;
                        }
                    }
                    self.cells.push(HistoryCell::System {
                        text: "[interrupted]".to_string(),
                    });
                    self.plan_mode = false;
                    self.state = AppState::Idle;
                } else if key.code == KeyCode::Up && key.modifiers.contains(KeyModifiers::ALT) {
                    // Pop last queued message back into composer for editing
                    if let Some(msg) = self.queued_messages.pop_back() {
                        self.composer.set_text(&msg);
                    }
                } else if key.code == KeyCode::Tab {
                    // Queue current composer text to auto-submit after turn completes
                    let text = self.composer.text().trim().to_string();
                    if !text.is_empty() {
                        self.queued_messages.push_back(text);
                        self.composer.set_text("");
                    }
                } else if key.code == KeyCode::Enter {
                    // No-op during streaming
                } else {
                    // Pass other keys to composer so user can type ahead
                    self.composer.handle_key(key);
                }
            }
            AppState::Idle => {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(AppAction::Quit);
                }

                // Handle error-paused queue: Enter resumes, Esc clears
                if self.last_turn_errored && !self.queued_messages.is_empty() {
                    match key.code {
                        KeyCode::Enter => {
                            self.last_turn_errored = false;
                            self.queue_pause_notified = false;
                            if let Some(queued) = self.queued_messages.pop_front() {
                                return self.handle_submit(&queued);
                            }
                            return Ok(AppAction::Continue);
                        }
                        KeyCode::Esc => {
                            self.last_turn_errored = false;
                            self.queue_pause_notified = false;
                            self.queued_messages.clear();
                            self.push_system_message("[queue cleared]".to_string());
                            return Ok(AppAction::Continue);
                        }
                        _ => {}
                    }
                }

                // Ctrl+L — clear visual transcript
                if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.cells.clear();
                    self.scroll_offset = 0;
                    self.auto_scroll = true;
                    return Ok(AppAction::Continue);
                }

                // Ctrl+D — quit when composer is empty (EOF)
                if key.code == KeyCode::Char('d')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.composer.is_empty()
                {
                    return Ok(AppAction::Quit);
                }

                // Ctrl+G — launch external editor
                if key.code == KeyCode::Char('g') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(AppAction::LaunchExternalEditor);
                }

                // Shift+Tab — toggle plan mode
                if key.code == KeyCode::BackTab {
                    self.plan_mode = !self.plan_mode;
                    if self.plan_mode {
                        self.push_system_message("[plan mode on]".to_string());
                    } else {
                        self.push_system_message("[plan mode off]".to_string());
                    }
                    return Ok(AppAction::Continue);
                }

                if self.settings_popup.is_visible() {
                    if let Some(action) = self.settings_popup.handle_key(key, &mut self.config)? {
                        return Ok(action);
                    }
                    return Ok(AppAction::Continue);
                }

                if self.customize_popup.is_visible() {
                    if let Some(actions) = self.customize_popup.handle_key(key) {
                        return Ok(AppAction::RunCustomize { actions });
                    }
                    return Ok(AppAction::Continue);
                }

                if self.schedule_popup.is_visible() {
                    if let Some(actions) = self.schedule_popup.handle_key(key) {
                        return Ok(AppAction::RunScheduleActions { actions });
                    }
                    return Ok(AppAction::Continue);
                }

                if self.command_popup.is_visible() {
                    match key.code {
                        KeyCode::Up => {
                            self.command_popup.move_up();
                            return Ok(AppAction::Continue);
                        }
                        KeyCode::Down => {
                            self.command_popup.move_down();
                            return Ok(AppAction::Continue);
                        }
                        KeyCode::Tab => {
                            if let Some(cmd) = self.command_popup.selected_command() {
                                let name = cmd.name.to_string();
                                self.composer.set_text(&name);
                                self.command_popup.dismiss();
                            }
                            return Ok(AppAction::Continue);
                        }
                        KeyCode::Enter => {
                            if let Some(cmd) = self.command_popup.selected_command() {
                                let name = cmd.name.to_string();
                                self.composer.set_text("");
                                self.command_popup.dismiss();
                                return self.handle_submit(&name);
                            }
                            return Ok(AppAction::Continue);
                        }
                        KeyCode::Esc => {
                            self.command_popup.dismiss();
                            self.composer.set_text("");
                            return Ok(AppAction::Continue);
                        }
                        _ => {
                            // Pass key to composer, then update filter
                            if let Some(text) = self.composer.handle_key(key) {
                                self.command_popup.dismiss();
                                return self.handle_submit(&text);
                            }
                            let text = self.composer.text();
                            self.command_popup.update_filter(&text);
                            return Ok(AppAction::Continue);
                        }
                    }
                }

                // ? — show keyboard shortcuts when composer is empty
                if key.code == KeyCode::Char('?')
                    && key.modifiers == KeyModifiers::NONE
                    && self.composer.is_empty()
                {
                    self.push_system_message(
                        "Keyboard Shortcuts:\n  \
                         Enter        — Send message\n  \
                         Shift+Enter  — New line\n  \
                         Up / Ctrl+P  — Previous history entry\n  \
                         Down / Ctrl+N — Next history entry\n  \
                         Esc          — Clear input\n  \
                         Ctrl+L       — Clear screen\n  \
                         Ctrl+D       — Quit (when empty)\n  \
                         Ctrl+G       — Open external editor ($EDITOR)\n  \
                         Tab          — Queue message while streaming\n  \
                         Alt+Up       — Edit last queued message\n  \
                         Ctrl+C       — Cancel / Quit\n  \
                         Shift+Tab    — Toggle plan mode\n  \
                         PageUp/Down  — Scroll transcript\n  \
                         /            — Show command menu"
                            .to_string(),
                    );
                    return Ok(AppAction::Continue);
                }

                match key.code {
                    KeyCode::Up
                        if self.composer.is_empty() && !self.composer.is_browsing_history() =>
                    {
                        self.scroll_offset = self.scroll_offset.saturating_add(1);
                        self.auto_scroll = false;
                        return Ok(AppAction::Continue);
                    }
                    KeyCode::Down if !self.composer.is_browsing_history() => {
                        self.scroll_offset = self.scroll_offset.saturating_sub(1);
                        if self.scroll_offset == 0 {
                            self.auto_scroll = true;
                        }
                        return Ok(AppAction::Continue);
                    }
                    KeyCode::PageUp => {
                        self.scroll_offset = self.scroll_offset.saturating_add(20);
                        self.auto_scroll = false;
                        return Ok(AppAction::Continue);
                    }
                    KeyCode::PageDown => {
                        self.scroll_offset = self.scroll_offset.saturating_sub(20);
                        if self.scroll_offset == 0 {
                            self.auto_scroll = true;
                        }
                        return Ok(AppAction::Continue);
                    }
                    _ => {}
                }

                if let Some(text) = self.composer.handle_key(key) {
                    return self.handle_submit(&text);
                }
                // Update popup filter after normal key input
                let text = self.composer.text();
                self.command_popup.update_filter(&text);
            }
        }

        Ok(AppAction::Continue)
    }

    pub fn push_system_message(&mut self, text: String) {
        self.cells.push(HistoryCell::System { text });
    }

    fn handle_submit(&mut self, input: &str) -> Result<AppAction> {
        let trimmed = input.trim();

        // Exact matches
        match trimmed {
            "quit" | "exit" | "/exit" => return Ok(AppAction::Quit),
            "help" | "/help" => {
                self.push_system_message(
                    "Commands:\n  \
                     /help      - Show this help\n  \
                     /settings  - Show/change settings (/settings <key> <value>)\n  \
                     /usage     - Show usage stats\n  \
                     /compact   - Compact conversation history\n  \
                     /clear     - Clear conversation\n  \
                     /undo      - Undo last agent turn\n  \
                     /tools     - List tools\n  \
                     /memory    - Show memory\n  \
                     /skills    - List skills\n  \
                     /doctor    - Run diagnostics\n  \
                     /history   - Show recent history\n  \
                     /sessions  - List saved sessions\n  \
                     /save      - Save current session\n  \
                     /load <id> - Load a saved session\n  \
                     /new       - Start new session\n  \
                     /customize - Integration marketplace\n  \
                     /schedule-tasks - Manage scheduled tasks\n  \
                     /restart   - Restart gateway and services\n  \
                     /logs      - Show recent logs\n  \
                     /plan      - Send message in plan mode\n  \
                     quit/exit  - Exit"
                        .to_string(),
                );
                return Ok(AppAction::Continue);
            }
            "/tools" => {
                let mut text = String::from("Built-in tools:\n");
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
                    text.push_str(&format!("  {name:<20} {desc}\n"));
                }
                if self.config.web.enabled {
                    text.push_str(&format!("  {:<20} Fetch a URL\n", "web_fetch"));
                    text.push_str(&format!("  {:<20} Search the web\n", "web_search"));
                }
                if self.config.security.host_audit {
                    text.push_str(&format!(
                        "  {:<20} Run host security audit\n",
                        "security_audit"
                    ));
                }

                text.push_str("\nUser tools:\n");
                match tamagotchi_tools::registry::ToolRegistry::new() {
                    Ok(registry) => {
                        let tools = registry.list_tools();
                        if tools.is_empty() {
                            text.push_str("  (none installed)");
                        } else {
                            for t in &tools {
                                text.push_str(&format!("  {t}\n"));
                            }
                        }
                    }
                    Err(e) => {
                        text.push_str(&format!("  Error loading tools: {e}"));
                    }
                }
                self.push_system_message(text.trim_end().to_string());
                return Ok(AppAction::Continue);
            }
            "/memory" => {
                let memory = tamagotchi_core::memory::load_memory_context(
                    self.config.memory.max_context_tokens,
                )?;
                let text = if memory.is_empty() {
                    "No memories loaded.".to_string()
                } else {
                    memory
                };
                self.push_system_message(text);
                return Ok(AppAction::Continue);
            }
            "/skills" => {
                let resolved_creds = self.config.resolve_credentials();
                let skills = tamagotchi_core::skills::load_all_skills(&resolved_creds)?;
                let text = if skills.is_empty() {
                    "No skills installed.".to_string()
                } else {
                    skills
                        .iter()
                        .map(|s| format!("  {}", s.summary_line()))
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                self.push_system_message(text);
                return Ok(AppAction::Continue);
            }
            "/history" => {
                match tamagotchi_core::logging::read_history_formatted(50) {
                    Ok(lines) => {
                        let text = if lines.is_empty() {
                            "No conversation history for today.".to_string()
                        } else {
                            lines.join("\n")
                        };
                        self.push_system_message(text);
                    }
                    Err(e) => {
                        self.push_system_message(format!("Error reading history: {e}"));
                    }
                }
                return Ok(AppAction::Continue);
            }
            "/logs" => {
                let log_path = match tamagotchi_core::config::Config::logs_dir() {
                    Ok(d) => d.join("tui.log"),
                    Err(e) => {
                        self.push_system_message(format!("Error resolving log directory: {e}"));
                        return Ok(AppAction::Continue);
                    }
                };
                let text = if log_path.exists() {
                    use std::io::{Read, Seek, SeekFrom};
                    const TAIL_BYTES: u64 = 32_768;
                    match std::fs::File::open(&log_path) {
                        Ok(mut f) => {
                            let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                            if len > TAIL_BYTES {
                                let _ = f.seek(SeekFrom::End(-(TAIL_BYTES as i64)));
                            }
                            let mut buf = String::new();
                            let _ = f.read_to_string(&mut buf);
                            let lines: Vec<&str> = buf.lines().collect();
                            let start = if len > TAIL_BYTES {
                                1 // skip partial first line after seek
                            } else {
                                lines.len().saturating_sub(50)
                            };
                            let tail: Vec<&str> = lines[start..].to_vec();
                            let tail = &tail[tail.len().saturating_sub(50)..];
                            tail.join("\n")
                        }
                        Err(e) => format!("Error reading log file: {e}"),
                    }
                } else {
                    "No log file found.".to_string()
                };
                self.push_system_message(if text.is_empty() {
                    "Log file is empty.".to_string()
                } else {
                    text
                });
                return Ok(AppAction::Continue);
            }
            "/doctor" => {
                let report = tamagotchi_core::doctor::run_diagnostics(&self.config);
                self.push_system_message(report.format());
                return Ok(AppAction::Continue);
            }
            "/settings" => {
                self.settings_popup.show();
                return Ok(AppAction::Continue);
            }
            "/customize" => {
                if let Ok(data_dir) = tamagotchi_core::config::Config::data_dir() {
                    self.customize_popup.show(&data_dir);
                } else {
                    self.push_system_message(
                        "Error: could not determine data directory".to_string(),
                    );
                }
                return Ok(AppAction::Continue);
            }
            "/schedule-tasks" => {
                self.schedule_popup.show();
                return Ok(AppAction::Continue);
            }
            "/restart" => {
                return Ok(AppAction::RestartGateway);
            }
            "/compact" => {
                return Ok(AppAction::CompactHistory);
            }
            "/clear" => {
                self.cells.clear();
                self.queued_messages.clear();
                return Ok(AppAction::ClearHistory);
            }
            "/usage" => {
                return Ok(AppAction::ShowUsage);
            }
            "/undo" => {
                return Ok(AppAction::UndoLastTurn);
            }
            "/sessions" => {
                return Ok(AppAction::ListSessions);
            }
            "/save" => {
                return Ok(AppAction::SaveSession);
            }
            "/new" => {
                self.cells.clear();
                self.queued_messages.clear();
                self.session_prompt_tokens = 0;
                self.session_completion_tokens = 0;
                return Ok(AppAction::NewSession);
            }
            _ => {}
        }

        // /plan — toggle plan mode or send message in plan mode
        if trimmed == "/plan" {
            self.plan_mode = !self.plan_mode;
            if self.plan_mode {
                self.push_system_message("[plan mode on]".to_string());
            } else {
                self.push_system_message("[plan mode off]".to_string());
            }
            return Ok(AppAction::Continue);
        }
        if let Some(rest) = trimmed.strip_prefix("/plan ") {
            self.plan_mode = true;
            let message = rest.trim();
            if message.is_empty() {
                self.push_system_message("[plan mode on]".to_string());
                return Ok(AppAction::Continue);
            }
            return self.handle_submit(message);
        }

        // Prefix command: /memory cleanup — list memory files for cleanup
        if trimmed == "/memory cleanup" {
            match tamagotchi_core::memory::list_memory_files() {
                Ok(files) => {
                    if files.is_empty() {
                        self.push_system_message("No memory files found.".to_string());
                    } else {
                        let mut text = String::from("Memory files (oldest first):\n");
                        for f in &files {
                            let modified = f
                                .modified_at
                                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_else(|| "unknown".to_string());
                            text.push_str(&format!(
                                "  {} ({} bytes, modified: {modified})\n",
                                f.filename, f.size_bytes
                            ));
                        }
                        text.push_str(
                            "\nTo delete a memory file, ask the agent to use write_memory.",
                        );
                        self.push_system_message(text.trim_end().to_string());
                    }
                }
                Err(e) => {
                    self.push_system_message(format!("Error listing memory files: {e}"));
                }
            }
            return Ok(AppAction::Continue);
        }

        // Prefix command: /load <session_id>
        if let Some(rest) = trimmed.strip_prefix("/load ") {
            let id = rest.trim().to_string();
            if id.is_empty() {
                self.push_system_message("Usage: /load <session_id>".to_string());
                return Ok(AppAction::Continue);
            }
            self.cells.clear();
            self.session_prompt_tokens = 0;
            self.session_completion_tokens = 0;
            return Ok(AppAction::LoadSession { id });
        }

        // Prefix commands: /settings <key> <value>
        if let Some(rest) = trimmed.strip_prefix("/settings ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let key = parts[0].to_string();
                let value = parts[1].to_string();
                match self.config.apply_setting(&key, &value) {
                    Ok(confirmation) => {
                        self.push_system_message(format!("Updated: {confirmation}"));
                        if let Err(e) = self.config.save() {
                            self.push_system_message(format!(
                                "Warning: failed to save config: {e}"
                            ));
                        }
                        return Ok(AppAction::UpdateSetting { key, value });
                    }
                    Err(e) => {
                        self.push_system_message(format!("Error: {e}"));
                        return Ok(AppAction::Continue);
                    }
                }
            } else {
                self.push_system_message(
                    "Usage: /settings <key> <value>\nUse /settings to see current values."
                        .to_string(),
                );
                return Ok(AppAction::Continue);
            }
        }

        // Prepare to send to agent
        self.cells.push(HistoryCell::User {
            text: input.to_string(),
        });
        self.cells.push(HistoryCell::Assistant {
            text: String::new(),
            streaming: true,
        });

        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>(256);
        self.event_rx = Some(event_rx);

        let cancel = CancellationToken::new();
        self.cancel_token = Some(cancel.clone());

        self.state = AppState::Streaming {
            start: Instant::now(),
        };
        self.auto_scroll = true;

        Ok(AppAction::SendMessage {
            input: input.to_string(),
            event_tx,
            cancel,
        })
    }

    pub fn process_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(delta) => {
                if let Some(HistoryCell::Assistant { text, .. }) = self.cells.last_mut() {
                    text.push_str(&delta);
                } else {
                    self.cells.push(HistoryCell::Assistant {
                        text: delta,
                        streaming: true,
                    });
                }
                if self.auto_scroll {
                    self.scroll_offset = 0;
                }
            }
            AgentEvent::ThinkingDelta(delta) => {
                // Check if the second-to-last cell is a Thinking cell (last is the
                // streaming Assistant cell), or if the last cell itself is Thinking.
                let len = self.cells.len();
                let thinking_idx = if len >= 2 {
                    if matches!(self.cells[len - 2], HistoryCell::Thinking { .. }) {
                        Some(len - 2)
                    } else if matches!(self.cells[len - 1], HistoryCell::Thinking { .. }) {
                        Some(len - 1)
                    } else {
                        None
                    }
                } else if len == 1 && matches!(self.cells[0], HistoryCell::Thinking { .. }) {
                    Some(0)
                } else {
                    None
                };

                if let Some(idx) = thinking_idx {
                    if let HistoryCell::Thinking { text, .. } = &mut self.cells[idx] {
                        text.push_str(&delta);
                    }
                } else {
                    // Insert thinking cell before the trailing Assistant cell so
                    // text deltas continue appending to the Assistant cell at the end.
                    let insert_pos = if len > 0
                        && matches!(self.cells[len - 1], HistoryCell::Assistant { .. })
                    {
                        len - 1
                    } else {
                        len
                    };
                    self.cells
                        .insert(insert_pos, HistoryCell::Thinking { text: delta });
                }
                if self.auto_scroll {
                    self.scroll_offset = 0;
                }
            }
            AgentEvent::ToolExecuting { name, args } => {
                self.cells.push(HistoryCell::ToolStart { name, args });
                if self.auto_scroll {
                    self.scroll_offset = 0;
                }
            }
            AgentEvent::ToolResult { name, result } => {
                let is_error = result.starts_with("Error:");
                self.cells.push(HistoryCell::ToolResult {
                    name,
                    output: result,
                    is_error,
                });
                if self.auto_scroll {
                    self.scroll_offset = 0;
                }
            }
            AgentEvent::ShellConfirmation { command, respond } => {
                self.cells.push(HistoryCell::ShellApproval {
                    command,
                    status: ApprovalStatus::Pending,
                });
                self.state = AppState::AwaitingApproval {
                    respond: Some(respond),
                };
                if self.auto_scroll {
                    self.scroll_offset = 0;
                }
            }
            AgentEvent::ToolConfirmation {
                tool_name,
                reason,
                respond,
            } => {
                self.cells.push(HistoryCell::ToolApproval {
                    tool_name,
                    reason,
                    status: ApprovalStatus::Pending,
                });
                self.state = AppState::AwaitingApproval {
                    respond: Some(respond),
                };
                if self.auto_scroll {
                    self.scroll_offset = 0;
                }
            }
            AgentEvent::Usage(usage) => {
                self.session_prompt_tokens += usage.prompt_tokens;
                self.session_completion_tokens += usage.completion_tokens;
                if let Ok(db) = tamagotchi_core::db::Database::open() {
                    let _ = db.log_token_usage(
                        usage.prompt_tokens,
                        usage.completion_tokens,
                        usage.prompt_tokens + usage.completion_tokens,
                    );
                }
            }
            AgentEvent::TurnComplete => {
                for cell in self.cells.iter_mut().rev() {
                    if let HistoryCell::Assistant { streaming, .. } = cell {
                        *streaming = false;
                        break;
                    }
                }
                self.last_turn_errored = false;
                self.queue_pause_notified = false;
                if self.plan_mode {
                    self.plan_mode = false;
                    let pct = self.compute_context_pct();
                    let name = self
                        .config
                        .user
                        .agent_name
                        .clone()
                        .unwrap_or_else(|| "Tamagotchi".to_string());
                    self.plan_overlay.show(pct, name);
                    self.state = AppState::PlanReview;
                } else {
                    self.state = AppState::Idle;
                }
            }
            AgentEvent::Error(e) => {
                self.cells.push(HistoryCell::System {
                    text: format!("Error: {e}"),
                });
                for cell in self.cells.iter_mut().rev() {
                    if let HistoryCell::Assistant { streaming, .. } = cell {
                        *streaming = false;
                        break;
                    }
                }
                self.last_turn_errored = true;
                self.plan_mode = false;
                self.state = AppState::Idle;
            }
        }
    }

    /// Handle the agent event channel closing (agent task finished or panicked).
    pub fn handle_agent_channel_closed(&mut self) {
        self.event_rx = None;
        for cell in self.cells.iter_mut().rev() {
            if let HistoryCell::Assistant { streaming, .. } = cell {
                *streaming = false;
                break;
            }
        }
        self.plan_mode = false;
        if !matches!(self.state, AppState::Idle) {
            self.state = AppState::Idle;
        }
    }

    pub fn process_heartbeat(&mut self, event: HeartbeatEvent) {
        match event {
            HeartbeatEvent::Message(msg) => {
                self.cells.push(HistoryCell::Heartbeat { text: msg });
                if self.auto_scroll {
                    self.scroll_offset = 0;
                }
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let show_status = !matches!(self.state, AppState::Idle);
        let composer_height = self.composer.height();
        let queue_preview_height = self.compute_queue_preview_height();
        let app_layout =
            layout::compute_layout(area, composer_height, show_status, queue_preview_height);

        self.render_transcript(frame, app_layout.transcript);
        if show_status {
            self.render_status(frame, app_layout.status);
        }
        if queue_preview_height > 0 {
            self.render_queue_preview(frame, app_layout.queue_preview);
        }
        self.composer.render(frame, app_layout.composer);
        self.render_footer(frame, app_layout.footer);
        self.plan_overlay.render(frame, app_layout.composer);
        self.command_popup.render(frame, app_layout.composer);
        self.settings_popup.render(frame, &self.config);
        self.customize_popup.render(frame);
        self.schedule_popup.render(frame);
    }

    fn compute_context_pct(&self) -> u8 {
        let max = self.config.conversation.max_history_tokens;
        if max == 0 {
            return 0;
        }
        let used = self.session_prompt_tokens + self.session_completion_tokens;
        ((used as f64 / max as f64) * 100.0).min(100.0) as u8
    }

    fn render_transcript(&mut self, frame: &mut Frame, area: Rect) {
        let width = area.width;
        let mut all_lines: Vec<Line<'static>> = Vec::new();

        let stream_elapsed = match &self.state {
            AppState::Streaming { start, .. } => Some(start.elapsed()),
            _ => None,
        };

        if self.cells.is_empty() {
            let title = match &self.config.user.agent_name {
                Some(name) => format!("{name} AI Assistant"),
                None => "Tamagotchi AI Assistant".to_string(),
            };
            all_lines.push(Line::from(Span::styled(title, theme::bold())));
            let subtitle = match &self.config.user.name {
                Some(name) => format!("Hey {name}! Type a message to begin."),
                None => "Type a message to begin.".to_string(),
            };
            all_lines.push(Line::from(Span::styled(subtitle, theme::dim())));
            all_lines.push(Line::default());
        }

        for cell in &self.cells {
            all_lines.extend(cell.render(width, stream_elapsed));
        }

        self.total_lines = all_lines.len();

        let visible_height = area.height as usize;
        let max_scroll = self.total_lines.saturating_sub(visible_height);
        self.scroll_offset = self.scroll_offset.min(max_scroll);
        let scroll_pos = max_scroll.saturating_sub(self.scroll_offset);

        // Clamp to u16 for ratatui's scroll API
        let scroll_pos_u16 = u16::try_from(scroll_pos).unwrap_or(u16::MAX);

        let paragraph = Paragraph::new(all_lines)
            .scroll((scroll_pos_u16, 0))
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);

        if self.total_lines > visible_height {
            let mut scrollbar_state =
                ScrollbarState::new(max_scroll).position(max_scroll - self.scroll_offset);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight),
                area,
                &mut scrollbar_state,
            );
        }
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let line = match &self.state {
            AppState::Streaming { start, .. } => {
                let elapsed_dur = start.elapsed();
                let elapsed_secs = elapsed_dur.as_secs();
                Line::from(vec![
                    Span::raw(" "),
                    spinner::status_spinner_frame(elapsed_dur),
                    Span::styled(format!(" Working ({elapsed_secs}s"), theme::tool_style()),
                    Span::styled(" • esc to interrupt)", theme::dim()),
                ])
            }
            AppState::AwaitingApproval { .. } => Line::from(vec![Span::styled(
                format!(" {} Approval needed — press y or n", theme::BULLET),
                theme::error_style(),
            )]),
            AppState::PlanReview => Line::from(vec![Span::styled(
                format!(" {} Plan ready — choose an action", theme::BULLET),
                theme::tool_style(),
            )]),
            AppState::Idle => Line::default(),
        };
        frame.render_widget(Paragraph::new(line), area);
    }

    /// Pop the next queued message (FIFO) for dispatch.
    pub fn pop_next_queued(&mut self) -> Option<String> {
        self.queued_messages.pop_front()
    }

    /// Submit a queued message (called from the event loop when a turn completes).
    pub fn handle_queued_submit(&mut self, input: &str) -> Result<AppAction> {
        self.handle_submit(input)
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let left = match &self.state {
            AppState::Idle if self.plan_mode => {
                "[plan]  •  shift+tab to toggle off  •  ? for shortcuts".to_string()
            }
            AppState::Idle => "? for shortcuts  •  quit to exit".to_string(),
            AppState::Streaming { .. } => {
                let count = self.queued_messages.len();
                if count > 0 {
                    format!("esc to cancel  •  tab to queue  •  ({count} queued)")
                } else {
                    "esc to cancel  •  tab to queue message".to_string()
                }
            }
            AppState::AwaitingApproval { .. } => "y to approve  •  n to deny".to_string(),
            AppState::PlanReview => {
                "shift+tab: cycle  •  1-3: jump  •  enter: confirm  •  esc: dismiss".to_string()
            }
        };
        let line = Line::from(Span::styled(format!(" {left}"), theme::dim()));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn compute_queue_preview_height(&self) -> u16 {
        let count = self.queued_messages.len();
        if count == 0 {
            return 0;
        }
        let shown = count.min(3) as u16;
        let overflow = if count > 3 { 1u16 } else { 0 };
        // header + shown messages + overflow + hint
        1 + shown + overflow + 1
    }

    fn render_queue_preview(&self, frame: &mut Frame, area: Rect) {
        let dim_italic = Style::default()
            .fg(theme::DIM_WHITE)
            .add_modifier(Modifier::ITALIC);
        let mut lines: Vec<Line<'static>> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Queued messages:".to_string(),
            theme::dim(),
        )));

        let count = self.queued_messages.len();
        let shown = count.min(3);
        for msg in self.queued_messages.iter().take(shown) {
            let truncated = if msg.len() > 60 {
                let end = msg
                    .char_indices()
                    .map(|(i, _)| i)
                    .take_while(|&i| i <= 57)
                    .last()
                    .unwrap_or(0);
                format!("{}...", &msg[..end])
            } else {
                msg.clone()
            };
            lines.push(Line::from(Span::styled(
                format!("  {} {truncated}", theme::TREE_END),
                dim_italic,
            )));
        }

        if count > 3 {
            lines.push(Line::from(Span::styled(
                format!("  ... and {} more", count - 3),
                theme::dim(),
            )));
        }

        lines.push(Line::from(Span::styled(
            "  Alt+Up to edit last".to_string(),
            theme::dim(),
        )));

        frame.render_widget(Paragraph::new(lines), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_app() -> App<'static> {
        let config = Config::default();
        App::new(config, None)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    // --- Up arrow: transcript scroll vs history navigation ---

    #[test]
    fn up_arrow_scrolls_transcript_when_composer_empty() {
        let mut app = make_app();
        assert!(app.composer.is_empty());
        assert_eq!(app.scroll_offset, 0);

        let action = app.handle_key(key(KeyCode::Up)).unwrap();
        assert!(matches!(action, AppAction::Continue));
        assert_eq!(app.scroll_offset, 1);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn up_arrow_does_not_scroll_transcript_when_composer_has_text() {
        let mut app = make_app();
        app.composer.set_text("hello");
        assert!(!app.composer.is_empty());

        let action = app.handle_key(key(KeyCode::Up)).unwrap();
        assert!(matches!(action, AppAction::Continue));
        // Should NOT have scrolled — Up was forwarded to composer for history
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn up_arrow_navigates_history_when_composer_has_text() {
        let mut app = make_app();
        // Submit a message to create history
        app.composer.set_text("first message");
        app.composer.handle_key(key(KeyCode::Enter));
        // Type something new
        app.composer.set_text("draft");

        app.handle_key(key(KeyCode::Up)).unwrap();
        // Composer should now show the history entry, not the draft
        assert_eq!(app.composer.text(), "first message");
        assert!(app.composer.is_browsing_history());
    }

    #[test]
    fn up_arrow_repeated_scrolls_transcript_incrementally() {
        let mut app = make_app();

        app.handle_key(key(KeyCode::Up)).unwrap();
        app.handle_key(key(KeyCode::Up)).unwrap();
        app.handle_key(key(KeyCode::Up)).unwrap();
        assert_eq!(app.scroll_offset, 3);
    }

    // --- Down arrow: transcript scroll vs history navigation ---

    #[test]
    fn down_arrow_scrolls_transcript_when_not_browsing_history() {
        let mut app = make_app();
        app.scroll_offset = 5;
        app.auto_scroll = false;

        let action = app.handle_key(key(KeyCode::Down)).unwrap();
        assert!(matches!(action, AppAction::Continue));
        assert_eq!(app.scroll_offset, 4);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn down_arrow_restores_auto_scroll_at_zero() {
        let mut app = make_app();
        app.scroll_offset = 1;
        app.auto_scroll = false;

        app.handle_key(key(KeyCode::Down)).unwrap();
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);
    }

    #[test]
    fn down_arrow_navigates_history_when_browsing() {
        let mut app = make_app();
        // Build history: two entries
        app.composer.set_text("msg1");
        app.composer.handle_key(key(KeyCode::Enter));
        app.composer.set_text("msg2");
        app.composer.handle_key(key(KeyCode::Enter));

        // Type a draft so Up goes to composer (not transcript scroll)
        app.composer.set_text("draft");
        app.handle_key(key(KeyCode::Up)).unwrap(); // -> msg2
        app.handle_key(key(KeyCode::Up)).unwrap(); // -> msg1
        assert!(app.composer.is_browsing_history());
        assert_eq!(app.composer.text(), "msg1");

        // Down should go forward in history, not scroll transcript
        app.handle_key(key(KeyCode::Down)).unwrap();
        assert_eq!(app.composer.text(), "msg2");
        // scroll_offset untouched
        assert_eq!(app.scroll_offset, 0);
    }

    // --- PageUp / PageDown ---

    #[test]
    fn page_up_scrolls_by_20() {
        let mut app = make_app();

        app.handle_key(key(KeyCode::PageUp)).unwrap();
        assert_eq!(app.scroll_offset, 20);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn page_down_scrolls_by_20() {
        let mut app = make_app();
        app.scroll_offset = 40;
        app.auto_scroll = false;

        app.handle_key(key(KeyCode::PageDown)).unwrap();
        assert_eq!(app.scroll_offset, 20);
        assert!(!app.auto_scroll);
    }

    #[test]
    fn page_down_restores_auto_scroll_at_zero() {
        let mut app = make_app();
        app.scroll_offset = 10;
        app.auto_scroll = false;

        app.handle_key(key(KeyCode::PageDown)).unwrap();
        assert_eq!(app.scroll_offset, 0);
        assert!(app.auto_scroll);
    }

    #[test]
    fn test_tab_queues_multiple() {
        let mut app = make_app();
        app.queued_messages.push_back("first".to_string());
        app.queued_messages.push_back("second".to_string());
        app.queued_messages.push_back("third".to_string());

        assert_eq!(app.queued_messages.len(), 3);
        assert_eq!(app.pop_next_queued(), Some("first".to_string()));
        assert_eq!(app.pop_next_queued(), Some("second".to_string()));
        assert_eq!(app.pop_next_queued(), Some("third".to_string()));
        assert_eq!(app.pop_next_queued(), None);
    }

    #[test]
    fn test_esc_clears_queue() {
        let mut app = make_app();
        app.queued_messages.push_back("a".to_string());
        app.queued_messages.push_back("b".to_string());

        app.queued_messages.clear();

        assert!(app.queued_messages.is_empty());
    }

    #[test]
    fn test_alt_up_pops_last() {
        let mut app = make_app();
        app.queued_messages.push_back("a".to_string());
        app.queued_messages.push_back("b".to_string());
        app.queued_messages.push_back("c".to_string());

        let last = app.queued_messages.pop_back();
        assert_eq!(last, Some("c".to_string()));
        assert_eq!(app.queued_messages.len(), 2);
        assert_eq!(app.queued_messages[0], "a");
        assert_eq!(app.queued_messages[1], "b");
    }

    #[test]
    fn test_drain_pops_front() {
        let mut app = make_app();
        app.queued_messages.push_back("first".to_string());
        app.queued_messages.push_back("second".to_string());

        let popped = app.pop_next_queued();
        assert_eq!(popped, Some("first".to_string()));
        assert_eq!(app.queued_messages.len(), 1);
        assert_eq!(app.queued_messages[0], "second");
    }

    #[test]
    fn test_queue_preview_height() {
        let mut app = make_app();

        // Empty queue
        assert_eq!(app.compute_queue_preview_height(), 0);

        // 1 message: header(1) + shown(1) + hint(1) = 3
        app.queued_messages.push_back("a".to_string());
        assert_eq!(app.compute_queue_preview_height(), 3);

        // 3 messages: header(1) + shown(3) + hint(1) = 5
        app.queued_messages.push_back("b".to_string());
        app.queued_messages.push_back("c".to_string());
        assert_eq!(app.compute_queue_preview_height(), 5);

        // 5 messages: header(1) + shown(3) + overflow(1) + hint(1) = 6
        app.queued_messages.push_back("d".to_string());
        app.queued_messages.push_back("e".to_string());
        assert_eq!(app.compute_queue_preview_height(), 6);
    }

    // --- /tools command ---

    fn last_system_text(app: &App) -> Option<String> {
        app.cells.iter().rev().find_map(|c| match c {
            HistoryCell::System { text } => Some(text.clone()),
            _ => None,
        })
    }

    #[test]
    fn tools_command_shows_builtin_tools() {
        let mut app = make_app();
        let action = app.handle_submit("/tools").unwrap();
        assert!(matches!(action, AppAction::Continue));

        let text = last_system_text(&app).expect("should have system message");
        assert!(text.contains("Built-in tools:"));
        assert!(text.contains("write_memory"));
        assert!(text.contains("read_memory"));
        assert!(text.contains("run_shell"));
        assert!(text.contains("apply_patch"));
        assert!(text.contains("read_pdf"));
        assert!(text.contains("manage_tasks"));
    }

    #[test]
    fn tools_command_shows_user_tools_section() {
        let mut app = make_app();
        app.handle_submit("/tools").unwrap();

        let text = last_system_text(&app).expect("should have system message");
        assert!(text.contains("User tools:"));
    }

    #[test]
    fn tools_command_hides_web_tools_when_disabled() {
        let mut app = make_app();
        app.config.web.enabled = false;
        app.handle_submit("/tools").unwrap();

        let text = last_system_text(&app).expect("should have system message");
        assert!(!text.contains("web_fetch"));
        assert!(!text.contains("web_search"));
    }

    #[test]
    fn tools_command_shows_web_tools_when_enabled() {
        let mut app = make_app();
        app.config.web.enabled = true;
        app.handle_submit("/tools").unwrap();

        let text = last_system_text(&app).expect("should have system message");
        assert!(text.contains("web_fetch"));
        assert!(text.contains("web_search"));
    }

    #[test]
    fn tools_command_hides_security_audit_when_disabled() {
        let mut app = make_app();
        app.config.security.host_audit = false;
        app.handle_submit("/tools").unwrap();

        let text = last_system_text(&app).expect("should have system message");
        assert!(!text.contains("security_audit"));
    }

    #[test]
    fn tools_command_shows_security_audit_when_enabled() {
        let mut app = make_app();
        app.config.security.host_audit = true;
        app.handle_submit("/tools").unwrap();

        let text = last_system_text(&app).expect("should have system message");
        assert!(text.contains("security_audit"));
    }
}
