use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use borg_core::config::Config;
use borg_core::db::Database;
use borg_core::settings::SettingSource;

use crate::onboarding::{models_for_provider, PROVIDERS};

use super::app::AppAction;
use super::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    Bool,
    Text,
    Float,
    Uint,
    Select,
}

#[derive(Clone, Copy)]
pub struct SettingEntry {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: SettingKind,
    pub category: &'static str,
}

#[derive(Clone)]
pub enum EditMode {
    Browsing,
    Editing { buffer: String },
}

pub struct SettingsPopup {
    visible: bool,
    entries: &'static [SettingEntry],
    selected: usize,
    mode: EditMode,
    status_message: Option<(String, bool)>, // (message, is_success)
    db: Option<Database>,
    provider_index: usize,
    model_index: usize,
}

const SETTINGS: &[SettingEntry] = &[
    SettingEntry {
        key: "provider",
        label: "Provider",
        kind: SettingKind::Select,
        category: "LLM",
    },
    SettingEntry {
        key: "model",
        label: "Model",
        kind: SettingKind::Select,
        category: "LLM",
    },
    SettingEntry {
        key: "temperature",
        label: "Temperature",
        kind: SettingKind::Float,
        category: "LLM",
    },
    SettingEntry {
        key: "max_tokens",
        label: "Max tokens",
        kind: SettingKind::Uint,
        category: "LLM",
    },
    SettingEntry {
        key: "sandbox.enabled",
        label: "Enabled",
        kind: SettingKind::Bool,
        category: "Sandbox",
    },
    SettingEntry {
        key: "sandbox.mode",
        label: "Mode",
        kind: SettingKind::Text,
        category: "Sandbox",
    },
    SettingEntry {
        key: "memory.max_context_tokens",
        label: "Max context tokens",
        kind: SettingKind::Uint,
        category: "Memory",
    },
    SettingEntry {
        key: "skills.enabled",
        label: "Enabled",
        kind: SettingKind::Bool,
        category: "Skills",
    },
    SettingEntry {
        key: "skills.max_context_tokens",
        label: "Max context tokens",
        kind: SettingKind::Uint,
        category: "Skills",
    },
    SettingEntry {
        key: "conversation.max_iterations",
        label: "Max iterations",
        kind: SettingKind::Uint,
        category: "Conversation",
    },
    SettingEntry {
        key: "conversation.show_thinking",
        label: "Show thinking",
        kind: SettingKind::Bool,
        category: "Conversation",
    },
    SettingEntry {
        key: "conversation.collaboration_mode",
        label: "Collaboration mode",
        kind: SettingKind::Select,
        category: "Conversation",
    },
    SettingEntry {
        key: "security.secret_detection",
        label: "Secret detection",
        kind: SettingKind::Bool,
        category: "Security",
    },
    SettingEntry {
        key: "security.hitl_dangerous_ops",
        label: "Confirm dangerous ops",
        kind: SettingKind::Bool,
        category: "Security",
    },
    SettingEntry {
        key: "budget.monthly_token_limit",
        label: "Monthly token limit",
        kind: SettingKind::Uint,
        category: "Budget",
    },
    SettingEntry {
        key: "budget.warning_threshold",
        label: "Warning threshold",
        kind: SettingKind::Float,
        category: "Budget",
    },
    SettingEntry {
        key: "conversation.tool_output_max_tokens",
        label: "Tool output max tokens",
        kind: SettingKind::Uint,
        category: "Agent",
    },
    SettingEntry {
        key: "conversation.compaction_marker_tokens",
        label: "Compaction marker tokens",
        kind: SettingKind::Uint,
        category: "Agent",
    },
    SettingEntry {
        key: "conversation.max_transcript_chars",
        label: "Max transcript chars",
        kind: SettingKind::Uint,
        category: "Agent",
    },
    SettingEntry {
        key: "gateway.max_body_size",
        label: "Max body size (bytes)",
        kind: SettingKind::Uint,
        category: "Gateway",
    },
    SettingEntry {
        key: "gateway.telegram_poll_timeout_secs",
        label: "Telegram poll timeout (s)",
        kind: SettingKind::Uint,
        category: "Gateway",
    },
    SettingEntry {
        key: "gateway.telegram_circuit_failure_threshold",
        label: "Circuit breaker threshold",
        kind: SettingKind::Uint,
        category: "Gateway",
    },
    SettingEntry {
        key: "gateway.telegram_circuit_suspension_secs",
        label: "Circuit suspension (s)",
        kind: SettingKind::Uint,
        category: "Gateway",
    },
    SettingEntry {
        key: "gateway.telegram_dedup_capacity",
        label: "Dedup capacity",
        kind: SettingKind::Uint,
        category: "Gateway",
    },
    SettingEntry {
        key: "tts.enabled",
        label: "Enabled",
        kind: SettingKind::Bool,
        category: "Voice",
    },
    SettingEntry {
        key: "tts.auto_mode",
        label: "Auto voice reply",
        kind: SettingKind::Bool,
        category: "Voice",
    },
    SettingEntry {
        key: "tts.default_voice",
        label: "Default voice",
        kind: SettingKind::Text,
        category: "Voice",
    },
    SettingEntry {
        key: "tts.default_format",
        label: "Output format",
        kind: SettingKind::Text,
        category: "Voice",
    },
];

