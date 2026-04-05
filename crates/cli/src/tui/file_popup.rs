use std::path::{Path, PathBuf, MAIN_SEPARATOR};

use ignore::WalkBuilder;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme;

pub struct FileMatch {
    /// Human-readable path shown in the popup and inserted into the composer.
    /// Preserves the user's typed prefix form (e.g. `~/Documents/foo.txt`).
    pub display: String,
    /// Fully resolved absolute path used for reading file contents downstream.
    pub full_path: PathBuf,
    /// True when the entry is a directory. Directory selections keep the
    /// popup open so the user can drill further into the tree.
    pub is_dir: bool,
}

pub struct FileSearchPopup {
    visible: bool,
    query: String,
    selected: usize,
    results: Vec<FileMatch>,
    cwd: PathBuf,
    blocked_paths: Vec<String>,
}

impl FileSearchPopup {
    #[cfg(test)]
    fn new() -> Self {
        Self::with_config(
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            Vec::new(),
        )
    }

    pub fn with_config(cwd: PathBuf, blocked_paths: Vec<String>) -> Self {
        Self {
            visible: false,
            query: String::new(),
            selected: 0,
            results: Vec::new(),
            cwd,
            blocked_paths,
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

        if is_path_like(query) {
            self.update_path(query);
        } else {
            self.update_fuzzy(query);
        }
    }

    fn update_fuzzy(&mut self, query: &str) {
        let query_lower = query.to_lowercase();

        let walker = WalkBuilder::new(&self.cwd)
            .max_depth(Some(8))
            .hidden(true)
            .build();

        for entry in walker.flatten() {
            if self.results.len() >= 50 {
                break;
            }
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

            if self.is_blocked(path) {
                continue;
            }

            if query_lower.is_empty() || rel.to_lowercase().contains(&query_lower) {
                self.results.push(FileMatch {
                    display: rel.to_string(),
                    full_path: path.to_path_buf(),
                    is_dir: false,
                });
            }
        }
    }

    fn update_path(&mut self, query: &str) {
        let (parent_frag, name_frag) = split_parent_fragment(query);

        // Resolve the parent directory against cwd / home / absolute.
        let Some(resolved_parent) = resolve_path_fragment(parent_frag, &self.cwd) else {
            return;
        };

        let read_dir = match std::fs::read_dir(&resolved_parent) {
            Ok(rd) => rd,
            Err(_) => return,
        };

        let name_lower = name_frag.to_lowercase();

        // Preserve the user's typed prefix form for display.
        // If the query ends with a separator, the entire query is the parent
        // prefix; otherwise strip the final segment.
        let display_parent: String = if name_frag.is_empty() {
            query.to_string()
        } else {
            query[..query.len() - name_frag.len()].to_string()
        };

        let mut entries: Vec<(String, PathBuf, bool)> = Vec::new();
        for entry in read_dir.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy().to_string();

            // Case-insensitive prefix match.
            if !name_lower.is_empty() && !name.to_lowercase().starts_with(&name_lower) {
                continue;
            }

            let full_path = entry.path();
            if self.is_blocked(&full_path) {
                continue;
            }

            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            entries.push((name, full_path, is_dir));
        }

        // Sort: directories first, then files, alphabetical (case-insensitive).
        entries.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });
        entries.truncate(50);

        for (name, full_path, is_dir) in entries {
            let mut display = format!("{display_parent}{name}");
            if is_dir {
                display.push(MAIN_SEPARATOR);
            }
            self.results.push(FileMatch {
                display,
                full_path,
                is_dir,
            });
        }
    }

    fn is_blocked(&self, path: &Path) -> bool {
        if self.blocked_paths.is_empty() {
            return false;
        }
        let path_str = path.to_string_lossy();
        self.blocked_paths.iter().any(|blocked| {
            // Match blocked segment anywhere in the path (mirrors other tool filtering).
            path.components().any(|c| c.as_os_str() == blocked.as_str())
                || path_str.contains(blocked.as_str())
        })
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

/// Returns true if the query looks like a filesystem path rather than a fuzzy
/// filename search. Triggers path-mode completion against a resolved parent
/// directory.
fn is_path_like(query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    if query.starts_with('~') || query.starts_with('/') {
        return true;
    }
    if query.starts_with("./") || query.starts_with("../") {
        return true;
    }
    if query.chars().any(std::path::is_separator) {
        return true;
    }
    // Windows drive prefix: `C:\` or `C:/`.
    let bytes = query.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
    {
        return true;
    }
    false
}

/// Split a path-like query into (parent_fragment, name_fragment) at the last
/// separator. If the query ends with a separator, the name fragment is empty.
/// If the query has no separator, the entire query is the name fragment and
/// the parent is empty.
pub(crate) fn split_parent_fragment(query: &str) -> (&str, &str) {
    // Find the last separator (either `/` or OS-native `\`).
    let last_sep = query
        .char_indices()
        .rev()
        .find(|(_, c)| std::path::is_separator(*c));
    match last_sep {
        Some((idx, c)) => {
            let split = idx + c.len_utf8();
            (&query[..split], &query[split..])
        }
        None => ("", query),
    }
}

/// Resolve a parent-path fragment to an absolute directory:
/// - `~` or `~/...` expand via `dirs::home_dir()`
/// - relative fragments resolve against `cwd`
/// - absolute fragments are returned as-is
/// - empty fragment means "current working directory"
fn resolve_path_fragment(fragment: &str, cwd: &Path) -> Option<PathBuf> {
    if fragment.is_empty() {
        return Some(cwd.to_path_buf());
    }

    // Tilde expansion: `~` alone, `~/...`, or `~\...`.
    if fragment == "~" {
        return dirs::home_dir();
    }
    if let Some(rest) = fragment
        .strip_prefix("~/")
        .or_else(|| fragment.strip_prefix("~\\"))
    {
        let home = dirs::home_dir()?;
        return Some(home.join(rest));
    }

    let path = Path::new(fragment);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(cwd.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.txt"), "a").unwrap();
        fs::write(tmp.path().join("b.txt"), "b").unwrap();
        let mut popup = FileSearchPopup::with_config(tmp.path().to_path_buf(), Vec::new());
        popup.update_query("");
        let count = popup.results.len();
        assert!(count >= 2);
        assert_eq!(popup.selected, 0);
        popup.move_up();
        assert_eq!(popup.selected, count - 1);
        popup.move_down();
        assert_eq!(popup.selected, 0);
    }

    // ------------------- is_path_like -------------------

    #[test]
    fn is_path_like_detects_tilde() {
        assert!(is_path_like("~"));
        assert!(is_path_like("~/"));
        assert!(is_path_like("~/Documents"));
    }

    #[test]
    fn is_path_like_detects_absolute() {
        assert!(is_path_like("/etc"));
        assert!(is_path_like("/etc/hosts"));
    }

    #[test]
    fn is_path_like_detects_relative_dot_prefix() {
        assert!(is_path_like("./foo"));
        assert!(is_path_like("../bar"));
    }

    #[test]
    fn is_path_like_detects_embedded_separator() {
        assert!(is_path_like("foo/bar"));
    }

    #[test]
    fn is_path_like_detects_windows_drive() {
        assert!(is_path_like("C:/Users"));
        assert!(is_path_like("C:\\Users"));
        assert!(is_path_like("d:/x"));
    }

    #[test]
    fn is_path_like_rejects_bare_names() {
        assert!(!is_path_like("foo"));
        assert!(!is_path_like("Cargo"));
        assert!(!is_path_like("my_file.txt"));
        assert!(!is_path_like(""));
    }

    // ------------------- split_parent_fragment -------------------

    #[test]
    fn split_no_separator() {
        assert_eq!(split_parent_fragment("foo"), ("", "foo"));
        assert_eq!(split_parent_fragment(""), ("", ""));
    }

    #[test]
    fn split_trailing_separator() {
        assert_eq!(split_parent_fragment("~/"), ("~/", ""));
        assert_eq!(split_parent_fragment("/etc/"), ("/etc/", ""));
        assert_eq!(split_parent_fragment("foo/bar/"), ("foo/bar/", ""));
    }

    #[test]
    fn split_mid_path() {
        assert_eq!(split_parent_fragment("~/Doc"), ("~/", "Doc"));
        assert_eq!(split_parent_fragment("/etc/ho"), ("/etc/", "ho"));
        assert_eq!(split_parent_fragment("./foo/ba"), ("./foo/", "ba"));
    }

    #[cfg(windows)]
    #[test]
    fn split_windows_backslash() {
        assert_eq!(split_parent_fragment("~\\Doc"), ("~\\", "Doc"));
        assert_eq!(
            split_parent_fragment("C:\\Users\\me\\fo"),
            ("C:\\Users\\me\\", "fo")
        );
    }

    // ------------------- resolve_path_fragment -------------------

    #[test]
    fn resolve_empty_is_cwd() {
        let cwd = PathBuf::from("/tmp/cwd");
        assert_eq!(resolve_path_fragment("", &cwd), Some(cwd));
    }

    #[test]
    fn resolve_bare_tilde() {
        let cwd = PathBuf::from("/tmp");
        let resolved = resolve_path_fragment("~", &cwd);
        assert_eq!(resolved, dirs::home_dir());
    }

    #[test]
    fn resolve_tilde_slash_prefix() {
        let cwd = PathBuf::from("/tmp");
        let resolved = resolve_path_fragment("~/Documents/", &cwd).unwrap();
        let mut expected = dirs::home_dir().unwrap();
        expected.push("Documents/");
        assert_eq!(resolved, expected);
    }

    #[cfg(unix)]
    #[test]
    fn resolve_absolute_path() {
        let cwd = PathBuf::from("/tmp");
        assert_eq!(
            resolve_path_fragment("/etc/", &cwd),
            Some(PathBuf::from("/etc/"))
        );
    }

    #[test]
    fn resolve_relative_joins_cwd() {
        let cwd = PathBuf::from("/tmp/project");
        assert_eq!(
            resolve_path_fragment("src/", &cwd),
            Some(PathBuf::from("/tmp/project/src/"))
        );
    }

    // ------------------- path mode integration -------------------

    #[test]
    fn path_mode_lists_direct_children_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("top.txt"), "x").unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("subdir").join("nested.txt"), "x").unwrap();

        // Query with a trailing separator to list the whole dir.
        let query = format!("{}/", tmp.path().display());
        let mut popup = FileSearchPopup::with_config(PathBuf::from("/"), Vec::new());
        popup.update_query(&query);

        let names: Vec<_> = popup.results.iter().map(|m| m.display.clone()).collect();
        assert!(names.iter().any(|n| n.ends_with("top.txt")));
        // Directory entry should have a trailing separator.
        assert!(names
            .iter()
            .any(|n| n.ends_with(&format!("subdir{MAIN_SEPARATOR}"))));
        // Nested file must NOT appear — path mode is single-level.
        assert!(!names.iter().any(|n| n.contains("nested.txt")));
    }

    #[test]
    fn path_mode_case_insensitive_prefix_filter() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Alpha.txt"), "x").unwrap();
        fs::write(tmp.path().join("beta.txt"), "x").unwrap();
        fs::write(tmp.path().join("alphabet.md"), "x").unwrap();

        let query = format!("{}/alp", tmp.path().display());
        let mut popup = FileSearchPopup::with_config(PathBuf::from("/"), Vec::new());
        popup.update_query(&query);

        let names: Vec<_> = popup.results.iter().map(|m| m.display.clone()).collect();
        assert_eq!(
            names.len(),
            2,
            "expected Alpha.txt + alphabet.md, got {names:?}"
        );
        assert!(names.iter().any(|n| n.ends_with("Alpha.txt")));
        assert!(names.iter().any(|n| n.ends_with("alphabet.md")));
        assert!(!names.iter().any(|n| n.ends_with("beta.txt")));
    }

    #[test]
    fn path_mode_sorts_directories_before_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a_file.txt"), "x").unwrap();
        fs::create_dir(tmp.path().join("z_dir")).unwrap();

        let query = format!("{}/", tmp.path().display());
        let mut popup = FileSearchPopup::with_config(PathBuf::from("/"), Vec::new());
        popup.update_query(&query);

        assert!(popup.results.len() >= 2);
        // z_dir (directory) comes before a_file.txt even though it's later alphabetically.
        assert!(popup.results[0].is_dir);
        assert!(popup.results[0]
            .display
            .ends_with(&format!("z_dir{MAIN_SEPARATOR}")));
    }

    #[test]
    fn path_mode_preserves_display_prefix() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "x").unwrap();

        // Using a relative-style query resolved against cwd=tmp.
        let mut popup = FileSearchPopup::with_config(tmp.path().to_path_buf(), Vec::new());
        popup.update_query("./fil");

        assert_eq!(popup.results.len(), 1);
        assert_eq!(popup.results[0].display, "./file.txt");
        assert!(!popup.results[0].is_dir);
        // full_path should be the resolved absolute path, not the display form.
        assert!(
            popup.results[0].full_path.is_absolute()
                || popup.results[0].full_path.starts_with(tmp.path())
        );
    }

    #[test]
    fn path_mode_empty_name_fragment_lists_all() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("one.txt"), "x").unwrap();
        fs::write(tmp.path().join("two.txt"), "x").unwrap();

        let mut popup = FileSearchPopup::with_config(tmp.path().to_path_buf(), Vec::new());
        popup.update_query("./");
        assert_eq!(popup.results.len(), 2);
    }

    #[test]
    fn path_mode_nonexistent_parent_yields_no_results() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does_not_exist");
        let query = format!("{}/", missing.display());

        let mut popup = FileSearchPopup::with_config(PathBuf::from("/"), Vec::new());
        popup.update_query(&query);
        assert!(popup.results.is_empty());
    }

    // ------------------- blocked_paths -------------------

    #[test]
    fn blocked_paths_filter_in_path_mode() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".ssh")).unwrap();
        fs::write(tmp.path().join("public.txt"), "x").unwrap();

        let mut popup =
            FileSearchPopup::with_config(tmp.path().to_path_buf(), vec![".ssh".to_string()]);
        popup.update_query("./");

        let names: Vec<_> = popup.results.iter().map(|m| m.display.clone()).collect();
        assert!(names.iter().any(|n| n.ends_with("public.txt")));
        assert!(
            !names.iter().any(|n| n.contains(".ssh")),
            ".ssh must be filtered, got {names:?}"
        );
    }

    #[test]
    fn blocked_paths_filter_in_fuzzy_mode() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("subdir").join("credentials.txt"), "x").unwrap();
        fs::write(tmp.path().join("subdir").join("ok.txt"), "x").unwrap();

        let mut popup = FileSearchPopup::with_config(
            tmp.path().to_path_buf(),
            vec!["credentials.txt".to_string()],
        );
        popup.update_query("txt");

        let names: Vec<_> = popup.results.iter().map(|m| m.display.clone()).collect();
        assert!(names.iter().any(|n| n.ends_with("ok.txt")));
        assert!(!names.iter().any(|n| n.contains("credentials.txt")));
    }

    // ------------------- fuzzy mode unchanged for bare names -------------------

    #[test]
    fn bare_query_uses_fuzzy_walk() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("nested")).unwrap();
        fs::write(tmp.path().join("nested").join("deep.rs"), "x").unwrap();

        let mut popup = FileSearchPopup::with_config(tmp.path().to_path_buf(), Vec::new());
        popup.update_query("deep");

        // Recursive walk finds nested/deep.rs even though it's not in the top level.
        assert!(popup.results.iter().any(|m| m.display.ends_with("deep.rs")));
    }
}
