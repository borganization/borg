use anyhow::{bail, Context, Result};
use std::path::Path;
use tracing::{debug, info};

use crate::parser::{Hunk, Patch, PatchOperation};

fn validate_patch_path(path: &str, base_dir: &Path) -> Result<()> {
    let full = base_dir.join(path);
    let canonical_base = std::fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());
    // For new files, canonicalize as far as possible then check prefix
    let resolved = if full.exists() {
        std::fs::canonicalize(&full)?
    } else {
        // Walk up to find an existing ancestor, then append the rest
        let mut existing = full;
        let mut tail = Vec::new();
        while !existing.exists() {
            if let Some(file_name) = existing.file_name() {
                tail.push(file_name.to_os_string());
            } else {
                break;
            }
            if let Some(parent) = existing.parent() {
                existing = parent.to_path_buf();
            } else {
                break;
            }
        }
        let mut resolved = if existing.exists() {
            std::fs::canonicalize(&existing)?
        } else {
            existing
        };
        for component in tail.into_iter().rev() {
            resolved = resolved.join(component);
        }
        resolved
    };
    if !resolved.starts_with(&canonical_base) {
        bail!("Path traversal detected: '{path}' escapes base directory");
    }
    Ok(())
}

pub fn apply_patch(patch: &Patch, base_dir: &Path) -> Result<Vec<String>> {
    let mut affected_files = Vec::new();

    for op in &patch.operations {
        match op {
            PatchOperation::AddFile { path, content } => {
                validate_patch_path(path, base_dir)?;
                let full_path = base_dir.join(path);
                if let Some(parent) = full_path.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("Failed to create directory for {path}"))?;
                }
                std::fs::write(&full_path, content)
                    .with_context(|| format!("Failed to write {path}"))?;
                info!("Added file: {path}");
                affected_files.push(path.clone());
            }
            PatchOperation::UpdateFile { path, hunks } => {
                validate_patch_path(path, base_dir)?;
                let full_path = base_dir.join(path);
                if !full_path.exists() {
                    bail!("Cannot update non-existent file: {path}");
                }

                let mut content = std::fs::read_to_string(&full_path)
                    .with_context(|| format!("Failed to read {path}"))?;

                for hunk in hunks {
                    content = apply_hunk(&content, hunk)
                        .with_context(|| format!("Failed to apply hunk to {path}"))?;
                }

                std::fs::write(&full_path, &content)
                    .with_context(|| format!("Failed to write {path}"))?;
                info!("Updated file: {path}");
                affected_files.push(path.clone());
            }
            PatchOperation::DeleteFile { path } => {
                validate_patch_path(path, base_dir)?;
                let full_path = base_dir.join(path);
                if full_path.exists() {
                    std::fs::remove_file(&full_path)
                        .with_context(|| format!("Failed to delete {path}"))?;
                    info!("Deleted file: {path}");
                } else {
                    debug!("File already absent: {path}");
                }
                affected_files.push(path.clone());
            }
        }
    }

    Ok(affected_files)
}

fn apply_hunk(content: &str, hunk: &Hunk) -> Result<String> {
    if hunk.search.is_empty() {
        // Append mode
        return Ok(format!("{content}\n{}", hunk.replace));
    }

    // Try exact match first
    if let Some(pos) = content.find(&hunk.search) {
        let mut result = String::with_capacity(content.len());
        result.push_str(&content[..pos]);
        result.push_str(&hunk.replace);
        result.push_str(&content[pos + hunk.search.len()..]);
        return Ok(result);
    }

    // Try whitespace-normalized match
    let content_lines: Vec<&str> = content.lines().collect();
    let search_lines: Vec<&str> = hunk.search.lines().collect();

    for start in 0..content_lines.len() {
        if start + search_lines.len() > content_lines.len() {
            break;
        }

        let window = &content_lines[start..start + search_lines.len()];
        let window_normalized: Vec<String> =
            window.iter().map(|l| normalize_whitespace(l)).collect();
        let search_normalized: Vec<String> = search_lines
            .iter()
            .map(|l| normalize_whitespace(l))
            .collect();

        if window_normalized == search_normalized {
            let mut result_lines: Vec<&str> = Vec::new();
            result_lines.extend(&content_lines[..start]);
            for line in hunk.replace.lines() {
                result_lines.push(line);
            }
            result_lines.extend(&content_lines[start + search_lines.len()..]);
            return Ok(result_lines.join("\n"));
        }
    }

    bail!(
        "Could not find search text in file:\n---\n{}\n---",
        hunk.search
    )
}

fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Hunk, Patch, PatchOperation};
    use tempfile::TempDir;

    fn make_patch(ops: Vec<PatchOperation>) -> Patch {
        Patch { operations: ops }
    }

    #[test]
    fn add_new_file() {
        let dir = TempDir::new().unwrap();
        let patch = make_patch(vec![PatchOperation::AddFile {
            path: "sub/dir/hello.txt".to_string(),
            content: "hello world".to_string(),
        }]);
        let affected = apply_patch(&patch, dir.path()).unwrap();
        assert_eq!(affected, vec!["sub/dir/hello.txt"]);
        let content = std::fs::read_to_string(dir.path().join("sub/dir/hello.txt")).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn delete_existing_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("doomed.txt");
        std::fs::write(&file_path, "bye").unwrap();
        assert!(file_path.exists());

        let patch = make_patch(vec![PatchOperation::DeleteFile {
            path: "doomed.txt".to_string(),
        }]);
        let affected = apply_patch(&patch, dir.path()).unwrap();
        assert_eq!(affected, vec!["doomed.txt"]);
        assert!(!file_path.exists());
    }

    #[test]
    fn delete_nonexistent_file_succeeds() {
        let dir = TempDir::new().unwrap();
        let patch = make_patch(vec![PatchOperation::DeleteFile {
            path: "ghost.txt".to_string(),
        }]);
        // Should not error — the code treats missing files as already absent
        let affected = apply_patch(&patch, dir.path()).unwrap();
        assert_eq!(affected, vec!["ghost.txt"]);
    }

    #[test]
    fn update_file_exact_match() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("main.rs");
        std::fs::write(&file_path, "fn main() {\n    old();\n}\n").unwrap();

        let patch = make_patch(vec![PatchOperation::UpdateFile {
            path: "main.rs".to_string(),
            hunks: vec![Hunk {
                search: "    old();".to_string(),
                replace: "    new();".to_string(),
            }],
        }]);
        apply_patch(&patch, dir.path()).unwrap();
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("new();"));
        assert!(!content.contains("old();"));
    }

    #[test]
    fn update_file_whitespace_normalized_match() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("ws.rs");
        // File has extra spaces/tabs
        std::fs::write(&file_path, "fn  run() {\n\t   do_thing() ;\n}\n").unwrap();

        let patch = make_patch(vec![PatchOperation::UpdateFile {
            path: "ws.rs".to_string(),
            hunks: vec![Hunk {
                search: "fn run() {\n do_thing() ;".to_string(),
                replace: "fn run() {\n    do_thing();".to_string(),
            }],
        }]);
        apply_patch(&patch, dir.path()).unwrap();
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("    do_thing();"));
    }

    #[test]
    fn update_file_search_not_found_errors() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("nope.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();

        let patch = make_patch(vec![PatchOperation::UpdateFile {
            path: "nope.rs".to_string(),
            hunks: vec![Hunk {
                search: "this text does not exist anywhere".to_string(),
                replace: "replacement".to_string(),
            }],
        }]);
        let result = apply_patch(&patch, dir.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Could not find search text") || msg.contains("Failed to apply hunk"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn update_nonexistent_file_errors() {
        let dir = TempDir::new().unwrap();
        let patch = make_patch(vec![PatchOperation::UpdateFile {
            path: "missing.rs".to_string(),
            hunks: vec![],
        }]);
        let result = apply_patch(&patch, dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-existent"));
    }

    #[test]
    fn add_file_path_traversal_blocked() {
        let dir = TempDir::new().unwrap();
        let patch = make_patch(vec![PatchOperation::AddFile {
            path: "../../etc/evil.txt".to_string(),
            content: "malicious".to_string(),
        }]);
        let result = apply_patch(&patch, dir.path());
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("Path traversal"),
            "expected path traversal error"
        );
    }

    #[test]
    fn update_file_path_traversal_blocked() {
        let dir = TempDir::new().unwrap();
        let patch = make_patch(vec![PatchOperation::UpdateFile {
            path: "../../../etc/passwd".to_string(),
            hunks: vec![],
        }]);
        let result = apply_patch(&patch, dir.path());
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("Path traversal"),
            "expected path traversal error"
        );
    }

    #[test]
    fn delete_file_path_traversal_blocked() {
        let dir = TempDir::new().unwrap();
        let patch = make_patch(vec![PatchOperation::DeleteFile {
            path: "../../dangerous.txt".to_string(),
        }]);
        let result = apply_patch(&patch, dir.path());
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("Path traversal"),
            "expected path traversal error"
        );
    }

    /// End-to-end test: parse + apply a patch that creates a file whose content
    /// contains embedded patch DSL markers (e.g., a README documenting the DSL).
    /// With '+' prefix encoding, embedded markers are unambiguous.
    #[test]
    fn end_to_end_add_file_with_embedded_patch_markers() {
        let dir = TempDir::new().unwrap();
        let patch_text = "\
*** Begin Patch
*** Add File: README.md
+# My Project
+
+## Patch DSL
+
+Example:
+
+*** Begin Patch
+*** Add File: tool-name/tool.toml
++content here
+*** End Patch
+
+That's it.
*** End Patch";

        let affected = crate::apply_patch_to_dir(patch_text, dir.path()).unwrap();

        // Should create exactly one file
        assert_eq!(affected, vec!["README.md"]);
        assert!(dir.path().join("README.md").exists());

        // Should NOT create spurious files from embedded patch markers
        assert!(
            !dir.path().join("tool-name").exists(),
            "Should not create tool-name/ directory from embedded patch example"
        );

        // Content should be complete and include the embedded markers
        let content = std::fs::read_to_string(dir.path().join("README.md")).unwrap();
        assert!(content.contains("# My Project"));
        assert!(content.contains("*** Begin Patch"));
        assert!(content.contains("*** Add File: tool-name/tool.toml"));
        assert!(content.contains("That's it."));
    }

    /// End-to-end: parse + apply a full patch with add, update, and delete.
    #[test]
    fn end_to_end_full_patch() {
        let dir = TempDir::new().unwrap();

        // Pre-create a file to update and one to delete
        std::fs::write(dir.path().join("update.txt"), "foo\nbar\nbaz\n").unwrap();
        std::fs::write(dir.path().join("delete.txt"), "gone").unwrap();

        let patch_text = "\
*** Begin Patch
*** Add File: new.txt
+hello world
*** Update File: update.txt
@@
 foo
-bar
+BAR
*** Delete File: delete.txt
*** End Patch";

        let affected = crate::apply_patch_to_dir(patch_text, dir.path()).unwrap();
        assert_eq!(affected.len(), 3);

        // New file created
        let content = std::fs::read_to_string(dir.path().join("new.txt")).unwrap();
        assert_eq!(content, "hello world");

        // File updated
        let content = std::fs::read_to_string(dir.path().join("update.txt")).unwrap();
        assert!(content.contains("BAR"));
        assert!(!content.contains("\nbar\n"));

        // File deleted
        assert!(!dir.path().join("delete.txt").exists());
    }
}
