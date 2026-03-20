use std::path::PathBuf;

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui::Frame;
use tui_textarea::TextArea;

use super::theme;

pub struct FileRef {
    pub display: String,
    pub path: PathBuf,
}

struct InputHistory {
    entries: Vec<String>,
    cursor: Option<usize>,
    draft: String,
}

impl InputHistory {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            cursor: None,
            draft: String::new(),
        }
    }

    fn push(&mut self, text: &str) {
        if !text.is_empty() {
            self.entries.push(text.to_string());
        }
        self.cursor = None;
        self.draft.clear();
    }

    fn up(&mut self, current_text: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        match self.cursor {
            None => {
                self.draft = current_text.to_string();
                self.cursor = Some(self.entries.len() - 1);
            }
            Some(0) => {
                // Already at oldest, clamp
            }
            Some(pos) => {
                self.cursor = Some(pos - 1);
            }
        }
        self.cursor.map(|i| self.entries[i].as_str())
    }

    /// Returns Some(text) to set, or None meaning restore the draft
    fn down(&mut self) -> Option<Option<&str>> {
        match self.cursor {
            None => None, // not browsing
            Some(pos) => {
                if pos + 1 < self.entries.len() {
                    self.cursor = Some(pos + 1);
                    Some(Some(self.entries[pos + 1].as_str()))
                } else {
                    // Past newest → restore draft
                    self.cursor = None;
                    Some(None)
                }
            }
        }
    }

    fn reset(&mut self) {
        self.cursor = None;
        self.draft.clear();
    }

    fn is_browsing(&self) -> bool {
        self.cursor.is_some()
    }

    #[cfg(test)]
    fn draft(&self) -> &str {
        &self.draft
    }
}

