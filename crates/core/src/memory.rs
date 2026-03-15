use anyhow::{bail, Result};
use std::path::PathBuf;
use tracing::debug;

use crate::config::Config;
use crate::tokenizer::estimate_tokens;

pub fn memory_dir() -> Result<PathBuf> {
    Config::memory_dir()
}

pub fn memory_index_path() -> Result<PathBuf> {
    Config::memory_index_path()
}

pub fn load_memory_context(max_tokens: usize) -> Result<String> {
    let mut parts = Vec::new();
    let mut estimated_tokens = 0;

    // Load MEMORY.md first
    let index_path = memory_index_path()?;
    if index_path.exists() {
        let content = std::fs::read_to_string(&index_path)?;
        let tokens = estimate_tokens(&content);
        if tokens < max_tokens {
            parts.push(format!("## MEMORY.md\n\n{content}"));
            estimated_tokens += tokens;
            debug!("Loaded MEMORY.md ({tokens} estimated tokens)");
        }
    }

    // Load memory/*.md files, sorted by modification time (most recent first)
    let mem_dir = memory_dir()?;
    load_memory_files_from_dir(
        &mem_dir,
        "Memory",
        max_tokens,
        &mut estimated_tokens,
        &mut parts,
    )?;

    // Load local project memory from CWD/.tamagotchi/memory/ if it exists
    if let Ok(cwd) = std::env::current_dir() {
        let local_mem_dir = cwd.join(".tamagotchi").join("memory");
        if local_mem_dir.exists() {
            // Also load local MEMORY.md if present
            let local_index = cwd.join(".tamagotchi").join("MEMORY.md");
            if local_index.exists() {
                let content = std::fs::read_to_string(&local_index)?;
                let tokens = estimate_tokens(&content);
                if estimated_tokens + tokens <= max_tokens {
                    parts.push(format!("## Local MEMORY.md\n\n{content}"));
                    estimated_tokens += tokens;
                    debug!("Loaded local MEMORY.md ({tokens} estimated tokens)");
                }
            }
            load_memory_files_from_dir(
                &local_mem_dir,
                "Local Memory",
                max_tokens,
                &mut estimated_tokens,
                &mut parts,
            )?;
        }
    }

    if parts.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("# Your Memory\n\n{}\n", parts.join("\n\n---\n\n")))
    }
}

fn load_memory_files_from_dir(
    dir: &std::path::Path,
    label: &str,
    max_tokens: usize,
    estimated_tokens: &mut usize,
    parts: &mut Vec<String>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().map(|ext| ext == "md").unwrap_or(false))
        .collect();

    entries.sort_by(|a, b| {
        let time_a = a.metadata().and_then(|m| m.modified()).ok();
        let time_b = b.metadata().and_then(|m| m.modified()).ok();
        time_b.cmp(&time_a)
    });

    for entry in entries {
        let content = std::fs::read_to_string(entry.path())?;
        let tokens = estimate_tokens(&content);

        if *estimated_tokens + tokens > max_tokens {
            debug!(
                "Skipping {} (would exceed token budget)",
                entry.path().display()
            );
            continue;
        }

        let filename = entry
            .path()
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        parts.push(format!("## {label}: {filename}\n\n{content}"));
        *estimated_tokens += tokens;
        debug!(
            "Loaded {}/{}.md ({tokens} estimated tokens)",
            dir.display(),
            filename
        );
    }

    Ok(())
}

fn validate_memory_filename(filename: &str) -> Result<()> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        bail!("Invalid memory filename: must not contain path separators or '..'");
    }
    if filename.is_empty() {
        bail!("Memory filename must not be empty");
    }
    Ok(())
}

fn resolve_memory_path(filename: &str) -> Result<PathBuf> {
    validate_memory_filename(filename)?;
    match filename {
        "SOUL.md" => Config::soul_path(),
        "MEMORY.md" => memory_index_path(),
        _ => Ok(memory_dir()?.join(filename)),
    }
}

pub fn write_memory(filename: &str, content: &str, append: bool) -> Result<String> {
    write_memory_scoped(filename, content, append, "global")
}

pub fn write_memory_scoped(
    filename: &str,
    content: &str,
    append: bool,
    scope: &str,
) -> Result<String> {
    let path = if scope == "local" {
        let cwd = std::env::current_dir()?;
        let local_dir = cwd.join(".tamagotchi");
        validate_memory_filename(filename)?;
        match filename {
            "MEMORY.md" => local_dir.join("MEMORY.md"),
            _ => local_dir.join("memory").join(filename),
        }
    } else {
        resolve_memory_path(filename)?
    };

    if append && path.exists() {
        let existing = std::fs::read_to_string(&path)?;
        std::fs::write(&path, format!("{existing}\n{content}"))?;
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
    }

    Ok(format!("Written to {}", path.display()))
}

pub fn read_memory(filename: &str) -> Result<String> {
    let path = resolve_memory_path(filename)?;

    if path.exists() {
        Ok(std::fs::read_to_string(&path)?)
    } else {
        Ok(format!("Memory file '{filename}' not found."))
    }
}

