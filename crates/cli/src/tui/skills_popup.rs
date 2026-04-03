use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use borg_core::config::Config;
use borg_core::skills::load_all_skills;

use super::theme;

struct SkillItem {
    name: String,
    description: String,
    source: String,
    available: bool,
    original_enabled: bool,
    is_enabled: bool,
}

pub struct SkillsPopup {
    visible: bool,
    items: Vec<SkillItem>,
    cursor: usize,
    status_message: Option<(String, bool)>,
}

pub enum SkillAction {
    SetEnabled { name: String, enabled: bool },
}

impl SkillsPopup {
    pub fn new() -> Self {
        Self {
            visible: false,
            items: Vec::new(),
            cursor: 0,
            status_message: None,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self, config: &Config) {
        self.visible = true;
        self.cursor = 0;
        self.status_message = None;

        let resolved_creds = config.resolve_credentials();
        match load_all_skills(&resolved_creds, &config.skills) {
            Ok(skills) => {
                self.items = skills
                    .into_iter()
                    .map(|s| SkillItem {
                        name: s.manifest.name.clone(),
                        description: s.manifest.description.clone(),
                        source: s.source_label().to_string(),
                        available: s.available,
                        original_enabled: !s.disabled,
                        is_enabled: !s.disabled,
                    })
                    .collect();
            }
            Err(_) => {
                self.items = Vec::new();
                self.status_message = Some(("Failed to load skills.".to_string(), false));
            }
        }
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
        self.status_message = None;
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<Vec<SkillAction>> {
        use crossterm::event::KeyCode;

        if !self.visible {
            return None;
        }

        match key.code {
            KeyCode::Esc => {
                self.dismiss();
                None
            }
            KeyCode::Up => {
                if !self.items.is_empty() {
                    if self.cursor == 0 {
                        self.cursor = self.items.len() - 1;
                    } else {
                        self.cursor -= 1;
                    }
                }
                self.status_message = None;
                None
            }
            KeyCode::Down => {
                if !self.items.is_empty() {
                    self.cursor = (self.cursor + 1) % self.items.len();
                }
                self.status_message = None;
                None
            }
            KeyCode::Char(' ') => {
                if let Some(item) = self.items.get_mut(self.cursor) {
                    item.is_enabled = !item.is_enabled;
                    self.status_message = None;
                }
                None
            }
            KeyCode::Enter => {
                let actions: Vec<SkillAction> = self
                    .items
                    .iter()
                    .filter(|item| item.is_enabled != item.original_enabled)
                    .map(|item| SkillAction::SetEnabled {
                        name: item.name.clone(),
                        enabled: item.is_enabled,
                    })
                    .collect();

                if actions.is_empty() {
                    self.status_message = Some(("No changes to apply.".to_string(), false));
                    return None;
                }

                self.dismiss();
                Some(actions)
            }
            _ => None,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        if !self.visible {
            return;
        }

        let area = frame.area();
        let popup_width = (area.width * 60 / 100)
            .max(44)
            .min(area.width.saturating_sub(4));
        let popup_height = (area.height * 80 / 100)
            .max(12)
            .min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(popup_width)) / 2;
        let y = (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::dim())
            .title(" Skills ");

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height < 5 || inner.width < 12 {
            return;
        }

        let content_height = (inner.height as usize).saturating_sub(2);
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut row_indices: Vec<usize> = Vec::new();

        if self.items.is_empty() {
            lines.push(Line::from(Span::styled(
                " No skills found.".to_string(),
                theme::dim(),
            )));
        }

        for (i, item) in self.items.iter().enumerate() {
            row_indices.push(lines.len());

            let check = if item.is_enabled { "x" } else { " " };

            // Truncate description to fit within popup width (char-safe)
            let max_desc_len = (inner.width as usize).saturating_sub(item.name.len() + 18);
            let desc = if item.description.chars().count() > max_desc_len {
                let truncated: String = item
                    .description
                    .char_indices()
                    .take_while(|(i, _)| *i < max_desc_len.saturating_sub(3))
                    .map(|(_, c)| c)
                    .collect();
                format!("{truncated}...")
            } else {
                item.description.clone()
            };

            let label = format!(
                "  [{check}] {} ({}) \u{2014} {desc}",
                item.name, item.source,
            );

            let is_cursor = i == self.cursor;
            let style = if is_cursor {
                theme::popup_selected()
            } else if !item.available && item.is_enabled {
                theme::dim()
            } else {
                ratatui::style::Style::default()
            };

            lines.push(Line::from(Span::styled(label, style)));
        }

        // Scroll to keep cursor visible
        let selected_line = row_indices.get(self.cursor).copied().unwrap_or(0);
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

        // Footer hint
        let hint = " Space: toggle  Enter: apply  Esc: close";
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
    fn new_popup_not_visible() {
        let popup = SkillsPopup::new();
        assert!(!popup.is_visible());
    }

    #[test]
    fn show_populates_items() {
        let mut popup = SkillsPopup::new();
        let config = Config::default();
        popup.show(&config);
        assert!(popup.is_visible());
        // Built-in skills should be present
        assert!(!popup.items.is_empty());
    }

    #[test]
    fn dismiss_hides_popup() {
        let mut popup = SkillsPopup::new();
        let config = Config::default();
        popup.show(&config);
        assert!(popup.is_visible());
        popup.dismiss();
        assert!(!popup.is_visible());
    }

    #[test]
    fn navigation_wraps() {
        let mut popup = SkillsPopup::new();
        let config = Config::default();
        popup.show(&config);

        let count = popup.items.len();
        assert!(count > 0);
        assert_eq!(popup.cursor, 0);

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        popup.handle_key(up);
        assert_eq!(popup.cursor, count - 1);

        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        popup.handle_key(down);
        assert_eq!(popup.cursor, 0);
    }

    #[test]
    fn toggle_and_apply() {
        let mut popup = SkillsPopup::new();
        let config = Config::default();
        popup.show(&config);

        assert!(!popup.items.is_empty());
        let original = popup.items[0].is_enabled;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        popup.handle_key(space);
        assert_eq!(popup.items[0].is_enabled, !original);

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let result = popup.handle_key(enter);
        assert!(result.is_some());
        let actions = result.unwrap();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            SkillAction::SetEnabled { enabled, .. } if *enabled == !original
        ));
    }

    #[test]
    fn enter_no_changes_shows_status() {
        let mut popup = SkillsPopup::new();
        let config = Config::default();
        popup.show(&config);

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let result = popup.handle_key(enter);
        assert!(result.is_none());
        assert!(popup.status_message.is_some());
    }

    #[test]
    fn esc_closes_popup() {
        let mut popup = SkillsPopup::new();
        let config = Config::default();
        popup.show(&config);
        assert!(popup.is_visible());

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        popup.handle_key(esc);
        assert!(!popup.is_visible());
    }

    #[test]
    fn esc_discards_changes() {
        let mut popup = SkillsPopup::new();
        let config = Config::default();
        popup.show(&config);

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // Toggle a skill
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        popup.handle_key(space);

        // Esc should discard, not return actions
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let result = popup.handle_key(esc);
        assert!(result.is_none());
        assert!(!popup.is_visible());
    }
}