pub struct Composer<'a> {
    textarea: TextArea<'a>,
    history: InputHistory,
    file_refs: Vec<FileRef>,
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
            history: InputHistory::new(),
            file_refs: Vec::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.code, key.modifiers) {
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let text: String = self.textarea.lines().join("\n");
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    return None;
                }
                self.history.push(&trimmed);
                // Clear the textarea
                self.textarea.select_all();
                self.textarea.cut();
                Some(trimmed)
            }
            (KeyCode::Enter, m)
                if m.contains(KeyModifiers::SHIFT) || m.contains(KeyModifiers::ALT) =>
            {
                self.textarea.insert_newline();
                self.history.reset();
                None
            }
            (KeyCode::Up, KeyModifiers::NONE) if self.is_single_line() => {
                let current = self.textarea.lines().join("\n");
                if let Some(entry) = self.history.up(&current) {
                    let entry = entry.to_string();
                    self.set_text(&entry);
                }
                None
            }
            (KeyCode::Down, KeyModifiers::NONE) if self.history.is_browsing() => {
                match self.history.down() {
                    Some(Some(entry)) => {
                        let entry = entry.to_string();
                        self.set_text(&entry);
                    }
                    Some(None) => {
                        // Past newest history entry: clear input instead of restoring draft
                        self.set_text("");
                    }
                    None => {}
                }
                None
            }
            // Ctrl+P — emacs-style history back (same as Up when single-line)
            (KeyCode::Char('p'), m)
                if m.contains(KeyModifiers::CONTROL) && self.is_single_line() =>
            {
                let current = self.textarea.lines().join("\n");
                if let Some(entry) = self.history.up(&current) {
                    let entry = entry.to_string();
                    self.set_text(&entry);
                }
                None
            }
            // Ctrl+N — emacs-style history forward (same as Down when browsing)
            (KeyCode::Char('n'), m)
                if m.contains(KeyModifiers::CONTROL) && self.history.is_browsing() =>
            {
                match self.history.down() {
                    Some(Some(entry)) => {
                        let entry = entry.to_string();
                        self.set_text(&entry);
                    }
                    Some(None) => {
                        // Past newest history entry: clear input instead of restoring draft
                        self.set_text("");
                    }
                    None => {}
                }
                None
            }
            (KeyCode::Esc, _) => {
                self.textarea.select_all();
                self.textarea.cut();
                self.history.reset();
                self.file_refs.clear();
                None
            }
            _ => {
                self.history.reset();
                self.textarea.input(key);
                None
            }
        }
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn set_text(&mut self, text: &str) {
        self.textarea.select_all();
        self.textarea.cut();
        self.textarea.insert_str(text);
    }

    pub fn add_file_ref(&mut self, display: String, path: PathBuf) {
        self.file_refs.push(FileRef {
            display: display.clone(),
            path,
        });
        let current = self.text();
        // Replace the partial @query with the completed @mention
        let new_text = if let Some(at_pos) = current.rfind('@') {
            format!("{}@{display} ", &current[..at_pos])
        } else {
            format!("{current}@{display} ")
        };
        self.set_text(&new_text);
    }

    pub fn take_file_refs(&mut self) -> Vec<FileRef> {
        std::mem::take(&mut self.file_refs)
    }

    pub fn height(&self) -> u16 {
        let content_lines = self.textarea.lines().len() as u16;
        // 2 for border, min 1 content line
        (content_lines.max(1) + 2).min(10)
    }

    pub fn is_empty(&self) -> bool {
        self.is_single_line() && self.textarea.lines().first().is_none_or(String::is_empty)
    }

    #[cfg(test)]
    pub fn is_browsing_history(&self) -> bool {
        self.history.is_browsing()
    }

    fn is_single_line(&self) -> bool {
        self.textarea.lines().len() <= 1
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(&self.textarea, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_push_and_up() {
        let mut h = InputHistory::new();
        h.push("first");
        h.push("second");
        h.push("third");

        assert_eq!(h.up(""), Some("third"));
        assert_eq!(h.up(""), Some("second"));
        assert_eq!(h.up(""), Some("first"));
    }

    #[test]
    fn test_history_down_returns_to_draft() {
        let mut h = InputHistory::new();
        h.push("aaa");
        h.push("bbb");

        h.up("my draft");
        h.up("my draft");
        // cursor at 0 ("aaa")
        assert_eq!(h.up("my draft"), Some("aaa"));

        // go forward
        assert_eq!(h.down(), Some(Some("bbb")));
        // past newest → restore draft
        assert_eq!(h.down(), Some(None));
        assert_eq!(h.draft(), "my draft");
        assert!(!h.is_browsing());
    }

    #[test]
    fn test_history_reset_on_new_input() {
        let mut h = InputHistory::new();
        h.push("aaa");
        h.up("");
        assert!(h.is_browsing());
        h.reset();
        assert!(!h.is_browsing());
    }

    #[test]
    fn test_history_empty() {
        let mut h = InputHistory::new();
        assert_eq!(h.up(""), None);
        assert!(!h.is_browsing());
    }

    #[test]
    fn test_history_up_clamps_at_oldest() {
        let mut h = InputHistory::new();
        h.push("only");

        assert_eq!(h.up(""), Some("only"));
        assert_eq!(h.up(""), Some("only"));
        assert_eq!(h.up(""), Some("only"));
    }

    #[test]
    fn test_down_past_newest_clears_input() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut c = Composer::new();
        c.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        c.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        c.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        // Up loads "hi"
        c.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(c.text(), "hi");

        // Down past newest should clear
        c.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(c.text(), "");
        assert!(!c.is_browsing_history());
    }

    #[test]
    fn test_down_past_newest_clears_even_with_draft() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut c = Composer::new();
        c.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        c.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        c.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        c.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        // Type draft text
        c.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
        c.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        c.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));

        // Up saves draft and loads "old"
        c.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(c.text(), "old");

        // Down past newest should clear, NOT restore "wip"
        c.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(c.text(), "");
    }

    #[test]
    fn test_ctrl_n_past_newest_clears_input() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut c = Composer::new();
        c.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        c.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        // Ctrl+P to go back
        c.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert_eq!(c.text(), "x");

        // Ctrl+N past newest should clear
        c.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(c.text(), "");
        assert!(!c.is_browsing_history());
    }

    #[test]
    fn test_history_preserves_draft() {
        let mut h = InputHistory::new();
        h.push("old");

        let result = h.up("work in progress");
        assert_eq!(result, Some("old"));

        // down past newest restores draft
        let result = h.down();
        assert_eq!(result, Some(None));
        assert_eq!(h.draft(), "work in progress");
    }
}