impl SettingsPopup {
    pub fn new() -> Self {
        let db = Database::open().ok();
        Self {
            visible: false,
            entries: SETTINGS,
            selected: 0,
            mode: EditMode::Browsing,
            status_message: None,
            db,
            provider_index: 0,
            model_index: 0,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self, config: &Config) {
        self.visible = true;
        self.selected = 0;
        self.mode = EditMode::Browsing;
        self.status_message = None;
        self.sync_select_indices(config);
    }

    /// Sync provider_index and model_index from the current config values.
    fn sync_select_indices(&mut self, config: &Config) {
        let provider_id = config.llm.provider.as_deref().unwrap_or("openrouter");
        self.provider_index = PROVIDERS
            .iter()
            .position(|(id, _, _)| *id == provider_id)
            .unwrap_or(0);

        let models = models_for_provider(provider_id);
        self.model_index = models
            .iter()
            .position(|(id, _)| *id == config.llm.model.as_str())
            .unwrap_or(0);
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
        self.mode = EditMode::Browsing;
        self.status_message = None;
    }

    fn current_value(&self, config: &Config, key: &str) -> String {
        match key {
            "provider" => {
                let (_, display, _) = PROVIDERS.get(self.provider_index).unwrap_or(&PROVIDERS[0]);
                display.to_string()
            }
            "model" => {
                let provider_id = PROVIDERS
                    .get(self.provider_index)
                    .map(|(id, _, _)| *id)
                    .unwrap_or("openrouter");
                let models = models_for_provider(provider_id);
                models
                    .get(self.model_index)
                    .map(|(_, display)| display.to_string())
                    .unwrap_or_else(|| config.llm.model.clone())
            }
            "temperature" => format!("{}", config.llm.temperature),
            "max_tokens" => format!("{}", config.llm.max_tokens),
            "sandbox.enabled" => format!("{}", config.sandbox.enabled),
            "sandbox.mode" => config.sandbox.mode.clone(),
            "memory.max_context_tokens" => format!("{}", config.memory.max_context_tokens),
            "skills.enabled" => format!("{}", config.skills.enabled),
            "skills.max_context_tokens" => format!("{}", config.skills.max_context_tokens),
            "conversation.max_iterations" => format!("{}", config.conversation.max_iterations),
            "conversation.show_thinking" => format!("{}", config.conversation.show_thinking),
            "conversation.tool_output_max_tokens" => {
                format!("{}", config.conversation.tool_output_max_tokens)
            }
            "conversation.compaction_marker_tokens" => {
                format!("{}", config.conversation.compaction_marker_tokens)
            }
            "conversation.max_transcript_chars" => {
                format!("{}", config.conversation.max_transcript_chars)
            }
            "security.secret_detection" => format!("{}", config.security.secret_detection),
            "security.hitl_dangerous_ops" => format!("{}", config.security.hitl_dangerous_ops),
            "budget.monthly_token_limit" => format!("{}", config.budget.monthly_token_limit),
            "budget.warning_threshold" => format!("{}", config.budget.warning_threshold),
            "gateway.max_body_size" => format!("{}", config.gateway.max_body_size),
            "gateway.telegram_poll_timeout_secs" => {
                format!("{}", config.gateway.telegram_poll_timeout_secs)
            }
            "gateway.telegram_circuit_failure_threshold" => {
                format!("{}", config.gateway.telegram_circuit_failure_threshold)
            }
            "gateway.telegram_circuit_suspension_secs" => {
                format!("{}", config.gateway.telegram_circuit_suspension_secs)
            }
            "gateway.telegram_dedup_capacity" => {
                format!("{}", config.gateway.telegram_dedup_capacity)
            }
            "tts.enabled" => format!("{}", config.tts.enabled),
            "tts.auto_mode" => format!("{}", config.tts.auto_mode),
            "tts.default_voice" => config.tts.default_voice.clone(),
            "tts.default_format" => config.tts.default_format.clone(),
            "conversation.collaboration_mode" => {
                format!("{}", config.conversation.collaboration_mode)
            }
            _ => "?".to_string(),
        }
    }

    pub fn handle_key(
        &mut self,
        key: crossterm::event::KeyEvent,
        config: &mut Config,
    ) -> anyhow::Result<Option<AppAction>> {
        use crossterm::event::KeyCode;

        match &mut self.mode {
            EditMode::Browsing => match key.code {
                KeyCode::Esc => {
                    self.dismiss();
                    Ok(None)
                }
                KeyCode::Up => {
                    if self.selected == 0 {
                        self.selected = self.entries.len() - 1;
                    } else {
                        self.selected -= 1;
                    }
                    self.status_message = None;
                    Ok(None)
                }
                KeyCode::Down => {
                    self.selected = (self.selected + 1) % self.entries.len();
                    self.status_message = None;
                    Ok(None)
                }
                KeyCode::Char(' ') => {
                    let entry = &self.entries[self.selected];
                    if entry.kind == SettingKind::Bool {
                        return self.toggle_bool(config);
                    }
                    Ok(None)
                }
                KeyCode::Left => {
                    let entry = &self.entries[self.selected];
                    match entry.kind {
                        SettingKind::Select => self.cycle_select(config, false),
                        SettingKind::Float => self.step_float(config, false),
                        _ => Ok(None),
                    }
                }
                KeyCode::Right => {
                    let entry = &self.entries[self.selected];
                    match entry.kind {
                        SettingKind::Select => self.cycle_select(config, true),
                        SettingKind::Float => self.step_float(config, true),
                        _ => Ok(None),
                    }
                }
                KeyCode::Enter => {
                    let entry = &self.entries[self.selected];
                    match entry.kind {
                        SettingKind::Bool => return self.toggle_bool(config),
                        SettingKind::Select => return self.cycle_select(config, true),
                        _ => {
                            let current = self.current_value(config, entry.key);
                            self.mode = EditMode::Editing { buffer: current };
                            self.status_message = None;
                        }
                    }
                    Ok(None)
                }
                _ => Ok(None),
            },
            EditMode::Editing { buffer } => match key.code {
                KeyCode::Esc => {
                    self.mode = EditMode::Browsing;
                    self.status_message = None;
                    Ok(None)
                }
                KeyCode::Enter => {
                    let value = buffer.clone();
                    let entry = &self.entries[self.selected];
                    let key_str = entry.key;
                    match config.apply_setting(key_str, &value) {
                        Ok(confirmation) => {
                            if let Err(e) = self.save_setting(key_str, &value) {
                                self.status_message = Some((format!("Save failed: {e}"), false));
                                self.mode = EditMode::Browsing;
                                return Ok(None);
                            }
                            self.status_message = Some((format!("Updated: {confirmation}"), true));
                            self.mode = EditMode::Browsing;
                            Ok(Some(AppAction::UpdateSetting {
                                key: key_str.to_string(),
                                value,
                            }))
                        }
                        Err(e) => {
                            self.status_message = Some((format!("Error: {e}"), false));
                            self.mode = EditMode::Browsing;
                            Ok(None)
                        }
                    }
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    Ok(None)
                }
                KeyCode::Char(c) => {
                    buffer.push(c);
                    Ok(None)
                }
                _ => Ok(None),
            },
        }
    }

    fn cycle_select(
        &mut self,
        config: &mut Config,
        forward: bool,
    ) -> anyhow::Result<Option<AppAction>> {
        let entry = &self.entries[self.selected];
        let mut actions: Vec<AppAction> = Vec::new();

        match entry.key {
            "provider" => {
                let count = PROVIDERS.len();
                self.provider_index = if forward {
                    (self.provider_index + 1) % count
                } else {
                    (self.provider_index + count - 1) % count
                };
                let (id, _, _) = PROVIDERS[self.provider_index];
                match config.apply_setting("provider", id) {
                    Ok(confirmation) => {
                        let _ = self.save_setting("provider", id);
                        self.status_message = Some((format!("Updated: {confirmation}"), true));
                        actions.push(AppAction::UpdateSetting {
                            key: "provider".to_string(),
                            value: id.to_string(),
                        });
                    }
                    Err(e) => {
                        self.status_message = Some((format!("Error: {e}"), false));
                        return Ok(None);
                    }
                }
                // Reset model to first option for new provider
                self.model_index = 0;
                let models = models_for_provider(id);
                if let Some((model_id, _)) = models.first() {
                    if config.apply_setting("model", model_id).is_ok() {
                        let _ = self.save_setting("model", model_id);
                        actions.push(AppAction::UpdateSetting {
                            key: "model".to_string(),
                            value: model_id.to_string(),
                        });
                    }
                }
            }
            "model" => {
                let provider_id = PROVIDERS
                    .get(self.provider_index)
                    .map(|(id, _, _)| *id)
                    .unwrap_or("openrouter");
                let models = models_for_provider(provider_id);
                let count = models.len();
                self.model_index = if forward {
                    (self.model_index + 1) % count
                } else {
                    (self.model_index + count - 1) % count
                };
                let (model_id, _) = models[self.model_index];
                match config.apply_setting("model", model_id) {
                    Ok(confirmation) => {
                        let _ = self.save_setting("model", model_id);
                        self.status_message = Some((format!("Updated: {confirmation}"), true));
                        actions.push(AppAction::UpdateSetting {
                            key: "model".to_string(),
                            value: model_id.to_string(),
                        });
                    }
                    Err(e) => {
                        self.status_message = Some((format!("Error: {e}"), false));
                        return Ok(None);
                    }
                }
            }
            "conversation.collaboration_mode" => {
                const MODES: &[&str] = &["default", "execute", "plan"];
                let current = format!("{}", config.conversation.collaboration_mode);
                let idx = MODES.iter().position(|&m| m == current).unwrap_or(0);
                let next_idx = if forward {
                    (idx + 1) % MODES.len()
                } else {
                    (idx + MODES.len() - 1) % MODES.len()
                };
                let new_mode = MODES[next_idx];
                match config.apply_setting("conversation.collaboration_mode", new_mode) {
                    Ok(confirmation) => {
                        let _ = self.save_setting("conversation.collaboration_mode", new_mode);
                        self.status_message = Some((format!("Updated: {confirmation}"), true));
                        actions.push(AppAction::UpdateSetting {
                            key: "conversation.collaboration_mode".to_string(),
                            value: new_mode.to_string(),
                        });
                    }
                    Err(e) => {
                        self.status_message = Some((format!("Error: {e}"), false));
                        return Ok(None);
                    }
                }
            }
            _ => return Ok(None),
        }

        // Return the first action (provider change is the primary one)
        Ok(actions.into_iter().next())
    }

    fn step_float(
        &mut self,
        config: &mut Config,
        increase: bool,
    ) -> anyhow::Result<Option<AppAction>> {
        let entry = &self.entries[self.selected];
        let current = self.current_value(config, entry.key);
        let val: f64 = current.parse().unwrap_or(0.0);

        let (step, min, max) = match entry.key {
            "budget.warning_threshold" => (0.01, 0.0, 1.0),
            _ => (0.1, 0.0, 2.0), // temperature
        };

        let new_val = if increase {
            (val + step).min(max)
        } else {
            (val - step).max(min)
        };

        // Round to avoid floating point drift
        let decimals = if step < 0.1 { 2 } else { 1 };
        let formatted = format!("{new_val:.decimals$}");

        match config.apply_setting(entry.key, &formatted) {
            Ok(confirmation) => {
                let _ = self.save_setting(entry.key, &formatted);
                self.status_message = Some((format!("Updated: {confirmation}"), true));
                Ok(Some(AppAction::UpdateSetting {
                    key: entry.key.to_string(),
                    value: formatted,
                }))
            }
            Err(e) => {
                self.status_message = Some((format!("Error: {e}"), false));
                Ok(None)
            }
        }
    }

    fn toggle_bool(&mut self, config: &mut Config) -> anyhow::Result<Option<AppAction>> {
        let entry = &self.entries[self.selected];
        let current = self.current_value(config, entry.key);
        let new_val = if current == "true" { "false" } else { "true" };
        match config.apply_setting(entry.key, new_val) {
            Ok(confirmation) => {
                if let Err(e) = self.save_setting(entry.key, new_val) {
                    self.status_message = Some((format!("Save failed: {e}"), false));
                    return Ok(None);
                }
                self.status_message = Some((format!("Updated: {confirmation}"), true));
                Ok(Some(AppAction::UpdateSetting {
                    key: entry.key.to_string(),
                    value: new_val.to_string(),
                }))
            }
            Err(e) => {
                self.status_message = Some((format!("Error: {e}"), false));
                Ok(None)
            }
        }
    }

    /// Save a setting to DB if available, otherwise fall back to config.toml.
    fn save_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        if let Some(ref db) = self.db {
            db.set_setting(key, value)?;
        }
        Ok(())
    }

