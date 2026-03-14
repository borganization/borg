use std::time::Instant;

use anyhow::Result;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;
use tokio::sync::{mpsc, oneshot};

use tamagotchi_core::agent::AgentEvent;
use tamagotchi_core::config::Config;
use tamagotchi_heartbeat::scheduler::HeartbeatEvent;

use super::composer::Composer;
use super::history::{ApprovalStatus, HistoryCell};
use super::layout;
use super::theme;

pub enum AppState {
    Idle,
    Streaming {
        start: Instant,
    },
    AwaitingApproval {
        respond: Option<oneshot::Sender<bool>>,
    },
}

pub enum AppAction {
    Continue,
    Quit,
    /// Request the event loop to spawn an agent call
    SendMessage {
        input: String,
        event_tx: mpsc::Sender<AgentEvent>,
    },
    CompactHistory,
    ClearHistory,
    ShowUsage,
    UpdateSetting {
        key: String,
        value: String,
    },
}

pub struct App<'a> {
    pub cells: Vec<HistoryCell>,
    pub state: AppState,
    pub composer: Composer<'a>,
    pub scroll_offset: usize,
    pub total_lines: usize,
    pub config: Config,
    pub event_rx: Option<mpsc::Receiver<AgentEvent>>,
    pub heartbeat_rx: Option<mpsc::Receiver<HeartbeatEvent>>,
    auto_scroll: bool,
}

impl<'a> App<'a> {
    pub fn new(config: Config, heartbeat_rx: Option<mpsc::Receiver<HeartbeatEvent>>) -> Self {
        Self {
            cells: Vec::new(),
            state: AppState::Idle,
            composer: Composer::new(),
            scroll_offset: 0,
            total_lines: 0,
            config,
            event_rx: None,
            heartbeat_rx,
            auto_scroll: true,
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
                    if let Some(HistoryCell::ShellApproval { status, .. }) = self.cells.last_mut() {
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
                    if let Some(HistoryCell::ShellApproval { status, .. }) = self.cells.last_mut() {
                        *status = ApprovalStatus::Denied;
                    }
                    self.state = AppState::Streaming {
                        start: Instant::now(),
                    };
                }
                _ => {}
            },
            AppState::Streaming { .. } => {
                if key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    self.event_rx = None;
                    for cell in self.cells.iter_mut().rev() {
                        if let HistoryCell::Assistant { streaming, .. } = cell {
                            *streaming = false;
                            break;
                        }
                    }
                    self.cells.push(HistoryCell::System {
                        text: "[interrupted]".to_string(),
                    });
                    self.state = AppState::Idle;
                    self.composer.set_disabled(false);
                }
            }
            AppState::Idle => {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(AppAction::Quit);
                }

                match key.code {
                    KeyCode::Up => {
                        self.scroll_offset = self.scroll_offset.saturating_add(1);
                        self.auto_scroll = false;
                        return Ok(AppAction::Continue);
                    }
                    KeyCode::Down => {
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
            "quit" | "exit" => return Ok(AppAction::Quit),
            "help" | "/help" => {
                self.push_system_message(
                    "Commands:\n  \
                     /help      - Show this help\n  \
                     /settings  - Show/change settings (/settings <key> <value>)\n  \
                     /usage     - Show usage stats\n  \
                     /compact   - Compact conversation history\n  \
                     /clear     - Clear conversation\n  \
                     /tools     - List tools\n  \
                     /memory    - Show memory\n  \
                     /skills    - List skills\n  \
                     /history   - Show history\n  \
                     quit/exit  - Exit"
                        .to_string(),
                );
                return Ok(AppAction::Continue);
            }
            "/tools" => {
                let registry = tamagotchi_tools::registry::ToolRegistry::new()?;
                let tools = registry.list_tools();
                let text = if tools.is_empty() {
                    "No user tools installed.".to_string()
                } else {
                    tools.join("\n")
                };
                self.push_system_message(text);
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
                let skills = tamagotchi_core::skills::load_all_skills()?;
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
                match tamagotchi_core::logging::read_history(50) {
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
            "/settings" => {
                self.push_system_message(self.config.display_settings());
                return Ok(AppAction::Continue);
            }
            "/compact" => {
                return Ok(AppAction::CompactHistory);
            }
            "/clear" => {
                self.cells.clear();
                return Ok(AppAction::ClearHistory);
            }
            "/usage" => {
                return Ok(AppAction::ShowUsage);
            }
            _ => {}
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

        self.state = AppState::Streaming {
            start: Instant::now(),
        };
        self.composer.set_disabled(true);
        self.auto_scroll = true;

        Ok(AppAction::SendMessage {
            input: input.to_string(),
            event_tx,
        })
    }

    pub fn process_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(delta) => {
                if let Some(HistoryCell::Assistant { text, .. }) = self.cells.last_mut() {
                    text.push_str(&delta);
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
            AgentEvent::TurnComplete => {
                for cell in self.cells.iter_mut().rev() {
                    if let HistoryCell::Assistant { streaming, .. } = cell {
                        *streaming = false;
                        break;
                    }
                }
                self.state = AppState::Idle;
                self.composer.set_disabled(false);
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
                self.state = AppState::Idle;
                self.composer.set_disabled(false);
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
        if !matches!(self.state, AppState::Idle) {
            self.state = AppState::Idle;
            self.composer.set_disabled(false);
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
        let app_layout = layout::compute_layout(area, composer_height, show_status);

        self.render_transcript(frame, app_layout.transcript);
        if show_status {
            self.render_status(frame, app_layout.status);
        }
        self.composer.render(frame, app_layout.composer);
        self.render_footer(frame, app_layout.footer);
    }

    fn render_transcript(&mut self, frame: &mut Frame, area: Rect) {
        let width = area.width;
        let mut all_lines: Vec<Line<'static>> = Vec::new();

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
            all_lines.extend(cell.render(width));
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
                let elapsed = start.elapsed().as_secs();
                Line::from(vec![
                    Span::styled(
                        format!(" {} Working ({elapsed}s", theme::BULLET),
                        theme::tool_style(),
                    ),
                    Span::styled(" • esc to interrupt)", theme::dim()),
                ])
            }
            AppState::AwaitingApproval { .. } => Line::from(vec![Span::styled(
                format!(" {} Approval needed — press y or n", theme::BULLET),
                theme::error_style(),
            )]),
            AppState::Idle => Line::default(),
        };
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let left = match &self.state {
            AppState::Idle => "? for shortcuts  •  quit to exit",
            AppState::Streaming { .. } => "esc to cancel",
            AppState::AwaitingApproval { .. } => "y to approve  •  n to deny",
        };
        let line = Line::from(Span::styled(format!(" {left}"), theme::dim()));
        frame.render_widget(Paragraph::new(line), area);
    }
}
