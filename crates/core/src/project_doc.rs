//! Project documentation discovery.
//!
//! Walks upward from CWD to the git root, collecting `AGENTS.md` and `CLAUDE.md`
//! files. These provide project-specific instructions to the agent, similar to
//! how Codex uses AGENTS.md for per-project context.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Filenames to search for, checked in order per directory.
const DOC_FILENAMES: &[&str] = &["AGENTS.md", "CLAUDE.md"];

/// Maximum total bytes to read from project docs (32 KiB).
const MAX_PROJECT_DOC_BYTES: usize = 32 * 1024;

/// Walk upward from `cwd` to the project root, collecting AGENTS.md / CLAUDE.md files.
///
/// Returns concatenated contents ordered from root to cwd (root doc first,
/// most-specific last). Returns `None` if no docs found.
pub fn discover_project_docs(cwd: &Path) -> Result<Option<String>> {
    let root = find_project_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let doc_paths = collect_doc_paths(&root, cwd);

    if doc_paths.is_empty() {
        return Ok(None);
    }

    let mut combined = String::new();
    let mut remaining_bytes = MAX_PROJECT_DOC_BYTES;

    for path in &doc_paths {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                if content.len() > remaining_bytes {
                    // Truncate to fit byte budget (find a valid UTF-8 boundary)
                    let truncated = &content[..floor_char_boundary(&content, remaining_bytes)];
                    if !combined.is_empty() {
                        combined.push_str("\n\n---\n\n");
                    }
                    combined.push_str(&format!("<!-- {} -->\n{}", path.display(), truncated));
                    break;
                }
                if !combined.is_empty() {
                    combined.push_str("\n\n---\n\n");
                }
                combined.push_str(&format!("<!-- {} -->\n{}", path.display(), content));
                remaining_bytes -= content.len();
            }
            Err(e) => {
                tracing::warn!("Failed to read project doc {}: {e}", path.display());
            }
        }
    }

    if combined.is_empty() {
        Ok(None)
    } else {
        Ok(Some(combined))
    }
}

/// Find the largest byte index <= `max` that is a valid UTF-8 char boundary.
fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut idx = max;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Find the project root by walking up looking for `.git`.
fn find_project_root(cwd: &Path) -> Option<PathBuf> {
    crate::git::find_repo_root(cwd)
}

/// Collect doc file paths from root down to cwd.
///
/// For each directory on the path from root to cwd, checks for AGENTS.md
/// then CLAUDE.md (first match wins per directory).
fn collect_doc_paths(root: &Path, cwd: &Path) -> Vec<PathBuf> {
    // Build list of directories from root down to cwd
    let mut dirs = Vec::new();
    let mut current = cwd.to_path_buf();

    // Walk from cwd up to root, collecting directories
    loop {
        dirs.push(current.clone());
        if current == root {
            break;
        }
        if !current.pop() {
            break;
        }
    }

    // Reverse so root is first
    dirs.reverse();
    // Deduplicate (root might appear twice if cwd == root)
    dirs.dedup();

    let mut doc_paths = Vec::new();
    for dir in &dirs {
        for filename in DOC_FILENAMES {
            let path = dir.join(filename);
            if path.is_file() {
                doc_paths.push(path);
                break; // First match per directory wins
            }
        }
    }

    doc_paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn discover_no_docs() {
        let tmp = TempDir::new().unwrap();
        let result = discover_project_docs(tmp.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn discover_agents_md() {
        let tmp = TempDir::new().unwrap();
        // Create a fake .git to mark project root
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "# Project Rules\nBe nice.").unwrap();

        let result = discover_project_docs(tmp.path()).unwrap();
        assert!(result.is_some());
        let content = result.unwrap();
        assert!(content.contains("# Project Rules"));
        assert!(content.contains("Be nice."));
    }

    #[test]
    fn discover_claude_md() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join("CLAUDE.md"), "# Claude Instructions").unwrap();

        let result = discover_project_docs(tmp.path()).unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().contains("# Claude Instructions"));
    }

    #[test]
    fn agents_md_takes_priority_over_claude_md() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "agents content").unwrap();
        std::fs::write(tmp.path().join("CLAUDE.md"), "claude content").unwrap();

        let result = discover_project_docs(tmp.path()).unwrap();
        let content = result.unwrap();
        // Should contain agents content but not claude (first match per dir wins)
        assert!(content.contains("agents content"));
        assert!(!content.contains("claude content"));
    }

    #[test]
    fn discover_nested_docs() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "root rules").unwrap();

        let sub = tmp.path().join("packages").join("web");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("CLAUDE.md"), "web rules").unwrap();

        let result = discover_project_docs(&sub).unwrap();
        let content = result.unwrap();
        // Root doc should come first
        assert!(content.contains("root rules"));
        assert!(content.contains("web rules"));
        let root_pos = content.find("root rules").unwrap();
        let web_pos = content.find("web rules").unwrap();
        assert!(root_pos < web_pos);
    }

    #[test]
    fn respects_byte_budget() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        // Write a file larger than MAX_PROJECT_DOC_BYTES
        let large = "x".repeat(MAX_PROJECT_DOC_BYTES + 1000);
        std::fs::write(tmp.path().join("AGENTS.md"), &large).unwrap();

        let result = discover_project_docs(tmp.path()).unwrap();
        let content = result.unwrap();
        assert!(content.len() <= MAX_PROJECT_DOC_BYTES + 200); // some overhead for path comment
    }
}
