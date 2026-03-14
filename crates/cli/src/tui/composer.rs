use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Block;
use ratatui::Frame;
use tui_textarea::TextArea;

use super::theme;

pub struct Composer<'a> {
    textarea: TextArea<'a>,
    disabled: bool,
}

impl<'a> Composer<'a> {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text("Type a message...");
        textarea.set_cursor_line_style(Style::default());
        textarea.set_block(
            Block::bordered()
                .title(format!("{} ", theme::INPUT_PROMPT))
                .border_style(Style::default().fg(theme::BORDER)),
        );
        Self {
            textarea,
            disabled: false,
        }
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.disabled = disabled;
        let border_color = if disabled {
            theme::DIM_WHITE
        } else {
            theme::BORDER
        };
        self.textarea.set_block(
            Block::bordered()
                .title(format!("{} ", theme::INPUT_PROMPT))
                .border_style(Style::default().fg(border_color)),
        );
        if disabled {
            self.textarea.set_cursor_style(Style::default());
        } else {
            self.textarea
                .set_cursor_style(Style::default().bg(Color::White).fg(Color::Black));
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        if self.disabled {
            return None;
        }

        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.code, key.modifiers) {
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let text: String = self.textarea.lines().join("\n");
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    return None;
                }
                // Clear the textarea
                self.textarea.select_all();
                self.textarea.cut();
                Some(trimmed)
            }
            (KeyCode::Enter, m)
                if m.contains(KeyModifiers::SHIFT) || m.contains(KeyModifiers::ALT) =>
            {
                self.textarea.insert_newline();
                None
            }
            (KeyCode::Esc, _) => {
                self.textarea.select_all();
                self.textarea.cut();
                None
            }
            _ => {
                self.textarea.input(key);
                None
            }
        }
    }

    pub fn height(&self) -> u16 {
        let content_lines = self.textarea.lines().len() as u16;
        // 2 for border, min 1 content line
        (content_lines.max(1) + 2).min(10)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(&self.textarea, area);
    }
}
