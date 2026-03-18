use thiserror::Error;

#[derive(Debug, Error, PartialEq, Clone)]
pub enum ParseError {
    #[error("Invalid patch format: {0}")]
    InvalidFormat(String),
    #[error("Invalid hunk at line {line_number}: {message}")]
    InvalidHunk { message: String, line_number: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatchOperation {
    AddFile {
        path: String,
        content: String,
    },
    UpdateFile {
        path: String,
        move_to: Option<String>,
        hunks: Vec<Hunk>,
    },
    DeleteFile {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hunk {
    pub context_hint: Option<String>,
    pub search: String,
    pub replace: String,
    pub is_end_of_file: bool,
    pub source_line: usize,
}

#[derive(Debug, PartialEq)]
pub struct Patch {
    pub operations: Vec<PatchOperation>,
}

// ── Marker constants ──

const BEGIN_PATCH_MARKER: &str = "*** Begin Patch";
const END_PATCH_MARKER: &str = "*** End Patch";
const ADD_FILE_MARKER: &str = "*** Add File: ";
const DELETE_FILE_MARKER: &str = "*** Delete File: ";
const UPDATE_FILE_MARKER: &str = "*** Update File: ";
const MOVE_TO_MARKER: &str = "*** Move to: ";
const EOF_MARKER: &str = "*** End of File";

/// Parse a patch string into a structured `Patch`.
///
/// The format follows the codex apply-patch DSL:
///
/// ```text
/// *** Begin Patch
/// *** Add File: path
/// +line1
/// +line2
/// *** Update File: path
/// @@ optional context
///  context line
/// -removed line
/// +added line
/// *** Delete File: path
/// *** End Patch
/// ```
///
/// Key rules:
/// - `*** Begin Patch` and `*** End Patch` are required boundaries.
/// - Add File content lines MUST start with `+` (stripped when stored).
///   Lines without `+` terminate the add block — this prevents ambiguity
///   when file content itself contains `***` markers.
/// - Update File hunk lines use unified diff prefixes: ` ` (context),
///   `-` (remove), `+` (add). Lines without these prefixes terminate the hunk.
/// - Delete File has no body.
pub fn parse_patch(input: &str) -> Result<Patch, ParseError> {
    let lines: Vec<&str> = input.lines().collect();
    let mut operations = Vec::new();
    let mut i = 0;

    // Skip leading whitespace/empty lines
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }

    // Expect "*** Begin Patch" (optional — be lenient for backward compat)
    if i < lines.len() && lines[i].trim() == BEGIN_PATCH_MARKER {
        i += 1;
    }

    while i < lines.len() {
        let line = lines[i].trim();

        if line == END_PATCH_MARKER || line.is_empty() {
            i += 1;
            continue;
        }

        if let Some(path) = line.strip_prefix(ADD_FILE_MARKER) {
            let path = path.trim().to_string();
            i += 1;

            // Collect content: every line MUST start with '+'.
            // Lines without '+' terminate the block (they belong to the
            // next operation or are the *** End Patch marker).
            let mut content = String::new();
            while i < lines.len() {
                if let Some(added) = lines[i].strip_prefix('+') {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(added);
                    i += 1;
                } else {
                    break;
                }
            }
            operations.push(PatchOperation::AddFile { path, content });
        } else if let Some(path) = line.strip_prefix(UPDATE_FILE_MARKER) {
            let path = path.trim().to_string();
            i += 1;

            // Check for optional *** Move to: line
            let move_to = if i < lines.len() {
                let next = lines[i].trim();
                if let Some(dest) = next.strip_prefix(MOVE_TO_MARKER) {
                    i += 1;
                    Some(dest.trim().to_string())
                } else {
                    None
                }
            } else {
                None
            };

            let mut hunks = Vec::new();

            // Parse chunks until we hit a *** marker (next operation or end)
            while i < lines.len() && !lines[i].starts_with("***") {
                let hunk_line = lines[i].trim();

                // Skip blank lines between chunks
                if hunk_line.is_empty() {
                    i += 1;
                    continue;
                }

                if let Some(after_at) = hunk_line.strip_prefix("@@") {
                    let hunk_source_line = i + 1; // 1-based line number

                    // Extract context hint from @@ header (strip trailing @@ if present)
                    let context_hint = {
                        let trimmed = after_at.trim();
                        let trimmed = trimmed.strip_suffix("@@").unwrap_or(trimmed).trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        }
                    };

                    i += 1;
                    let mut search_lines = Vec::new();
                    let mut replace_lines = Vec::new();
                    let mut is_end_of_file = false;

                    // Collect diff lines: must start with ' ', '-', or '+'
                    while i < lines.len() {
                        let l = lines[i];

                        // Check for *** End of File marker
                        if l.trim() == EOF_MARKER {
                            is_end_of_file = true;
                            i += 1;
                            break;
                        }

                        if let Some(removed) = l.strip_prefix('-') {
                            search_lines.push(removed);
                        } else if let Some(added) = l.strip_prefix('+') {
                            replace_lines.push(added);
                        } else if let Some(context) = l.strip_prefix(' ') {
                            search_lines.push(context);
                            replace_lines.push(context);
                        } else if l.is_empty() {
                            // Treat empty line as empty context
                            search_lines.push(l);
                            replace_lines.push(l);
                        } else {
                            // Line doesn't have a diff prefix — terminates this chunk
                            break;
                        }
                        i += 1;
                    }

                    hunks.push(Hunk {
                        context_hint,
                        search: search_lines.join("\n"),
                        replace: replace_lines.join("\n"),
                        is_end_of_file,
                        source_line: hunk_source_line,
                    });
                } else {
                    // Non-@@ non-blank line — skip (shouldn't happen in well-formed input)
                    i += 1;
                }
            }

            operations.push(PatchOperation::UpdateFile {
                path,
                move_to,
                hunks,
            });
        } else if let Some(path) = line.strip_prefix(DELETE_FILE_MARKER) {
            let path = path.trim().to_string();
            operations.push(PatchOperation::DeleteFile { path });
            i += 1;
        } else {
            i += 1;
        }
    }

    if operations.is_empty() {
        return Err(ParseError::InvalidFormat(
            "No operations found in patch".to_string(),
        ));
    }

    Ok(Patch { operations })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Add File ──

    #[test]
    fn parse_add_file() {
        let input = "\
*** Begin Patch
*** Add File: src/new.rs
+fn main() {}
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 1);
        match &patch.operations[0] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "src/new.rs");
                assert_eq!(content, "fn main() {}");
            }
            other => panic!("Expected AddFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_add_file_multiline_content() {
        let input = "\
*** Begin Patch
*** Add File: src/main.rs
+fn main() {
+    println!(\"hello\");
+    println!(\"world\");
+}
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 1);
        match &patch.operations[0] {
            PatchOperation::AddFile { content, .. } => {
                assert!(content.contains("println!(\"hello\");"));
                assert!(content.contains("println!(\"world\");"));
                assert_eq!(content.lines().count(), 4);
            }
            other => panic!("Expected AddFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_add_file_content_with_embedded_patch_markers() {
        // Content that contains "*** " text is safe because those lines
        // will have '+' prefix. Lines without '+' terminate the block.
        let input = "\
*** Begin Patch
*** Add File: README.md
+# My Tool
+
+## Patch DSL
+
+Use the following format:
+
+*** Begin Patch
+*** Add File: tool-name/tool.toml
+content here
+*** End Patch
+
+That's how you create tools.
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(
            patch.operations.len(),
            1,
            "Should produce exactly one AddFile, got: {:?}",
            patch.operations
        );
        match &patch.operations[0] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "README.md");
                assert!(
                    content.contains("*** Begin Patch"),
                    "Content should preserve embedded *** Begin Patch"
                );
                assert!(
                    content.contains("*** Add File: tool-name/tool.toml"),
                    "Content should preserve embedded *** Add File"
                );
                assert!(
                    content.contains("That's how you create tools."),
                    "Content should include text after embedded patch example"
                );
            }
            other => panic!("Expected AddFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_add_file_content_with_markdown_bold() {
        let input = "\
*** Begin Patch
*** Add File: notes.md
+# Important
+
+***Note:*** This is critical.
+*** Bold header ***
+Some more text.
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 1);
        match &patch.operations[0] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "notes.md");
                assert!(content.contains("***Note:***"));
                assert!(content.contains("*** Bold header ***"));
                assert!(content.contains("Some more text."));
            }
            other => panic!("Expected AddFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_add_file_without_begin_end_markers() {
        // The parser is lenient and doesn't require Begin/End markers
        let input = "\
*** Add File: test.txt
+content here";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 1);
        match &patch.operations[0] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "test.txt");
                assert_eq!(content, "content here");
            }
            other => panic!("Expected AddFile, got {:?}", other),
        }
    }

    // ── Delete File ──

    #[test]
    fn parse_delete_file() {
        let input = "\
*** Begin Patch
*** Delete File: src/old.rs
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 1);
        match &patch.operations[0] {
            PatchOperation::DeleteFile { path } => {
                assert_eq!(path, "src/old.rs");
            }
            other => panic!("Expected DeleteFile, got {:?}", other),
        }
    }

    // ── Update File ──

    #[test]
    fn parse_update_file_with_hunks() {
        let input = "\
*** Begin Patch
*** Update File: src/lib.rs
@@ fn hello()
 fn hello() {
-    println!(\"hi\");
+    println!(\"hello world\");
 }
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 1);
        match &patch.operations[0] {
            PatchOperation::UpdateFile { path, hunks, .. } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(hunks.len(), 1);
                assert!(hunks[0].search.contains("println!(\"hi\");"));
                assert!(hunks[0].replace.contains("println!(\"hello world\");"));
                assert!(hunks[0].search.contains("fn hello()"));
                assert!(hunks[0].replace.contains("fn hello()"));
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_update_with_multiple_hunks() {
        let input = "\
*** Begin Patch
*** Update File: lib.rs
@@
-fn old_a() {}
+fn new_a() {}
@@
-fn old_b() {}
+fn new_b() {}
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile { hunks, .. } => {
                assert_eq!(hunks.len(), 2);
                assert!(hunks[0].search.contains("old_a"));
                assert!(hunks[0].replace.contains("new_a"));
                assert!(hunks[1].search.contains("old_b"));
                assert!(hunks[1].replace.contains("new_b"));
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_update_with_context_lines() {
        let input = "\
*** Begin Patch
*** Update File: main.rs
@@
 use std::io;
 fn main() {
-    old_call();
+    new_call();
 }
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile { hunks, .. } => {
                assert_eq!(hunks.len(), 1);
                assert!(hunks[0].search.contains("use std::io;"));
                assert!(hunks[0].replace.contains("use std::io;"));
                assert!(hunks[0].search.contains("old_call()"));
                assert!(!hunks[0].search.contains("new_call()"));
                assert!(hunks[0].replace.contains("new_call()"));
                assert!(!hunks[0].replace.contains("old_call()"));
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_update_followed_by_add() {
        // Ensures *** marker correctly terminates an update block
        let input = "\
*** Begin Patch
*** Update File: file.py
@@
+line
*** Add File: other.py
+content
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 2);
        match &patch.operations[0] {
            PatchOperation::UpdateFile { path, hunks, .. } => {
                assert_eq!(path, "file.py");
                assert_eq!(hunks.len(), 1);
                assert!(hunks[0].replace.contains("line"));
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
        match &patch.operations[1] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "other.py");
                assert_eq!(content, "content");
            }
            other => panic!("Expected AddFile, got {:?}", other),
        }
    }

    // ── Multiple operations ──

    #[test]
    fn parse_multiple_operations() {
        let input = "\
*** Begin Patch
*** Add File: a.txt
+hello
*** Delete File: b.txt
*** Update File: c.txt
@@
-old
+new
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 3);
        assert!(matches!(
            &patch.operations[0],
            PatchOperation::AddFile { .. }
        ));
        assert!(matches!(
            &patch.operations[1],
            PatchOperation::DeleteFile { .. }
        ));
        assert!(matches!(
            &patch.operations[2],
            PatchOperation::UpdateFile { .. }
        ));
    }

    #[test]
    fn parse_full_example() {
        // Mirrors the codex documentation example
        let input = "\
*** Begin Patch
*** Add File: hello.txt
+Hello world
*** Update File: src/app.py
@@ def greet():
-print(\"Hi\")
+print(\"Hello, world!\")
*** Delete File: obsolete.txt
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 3);

        match &patch.operations[0] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "hello.txt");
                assert_eq!(content, "Hello world");
            }
            other => panic!("Expected AddFile, got {:?}", other),
        }
        match &patch.operations[1] {
            PatchOperation::UpdateFile { path, hunks, .. } => {
                assert_eq!(path, "src/app.py");
                assert_eq!(hunks.len(), 1);
                assert!(hunks[0].search.contains("print(\"Hi\")"));
                assert!(hunks[0].replace.contains("print(\"Hello, world!\")"));
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
        match &patch.operations[2] {
            PatchOperation::DeleteFile { path } => {
                assert_eq!(path, "obsolete.txt");
            }
            other => panic!("Expected DeleteFile, got {:?}", other),
        }
    }

    // ── Error cases ──

    #[test]
    fn parse_empty_input_errors() {
        let result = parse_patch("");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No operations found"), "got: {err}");
    }

    #[test]
    fn parse_garbage_input_errors() {
        let result = parse_patch("this is not a patch at all");
        assert!(result.is_err());
    }

    // ── Edge cases ──

    #[test]
    fn parse_add_file_empty_content() {
        // Add file with no + lines produces empty content
        let input = "\
*** Begin Patch
*** Add File: empty.txt
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::AddFile { content, .. } => {
                assert_eq!(content, "");
            }
            other => panic!("Expected AddFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_update_empty_hunk_header() {
        // @@ with no context text is valid
        let input = "\
*** Begin Patch
*** Update File: test.rs
@@
-old
+new
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile { hunks, .. } => {
                assert_eq!(hunks.len(), 1);
                assert_eq!(hunks[0].search, "old");
                assert_eq!(hunks[0].replace, "new");
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_whitespace_padded_markers() {
        // Markers with leading/trailing whitespace should still be recognized
        let input = "  *** Begin Patch  \n*** Add File: f.txt\n+hi\n  *** End Patch  ";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 1);
        match &patch.operations[0] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "f.txt");
                assert_eq!(content, "hi");
            }
            other => panic!("Expected AddFile, got {:?}", other),
        }
    }

    // ── New feature tests ──

    #[test]
    fn parse_move_to() {
        let input = "\
*** Begin Patch
*** Update File: old.rs
*** Move to: new.rs
@@
-old
+new
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile {
                path,
                move_to,
                hunks,
                ..
            } => {
                assert_eq!(path, "old.rs");
                assert_eq!(move_to.as_deref(), Some("new.rs"));
                assert_eq!(hunks.len(), 1);
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_end_of_file_marker() {
        let input = "\
*** Begin Patch
*** Update File: f.txt
@@
-old
+new
*** End of File
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile { hunks, .. } => {
                assert_eq!(hunks.len(), 1);
                assert!(hunks[0].is_end_of_file);
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_context_hint() {
        let input = "\
*** Begin Patch
*** Update File: f.txt
@@ fn hello()
 fn hello() {
-    old();
+    new();
 }
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile { hunks, .. } => {
                assert_eq!(hunks[0].context_hint.as_deref(), Some("fn hello()"));
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_source_line_tracking() {
        let input = "\
*** Begin Patch
*** Update File: f.txt
@@
-old
+new
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile { hunks, .. } => {
                // @@ is on line 3 (1-indexed)
                assert_eq!(hunks[0].source_line, 3);
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_no_context_hint_gives_none() {
        let input = "\
*** Begin Patch
*** Update File: f.txt
@@
-old
+new
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile { hunks, .. } => {
                assert!(hunks[0].context_hint.is_none());
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_no_eof_marker_gives_false() {
        let input = "\
*** Begin Patch
*** Update File: f.txt
@@
-old
+new
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile { hunks, .. } => {
                assert!(!hunks[0].is_end_of_file);
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_update_without_move_to() {
        let input = "\
*** Begin Patch
*** Update File: f.txt
@@
-old
+new
*** End Patch";
        let patch = parse_patch(input).unwrap();
        match &patch.operations[0] {
            PatchOperation::UpdateFile { move_to, .. } => {
                assert!(move_to.is_none());
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }
}