    /// Get the source of a setting value.
    fn setting_source(&self, key: &str) -> SettingSource {
        if let Some(ref db) = self.db {
            if let Ok(Some(_)) = db.get_setting(key) {
                return SettingSource::Database;
            }
        }
        SettingSource::Default
    }

    pub fn render(&self, frame: &mut Frame, config: &Config) {
        if !self.visible {
            return;
        }

        let area = frame.area();
        let popup_width = (area.width * 60 / 100)
            .max(40)
            .min(area.width.saturating_sub(4));
        let popup_height = (area.height * 80 / 100)
            .max(10)
            .min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(popup_width)) / 2;
        let y = (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::dim())
            .title(" Settings ");

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height < 3 || inner.width < 10 {
            return;
        }

        let content_height = (inner.height as usize).saturating_sub(2); // reserve footer + status
        let mut lines: Vec<Line<'static>> = Vec::new();

        let mut last_category: Option<&str> = None;
        let mut row_indices: Vec<usize> = Vec::new(); // maps entry index to line index

        for (i, entry) in self.entries.iter().enumerate() {
            if last_category != Some(entry.category) {
                if last_category.is_some() {
                    lines.push(Line::default());
                }
                lines.push(Line::from(Span::styled(
                    format!(" {}", entry.category),
                    theme::bold(),
                )));
                last_category = Some(entry.category);
            }

            row_indices.push(lines.len());
            let value = self.current_value(config, entry.key);
            let source = self.setting_source(entry.key);
            let source_tag = match source {
                SettingSource::Database => " [db]",
                SettingSource::ConfigToml => " [toml]",
                SettingSource::Default => "",
            };
            let is_selected = i == self.selected;

            let display_value = if is_selected {
                if let EditMode::Editing { ref buffer } = self.mode {
                    format!("{buffer}_")
                } else if entry.kind == SettingKind::Bool {
                    let check = if value == "true" { "x" } else { " " };
                    format!("[{check}]{source_tag}")
                } else if entry.kind == SettingKind::Select {
                    format!("< {value} >{source_tag}")
                } else {
                    format!("{value}{source_tag}")
                }
            } else if entry.kind == SettingKind::Bool {
                let check = if value == "true" { "x" } else { " " };
                format!("[{check}]{source_tag}")
            } else {
                format!("{value}{source_tag}")
            };

            let label_width = inner.width.saturating_sub(4) as usize;
            let label = format!("  {}", entry.label);
            let val_len = display_value.len();
            let padding = label_width.saturating_sub(label.len() + val_len);

            let row_text = format!("{label}{:>pad$}{display_value} ", "", pad = padding);

            let style = if is_selected {
                theme::popup_selected()
            } else {
                ratatui::style::Style::default()
            };

            lines.push(Line::from(Span::styled(row_text, style)));
        }

