use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use tamagotchi_customizations::catalog::{CustomizationDef, CATALOG};
use tamagotchi_customizations::Category;

use super::theme;

/// State for a single item in the customize list.
struct CustomizeItem {
    def: &'static CustomizationDef,
    is_installed: bool,
    is_selected: bool,
}

/// Phase of the customize popup.
#[derive(Clone, PartialEq)]
enum CustomizePhase {
    Browsing,
}

pub struct CustomizePopup {
    visible: bool,
    items: Vec<CustomizeItem>,
    cursor: usize,
    phase: CustomizePhase,
    status_message: Option<(String, bool)>,
}

/// Actions that the customize popup can request from the event loop.
pub enum CustomizeAction {
    Install { id: String },
    Uninstall { id: String },
}

impl CustomizePopup {
    pub fn new() -> Self {
        Self {
            visible: false,
            items: Vec::new(),
            cursor: 0,
            phase: CustomizePhase::Browsing,
            status_message: None,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show the popup, scanning filesystem for installed state.
    pub fn show(&mut self, data_dir: &std::path::Path) {
        self.visible = true;
        self.cursor = 0;
        self.phase = CustomizePhase::Browsing;
        self.status_message = None;

        self.items = CATALOG
            .iter()
            .map(|def| {
                let installed = tamagotchi_customizations::installer::is_installed(def, data_dir);
                CustomizeItem {
                    def,
                    is_installed: installed,
                    is_selected: installed,
                }
            })
            .collect();
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
        self.phase = CustomizePhase::Browsing;
        self.status_message = None;
    }

    /// Handle a key event. Returns actions to execute if Enter is pressed.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<Vec<CustomizeAction>> {
        use crossterm::event::KeyCode;

        if !self.visible {
            return None;
        }

        match &self.phase {
            CustomizePhase::Browsing => match key.code {
                KeyCode::Esc => {
                    self.dismiss();
                    None
                }
                KeyCode::Up => {
                    if self.items.is_empty() {
                        return None;
                    }
                    if self.cursor == 0 {
                        self.cursor = self.items.len() - 1;
                    } else {
                        self.cursor -= 1;
                    }
                    self.status_message = None;
                    None
                }
                KeyCode::Down => {
                    if self.items.is_empty() {
                        return None;
                    }
                    self.cursor = (self.cursor + 1) % self.items.len();
                    self.status_message = None;
                    None
                }
                KeyCode::Char(' ') | KeyCode::Enter => {
                    if let Some(item) = self.items.get_mut(self.cursor) {
                        if !item.def.platform.is_available() {
                            self.status_message = Some((
                                format!(
                                    "{} requires {}",
                                    item.def.name,
                                    item.def.platform.label().unwrap_or("a different platform")
                                ),
                                false,
                            ));
                            return None;
                        }
                        item.is_selected = !item.is_selected;
                        self.status_message = None;
                    }
                    None
                }
                KeyCode::Tab => {
                    let actions = self.compute_actions();
                    if actions.is_empty() {
                        self.status_message = Some(("No changes to apply.".to_string(), false));
                        return None;
                    }
                    self.dismiss();
                    Some(actions)
                }
                _ => None,
            },
        }
    }

    fn compute_actions(&self) -> Vec<CustomizeAction> {
        let mut actions = Vec::new();
        for item in &self.items {
            if item.is_selected && !item.is_installed {
                actions.push(CustomizeAction::Install {
                    id: item.def.id.to_string(),
                });
            } else if !item.is_selected && item.is_installed {
                actions.push(CustomizeAction::Uninstall {
                    id: item.def.id.to_string(),
                });
            }
        }
        actions
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
        let x = 1; // left-aligned with a small margin
        let y = (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::dim())
            .title(" Customize ");

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height < 5 || inner.width < 12 {
            return;
        }

        let content_height = (inner.height as usize).saturating_sub(2);
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut row_indices: Vec<usize> = Vec::new();

        let mut last_category: Option<Category> = None;
        for (i, item) in self.items.iter().enumerate() {
            if last_category != Some(item.def.category) {
                if last_category.is_some() {
                    lines.push(Line::default());
                }
                lines.push(Line::from(Span::styled(
                    format!(" {}", item.def.category),
                    theme::bold(),
                )));
                last_category = Some(item.def.category);
            }

            row_indices.push(lines.len());

            let check = if item.is_selected { "x" } else { " " };
            let status = if item.is_installed && item.is_selected {
                " \u{2713} installed"
            } else if item.is_installed && !item.is_selected {
                " (remove)"
            } else if !item.is_installed && item.is_selected {
                " (install)"
            } else {
                ""
            };

            let platform_note = if let Some(label) = item.def.platform.label() {
                if !item.def.platform.is_available() {
                    format!("  ({label})")
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let python_note = if item.def.required_bins.contains(&"python3")
                && which::which("python3").is_err()
            {
                "  (needs python3)".to_string()
            } else {
                String::new()
            };

            let label = format!(
                "  [{check}] {}{status}{platform_note}{python_note}",
                item.def.name,
            );

            let is_selected = i == self.cursor;
            let style = if is_selected {
                theme::popup_selected()
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
        let hint = " Enter: toggle  Tab: apply  Esc: close";
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
        let popup = CustomizePopup::new();
        assert!(!popup.is_visible());
    }

    #[test]
    fn show_and_dismiss() {
        let mut popup = CustomizePopup::new();
        let tmp = std::env::temp_dir().join("tamagotchi-customize-test");
        popup.show(&tmp);
        assert!(popup.is_visible());
        assert!(!popup.items.is_empty());
        popup.dismiss();
        assert!(!popup.is_visible());
    }

    #[test]
    fn navigation_wraps() {
        let mut popup = CustomizePopup::new();
        let tmp = std::env::temp_dir().join("tamagotchi-customize-test-nav");
        popup.show(&tmp);

        let count = popup.items.len();
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
    fn toggle_and_compute_actions() {
        let mut popup = CustomizePopup::new();
        let tmp = std::env::temp_dir().join("tamagotchi-customize-test-toggle");
        popup.show(&tmp);

        // All items start unselected (since nothing is installed in temp dir)
        assert!(!popup.items[0].is_selected);

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // Enter toggles selection
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        popup.handle_key(enter);
        assert!(popup.items[0].is_selected);

        // Space also toggles selection
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        popup.handle_key(space);
        assert!(!popup.items[0].is_selected);

        // Toggle back on for action check
        popup.handle_key(enter);
        assert!(popup.items[0].is_selected);

        let actions = popup.compute_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], CustomizeAction::Install { .. }));
    }
}
