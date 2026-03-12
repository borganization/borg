use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Invalid patch format: {0}")]
    InvalidFormat(String),
    #[error("Unexpected end of patch")]
    UnexpectedEnd,
}

#[derive(Debug, Clone)]
pub enum PatchOperation {
    AddFile { path: String, content: String },
    UpdateFile { path: String, hunks: Vec<Hunk> },
    DeleteFile { path: String },
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub search: String,
    pub replace: String,
}

#[derive(Debug)]
pub struct Patch {
    pub operations: Vec<PatchOperation>,
}

pub fn parse_patch(input: &str) -> Result<Patch, ParseError> {
    let lines: Vec<&str> = input.lines().collect();
    let mut operations = Vec::new();
    let mut i = 0;

    // Skip leading whitespace/empty lines
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }

    // Expect "*** Begin Patch" (optional — be lenient)
    if i < lines.len() && lines[i].trim() == "*** Begin Patch" {
        i += 1;
    }

    while i < lines.len() {
        let line = lines[i].trim();

        if line == "*** End Patch" || line.is_empty() {
            i += 1;
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Add File: ") {
            let path = path.trim().to_string();
            i += 1;
            let mut content = String::new();
            while i < lines.len() && !lines[i].trim().starts_with("*** ") {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(lines[i]);
                i += 1;
            }
            operations.push(PatchOperation::AddFile { path, content });
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            let path = path.trim().to_string();
            i += 1;
            let mut hunks = Vec::new();

            while i < lines.len() && !lines[i].trim().starts_with("*** ") {
                let hunk_line = lines[i].trim();

                if hunk_line.starts_with("@@") {
                    // Parse unified diff style hunk header — skip it
                    i += 1;
                    let mut search_lines = Vec::new();
                    let mut replace_lines = Vec::new();

                    while i < lines.len()
                        && !lines[i].trim().starts_with("@@")
                        && !lines[i].trim().starts_with("*** ")
                    {
                        let l = lines[i];
                        if let Some(removed) = l.strip_prefix('-') {
                            search_lines.push(removed);
                        } else if let Some(added) = l.strip_prefix('+') {
                            replace_lines.push(added);
                        } else if let Some(context) = l.strip_prefix(' ') {
                            search_lines.push(context);
                            replace_lines.push(context);
                        } else {
                            search_lines.push(l);
                            replace_lines.push(l);
                        }
                        i += 1;
                    }

                    hunks.push(Hunk {
                        search: search_lines.join("\n"),
                        replace: replace_lines.join("\n"),
                    });
                } else {
                    i += 1;
                }
            }

            operations.push(PatchOperation::UpdateFile { path, hunks });
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
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

    #[test]
    fn parse_add_file() {
        let input = "\
*** Begin Patch
*** Add File: src/new.rs
fn main() {}
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

    #[test]
    fn parse_update_file_with_hunks() {
        let input = "\
*** Begin Patch
*** Update File: src/lib.rs
@@ -1,3 +1,3 @@
 fn hello() {
-    println!(\"hi\");
+    println!(\"hello world\");
 }
*** End Patch";
        let patch = parse_patch(input).unwrap();
        assert_eq!(patch.operations.len(), 1);
        match &patch.operations[0] {
            PatchOperation::UpdateFile { path, hunks } => {
                assert_eq!(path, "src/lib.rs");
                assert_eq!(hunks.len(), 1);
                assert!(hunks[0].search.contains("println!(\"hi\");"));
                assert!(hunks[0].replace.contains("println!(\"hello world\");"));
                // Context lines should appear in both search and replace
                assert!(hunks[0].search.contains("fn hello()"));
                assert!(hunks[0].replace.contains("fn hello()"));
            }
            other => panic!("Expected UpdateFile, got {:?}", other),
        }
    }

    #[test]
    fn parse_multiple_operations() {
        let input = "\
*** Begin Patch
*** Add File: a.txt
hello
*** Delete File: b.txt
*** Update File: c.txt
@@ -1,1 +1,1 @@
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
}
