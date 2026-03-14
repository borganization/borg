use anyhow::{bail, Result};
use std::path::PathBuf;
use tracing::debug;

use crate::config::Config;
use crate::conversation::estimate_tokens;

pub fn memory_dir() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("memory"))
}

pub fn memory_index_path() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("MEMORY.md"))
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
    if mem_dir.exists() {
        let mut entries: Vec<_> = std::fs::read_dir(&mem_dir)?
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

            if estimated_tokens + tokens > max_tokens {
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
            parts.push(format!("## Memory: {filename}\n\n{content}"));
            estimated_tokens += tokens;
            debug!("Loaded memory/{}.md ({tokens} estimated tokens)", filename);
        }
    }

    if parts.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("# Your Memory\n\n{}\n", parts.join("\n\n---\n\n")))
    }
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

pub fn write_memory(filename: &str, content: &str, append: bool) -> Result<String> {
    validate_memory_filename(filename)?;

    let path = if filename == "SOUL.md" {
        Config::data_dir()?.join("SOUL.md")
    } else if filename == "MEMORY.md" {
        memory_index_path()?
    } else {
        let dir = memory_dir()?;
        std::fs::create_dir_all(&dir)?;
        dir.join(filename)
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
    validate_memory_filename(filename)?;

    let path = if filename == "SOUL.md" {
        Config::data_dir()?.join("SOUL.md")
    } else if filename == "MEMORY.md" {
        memory_index_path()?
    } else {
        memory_dir()?.join(filename)
    };

    if path.exists() {
        Ok(std::fs::read_to_string(&path)?)
    } else {
        Ok(format!("Memory file '{filename}' not found."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_calculation() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1); // 4 chars = 1 token
        assert_eq!(estimate_tokens("abcdefgh"), 2); // 8 chars = 2 tokens
        assert_eq!(estimate_tokens("ab"), 0); // 2 chars = 0 tokens (integer division)
    }

    #[test]
    fn estimate_tokens_longer_text() {
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
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
        // SOUL.md should resolve to data_dir/SOUL.md
        // Just test that the function doesn't panic
        let _result = read_memory("SOUL.md");
        let _result = read_memory("MEMORY.md");
    }

    #[test]
    fn load_memory_context_empty_returns_empty() {
        // With a very small budget, we might get empty (or content if ~/.tamagotchi exists)
        let result = load_memory_context(0);
        assert!(result.is_ok());
        // Budget of 0 means nothing should be loaded
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
