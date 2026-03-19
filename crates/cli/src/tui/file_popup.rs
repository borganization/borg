use std::path::PathBuf;

use ignore::WalkBuilder;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme;

pub struct FileMatch {
    pub display: String,
    pub full_path: PathBuf,
}

pub struct FileSearchPopup {
    visible: bool,
    query: String,
    selected: usize,
    results: Vec<FileMatch>,
    cwd: PathBuf,
}

impl FileSearchPopup {
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            selected: 0,
            results: Vec::new(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn update_query(&mut self, query: &str) {
        self.visible = true;
        self.query = query.to_string();
        self.selected = 0;
        self.results.clear();

        let query_lower = query.to_lowercase();

        let walker = WalkBuilder::new(&self.cwd)
            .max_depth(Some(8))
            .hidden(true)
            .build();

        let mut count = 0;
        for entry in walker.flatten() {
            if count >= 50 {
                break;
            }
            // Skip directories
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                continue;
            }
            let path = entry.path();
            let rel = path
                .strip_prefix(&self.cwd)
                .unwrap_or(path)
                .to_string_lossy();

            if rel.is_empty() {
                continue;
            }

            if query_lower.is_empty() || rel.to_lowercase().contains(&query_lower) {
                self.results.push(FileMatch {
                    display: rel.to_string(),
                    full_path: path.to_path_buf(),
                });
                count += 1;
            }
        }
    }

    pub fn move_up(&mut self) {
        if self.results.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.results.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.results.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.results.len();
    }

    pub fn selected_file(&self) -> Option<&FileMatch> {
        self.results.get(self.selected)
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
        self.selected = 0;
        self.query.clear();
        self.results.clear();
    }

    pub fn render(&self, frame: &mut Frame, composer_area: Rect) {
        if !self.visible {
            return;
        }

        if self.results.is_empty() {
            return;
        }

        let available_above = composer_area.y;
        if available_above < 3 {
            return;
        }

        let item_count = self.results.len() as u16;
        let popup_height = (item_count + 2).min(available_above);
        let max_visible = (popup_height - 2) as usize;
        let popup_width = composer_area.width.min(60);

        let popup_y = composer_area.y - popup_height;
        let popup_area = Rect::new(composer_area.x, popup_y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let scroll_offset = if self.selected >= max_visible {
            self.selected - max_visible + 1
        } else {
            0
        };

        let lines: Vec<Line<'_>> = self
            .results
            .iter()
            .skip(scroll_offset)
            .take(max_visible)
            .enumerate()
            .map(|(i, file)| {
                let actual_index = i + scroll_offset;
                let style = if actual_index == self.selected {
                    theme::popup_selected()
                } else {
                    theme::dim()
                };
                Line::from(Span::styled(format!(" {}", file.display), style))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::dim())
            .title(" Files ");

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, popup_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_popup_is_not_visible() {
        let popup = FileSearchPopup::new();
        assert!(!popup.is_visible());
    }

    #[test]
    fn dismiss_hides_popup() {
        let mut popup = FileSearchPopup::new();
        popup.update_query("");
        assert!(popup.is_visible());
        popup.dismiss();
        assert!(!popup.is_visible());
    }

    #[test]
    fn up_down_wrapping() {
        let mut popup = FileSearchPopup::new();
        popup.update_query("");
        if popup.results.is_empty() {
            return; // no files in cwd, skip
        }
        let count = popup.results.len();
        assert_eq!(popup.selected, 0);
        popup.move_up();
        assert_eq!(popup.selected, count - 1);
        popup.move_down();
        assert_eq!(popup.selected, 0);
    }
}