        // Scroll to keep selected visible
        let selected_line = row_indices.get(self.selected).copied().unwrap_or(0);
        let scroll_offset = if selected_line >= content_height {
            selected_line - content_height + 1
        } else {
            0
        };

        let visible_lines: Vec<Line<'static>> = lines
            .into_iter()
            .skip(scroll_offset)
            .take(content_height)
            .collect();

        let content_area = Rect::new(inner.x, inner.y, inner.width, content_height as u16);
        frame.render_widget(Paragraph::new(visible_lines), content_area);

        // Status line
        if let Some((ref msg, is_success)) = self.status_message {
            let style = if is_success {
                theme::success_style()
            } else {
                theme::error_style()
            };
            let status_y = inner.y + inner.height - 2;
            let status_area = Rect::new(inner.x, status_y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(format!(" {msg}"), style))),
                status_area,
            );
        }

        // Footer hint (context-sensitive)
        let hint = match self.mode {
            EditMode::Browsing => {
                let entry = &self.entries[self.selected];
                match entry.kind {
                    SettingKind::Select => " \u{25C0}\u{25B6}: cycle  Enter: next  Esc: close",
                    SettingKind::Bool => " Space: toggle  Esc: close",
                    SettingKind::Float => " Enter: edit  \u{25C0}\u{25B6}: \u{00B1}0.1  Esc: close",
                    _ => " Enter: edit  Esc: close",
                }
            }
            EditMode::Editing { .. } => " Enter: apply  Esc: cancel",
        };
        let footer_y = inner.y + inner.height - 1;
        let footer_area = Rect::new(inner.x, footer_y, inner.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(hint.to_string(), theme::dim()))),
            footer_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_and_dismiss() {
        let mut popup = SettingsPopup::new();
        let cfg = Config::default();
        assert!(!popup.is_visible());
        popup.show(&cfg);
        assert!(popup.is_visible());
        popup.dismiss();
        assert!(!popup.is_visible());
    }

    #[test]
    fn navigation_wraps() {
        let mut popup = SettingsPopup::new();
        let cfg = Config::default();
        popup.show(&cfg);
        let count = popup.entries.len();

        assert_eq!(popup.selected, 0);

        // Up from 0 wraps to last
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let mut cfg = Config::default();

        popup.handle_key(up, &mut cfg).unwrap();
        assert_eq!(popup.selected, count - 1);

        popup.handle_key(down, &mut cfg).unwrap();
        assert_eq!(popup.selected, 0);
    }

    #[test]
    fn bool_toggle() {
        let mut popup = SettingsPopup::new();
        let cfg = Config::default();
        popup.show(&cfg);

        // Find sandbox.enabled (index 4)
        popup.selected = 4;
        assert_eq!(popup.entries[4].key, "sandbox.enabled");

        let mut cfg = Config::default();
        assert!(cfg.sandbox.enabled);

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let result = popup.handle_key(space, &mut cfg).unwrap();
        assert!(!cfg.sandbox.enabled);
        assert!(result.is_some());
    }

    #[test]
    fn valid_edit_applies() {
        let mut popup = SettingsPopup::new();
        let cfg = Config::default();
        popup.show(&cfg);

        // Select temperature (index 2)
        popup.selected = 2;
        assert_eq!(popup.entries[2].key, "temperature");

        let mut cfg = Config::default();

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        // Start editing
        popup.handle_key(enter, &mut cfg).unwrap();
        assert!(matches!(popup.mode, EditMode::Editing { .. }));

        // Clear buffer and type "1.5"
        if let EditMode::Editing { ref mut buffer } = popup.mode {
            buffer.clear();
            buffer.push_str("1.5");
        }

        // Apply
        let result = popup.handle_key(enter, &mut cfg).unwrap();
        assert!((cfg.llm.temperature - 1.5).abs() < f32::EPSILON);
        assert!(result.is_some());
        assert!(matches!(popup.mode, EditMode::Browsing));
    }

    #[test]
    fn invalid_edit_shows_error() {
        let mut popup = SettingsPopup::new();
        let cfg = Config::default();
        popup.show(&cfg);

        // Select temperature (index 2)
        popup.selected = 2;
        let mut cfg = Config::default();
        let original_temp = cfg.llm.temperature;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        // Start editing
        popup.handle_key(enter, &mut cfg).unwrap();

        // Set invalid value
        if let EditMode::Editing { ref mut buffer } = popup.mode {
            buffer.clear();
            buffer.push_str("5.0"); // out of range
        }

        // Apply — should fail
        let result = popup.handle_key(enter, &mut cfg).unwrap();
        assert!((cfg.llm.temperature - original_temp).abs() < f32::EPSILON);
        assert!(result.is_none());
        assert!(popup.status_message.as_ref().map_or(false, |(_, ok)| !ok));
    }

    #[test]
    fn all_settings_covered() {
        let popup = SettingsPopup::new();
        assert_eq!(popup.entries.len(), 28);

        let cfg = Config::default();
        for entry in popup.entries {
            let val = popup.current_value(&cfg, entry.key);
            assert_ne!(val, "?", "missing current_value for {}", entry.key);
        }
    }

    #[test]
    fn esc_during_edit_cancels() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        popup.show(&cfg);

        // Select temperature (index 2) — model is now Select, not editable via Enter
        popup.selected = 2;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);

        popup.handle_key(enter, &mut cfg).unwrap();
        assert!(matches!(popup.mode, EditMode::Editing { .. }));

        popup.handle_key(esc, &mut cfg).unwrap();
        assert!(matches!(popup.mode, EditMode::Browsing));
        assert!(popup.is_visible());
    }

    #[test]
    fn provider_cycle_forward() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        popup.show(&cfg);

        // Provider is entry 0
        popup.selected = 0;
        assert_eq!(popup.entries[0].key, "provider");
        let initial_provider_index = popup.provider_index;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        let result = popup.handle_key(right, &mut cfg).unwrap();
        assert!(result.is_some());
        assert_eq!(
            popup.provider_index,
            (initial_provider_index + 1) % PROVIDERS.len()
        );
        // Model index should reset to 0
        assert_eq!(popup.model_index, 0);
    }

    #[test]
    fn provider_cycle_backward() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        popup.show(&cfg);

        popup.selected = 0;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        let result = popup.handle_key(left, &mut cfg).unwrap();
        assert!(result.is_some());
        // From 0, should wrap to last
        assert_eq!(popup.provider_index, PROVIDERS.len() - 1);
    }

    #[test]
    fn model_cycle_forward() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        popup.show(&cfg);

        // Model is entry 1
        popup.selected = 1;
        assert_eq!(popup.entries[1].key, "model");

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        let result = popup.handle_key(right, &mut cfg).unwrap();
        assert!(result.is_some());
        assert_eq!(popup.model_index, 1);
    }

    #[test]
    fn float_step_right_increases() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        popup.show(&cfg);

        // Temperature is entry 2
        popup.selected = 2;
        let original = cfg.llm.temperature;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        let result = popup.handle_key(right, &mut cfg).unwrap();
        assert!(result.is_some());
        assert!((cfg.llm.temperature - (original + 0.1)).abs() < 0.01);
    }

    #[test]
    fn float_step_left_decreases() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        popup.show(&cfg);

        popup.selected = 2;
        let original = cfg.llm.temperature;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        let result = popup.handle_key(left, &mut cfg).unwrap();
        assert!(result.is_some());
        assert!((cfg.llm.temperature - (original - 0.1)).abs() < 0.01);
    }

    #[test]
    fn select_enter_cycles_forward() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        popup.show(&cfg);

        // Provider is entry 0 — Enter should cycle forward, not open edit mode
        popup.selected = 0;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        popup.handle_key(enter, &mut cfg).unwrap();
        // Should NOT enter editing mode
        assert!(matches!(popup.mode, EditMode::Browsing));
        // Should have cycled
        assert_eq!(popup.provider_index, 1);
    }

    #[test]
    fn sync_indices_from_config() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        cfg.llm.provider = Some("anthropic".to_string());
        cfg.llm.model = "claude-haiku-4".to_string();
        popup.show(&cfg);

        assert_eq!(popup.provider_index, 2); // anthropic is index 2
        assert_eq!(popup.model_index, 1); // claude-haiku-4 is index 1 in ANTHROPIC_MODELS
    }

    #[test]
    fn float_clamps_at_max() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        cfg.llm.temperature = 2.0;
        popup.show(&cfg);

        popup.selected = 2; // temperature

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        popup.handle_key(right, &mut cfg).unwrap();
        // Should stay at 2.0
        assert!((cfg.llm.temperature - 2.0).abs() < 0.01);
    }

    #[test]
    fn float_clamps_at_min() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        cfg.llm.temperature = 0.0;
        popup.show(&cfg);

        popup.selected = 2; // temperature

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        popup.handle_key(left, &mut cfg).unwrap();
        assert!((cfg.llm.temperature - 0.0).abs() < 0.01);
    }

    #[test]
    fn provider_change_updates_model_config() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        popup.show(&cfg);

        // Start at openrouter (index 0), cycle to openai (index 1)
        popup.selected = 0;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        popup.handle_key(right, &mut cfg).unwrap();

        // Config should now have openai provider and first openai model
        assert_eq!(cfg.llm.provider.as_deref(), Some("openai"));
        assert_eq!(cfg.llm.model, "gpt-4.1");
    }

    #[test]
    fn model_cycle_wraps() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        // Set to anthropic with last model
        cfg.llm.provider = Some("anthropic".to_string());
        cfg.llm.model = "claude-opus-4".to_string();
        popup.show(&cfg);

        popup.selected = 1; // model
        assert_eq!(popup.model_index, 2); // last model in ANTHROPIC_MODELS

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        popup.handle_key(right, &mut cfg).unwrap();
        // Should wrap to 0
        assert_eq!(popup.model_index, 0);
        assert_eq!(cfg.llm.model, "claude-sonnet-4");
    }

    #[test]
    fn left_right_noop_on_text_fields() {
        let mut popup = SettingsPopup::new();
        let mut cfg = Config::default();
        popup.show(&cfg);

        // sandbox.mode is a Text field (index 5)
        popup.selected = 5;
        assert_eq!(popup.entries[5].key, "sandbox.mode");

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        let result = popup.handle_key(right, &mut cfg).unwrap();
        assert!(result.is_none());
    }
}