/// List all memory files with metadata for cleanup/management.
pub fn list_memory_files() -> Result<Vec<MemoryFileInfo>> {
    let mut files = Vec::new();
    let mem_dir = memory_dir()?;

    // Include MEMORY.md
    let index_path = memory_index_path()?;
    if index_path.exists() {
        let meta = std::fs::metadata(&index_path)?;
        let modified = meta
            .modified()
            .ok()
            .map(chrono::DateTime::<chrono::Local>::from);
        let size = meta.len();
        files.push(MemoryFileInfo {
            filename: "MEMORY.md".to_string(),
            size_bytes: size,
            modified_at: modified,
        });
    }

    // Include memory/*.md files
    if mem_dir.exists() {
        for entry in std::fs::read_dir(&mem_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let meta = entry.metadata()?;
                let modified = meta
                    .modified()
                    .ok()
                    .map(chrono::DateTime::<chrono::Local>::from);
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                files.push(MemoryFileInfo {
                    filename,
                    size_bytes: meta.len(),
                    modified_at: modified,
                });
            }
        }
    }

    // Sort by modified time (oldest first — candidates for cleanup)
    files.sort_by(|a, b| a.modified_at.cmp(&b.modified_at));
    Ok(files)
}

pub struct MemoryFileInfo {
    pub filename: String,
    pub size_bytes: u64,
    pub modified_at: Option<chrono::DateTime<chrono::Local>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_nonempty() {
        assert_eq!(estimate_tokens(""), 0);
        assert!(estimate_tokens("Hello, world!") > 0);
    }

    #[test]
    fn write_and_read_memory_file() {
        // Write to a topic file
        let result = write_memory("_test_topic_12345.md", "test content", false);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("Written to"));

        // Read it back
        let content = read_memory("_test_topic_12345.md").unwrap();
        assert_eq!(content, "test content");

        // Cleanup
        let path = memory_dir().unwrap().join("_test_topic_12345.md");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_memory_append_mode() {
        let filename = "_test_append_12345.md";

        // Write initial content
        write_memory(filename, "line1", false).unwrap();

        // Append
        write_memory(filename, "line2", true).unwrap();

        let content = read_memory(filename).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));

        // Cleanup
        let path = memory_dir().unwrap().join(filename);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_memory_overwrite_mode() {
        let filename = "_test_overwrite_12345.md";

        write_memory(filename, "original", false).unwrap();
        write_memory(filename, "replaced", false).unwrap();

        let content = read_memory(filename).unwrap();
        assert_eq!(content, "replaced");

        // Cleanup
        let path = memory_dir().unwrap().join(filename);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_memory_nonexistent_returns_not_found() {
        let content = read_memory("_definitely_nonexistent_xyz_12345.md").unwrap();
        assert!(content.contains("not found"));
    }

    #[test]
    fn read_memory_special_filenames() {
        let _result = read_memory("SOUL.md");
        let _result = read_memory("MEMORY.md");
    }

    #[test]
    fn load_memory_context_empty_returns_empty() {
        let result = load_memory_context(0);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn memory_dir_path() {
        let dir = memory_dir().unwrap();
        assert!(dir.to_string_lossy().contains(".tamagotchi"));
        assert!(dir.to_string_lossy().ends_with("memory"));
    }

    #[test]
    fn memory_index_path_check() {
        let path = memory_index_path().unwrap();
        assert!(path.to_string_lossy().contains("MEMORY.md"));
    }

    #[test]
    fn resolve_memory_path_soul() {
        let path = resolve_memory_path("SOUL.md").unwrap();
        assert!(path.to_string_lossy().contains(".tamagotchi"));
        assert!(path.to_string_lossy().ends_with("SOUL.md"));
        // Should NOT be inside memory/ subdirectory
        assert!(!path.to_string_lossy().contains("memory/SOUL.md"));
    }

    #[test]
    fn resolve_memory_path_memory_index() {
        let path = resolve_memory_path("MEMORY.md").unwrap();
        assert!(path.to_string_lossy().contains(".tamagotchi"));
        assert!(path.to_string_lossy().ends_with("MEMORY.md"));
        assert!(!path.to_string_lossy().contains("memory/MEMORY.md"));
    }

    #[test]
    fn resolve_memory_path_topic_file() {
        let path = resolve_memory_path("notes.md").unwrap();
        assert!(path.to_string_lossy().contains("memory/notes.md"));
    }

    #[test]
    fn resolve_memory_path_rejects_invalid() {
        assert!(resolve_memory_path("").is_err());
        assert!(resolve_memory_path("../etc/passwd").is_err());
        assert!(resolve_memory_path("sub/dir.md").is_err());
    }

    #[test]
    fn validate_rejects_path_traversal() {
        assert!(validate_memory_filename("../../etc/passwd").is_err());
        assert!(validate_memory_filename("../secret.md").is_err());
        assert!(validate_memory_filename("..").is_err());
    }

    #[test]
    fn validate_rejects_slashes() {
        assert!(validate_memory_filename("sub/dir/file.md").is_err());
        assert!(validate_memory_filename("sub\\dir\\file.md").is_err());
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_memory_filename("").is_err());
    }

    #[test]
    fn validate_accepts_simple_filenames() {
        assert!(validate_memory_filename("SOUL.md").is_ok());
        assert!(validate_memory_filename("MEMORY.md").is_ok());
        assert!(validate_memory_filename("notes.md").is_ok());
        assert!(validate_memory_filename("my-topic.md").is_ok());
    }
}
