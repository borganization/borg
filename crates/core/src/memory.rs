use anyhow::{bail, Result};
use std::path::PathBuf;
use tracing::{debug, instrument};

use crate::config::Config;
use crate::tokenizer::estimate_tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Overwrite,
    Append,
}

pub fn memory_dir() -> Result<PathBuf> {
    Config::memory_dir()
}

pub fn memory_index_path() -> Result<PathBuf> {
    Config::memory_index_path()
}

#[instrument(skip_all, fields(token_budget = max_tokens))]
pub fn load_memory_context(max_tokens: usize) -> Result<String> {
    load_memory_context_scoped(max_tokens, None)
}

/// Load memory context, optionally from a scoped subdirectory.
/// When `scope` is Some, loads from `~/.borg/memory/scopes/{scope}/` instead of `~/.borg/memory/`.
/// Global MEMORY.md is always loaded regardless of scope.
#[instrument(skip_all, fields(token_budget = max_tokens, scope = ?scope))]
pub fn load_memory_context_scoped(max_tokens: usize, scope: Option<&str>) -> Result<String> {
    let mut parts = Vec::new();
    let mut estimated_tokens = 0;

    // Load MEMORY.md first (always global)
    let index_path = memory_index_path()?;
    if index_path.exists() {
        let content = std::fs::read_to_string(&index_path)?;
        let tokens = estimate_tokens(&content);
        if tokens < max_tokens {
            parts.push(format!(
                "<memory_file name=\"MEMORY.md\">\n{content}\n</memory_file>"
            ));
            estimated_tokens += tokens;
            debug!("Loaded MEMORY.md ({tokens} estimated tokens)");
        }
    }

    // Load memory files — from scoped dir if scope is set, otherwise from default memory dir
    let mem_dir = if let Some(scope_name) = scope {
        // Validate scope name to prevent path traversal
        if scope_name.contains("..") || scope_name.contains('/') || scope_name.contains('\\') {
            bail!("Invalid memory scope name: must not contain path separators or '..'");
        }
        let scoped_dir = Config::data_dir()?
            .join("memory")
            .join("scopes")
            .join(scope_name);
        if !scoped_dir.exists() {
            std::fs::create_dir_all(&scoped_dir)?;
        }
        scoped_dir
    } else {
        memory_dir()?
    };
    let label = if scope.is_some() {
        "Scoped Memory"
    } else {
        "Memory"
    };
    load_memory_files_from_dir(
        &mem_dir,
        label,
        max_tokens,
        &mut estimated_tokens,
        &mut parts,
    )?;

    // Load local project memory from CWD/.borg/memory/ if it exists
    if let Ok(cwd) = std::env::current_dir() {
        let local_mem_dir = cwd.join(".borg").join("memory");
        if local_mem_dir.exists() {
            // Also load local MEMORY.md if present
            let local_index = cwd.join(".borg").join("MEMORY.md");
            if local_index.exists() {
                let content = std::fs::read_to_string(&local_index)?;
                let tokens = estimate_tokens(&content);
                if estimated_tokens + tokens <= max_tokens {
                    parts.push(format!(
                        "<memory_file name=\"Local MEMORY.md\">\n{content}\n</memory_file>"
                    ));
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
        Ok(parts.join("\n\n"))
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
        parts.push(format!(
            "<memory_file name=\"{label}: {filename}\">\n{content}\n</memory_file>"
        ));
        *estimated_tokens += tokens;
        debug!(
            "Loaded {}/{}.md ({tokens} estimated tokens)",
            dir.display(),
            filename
        );
    }

    Ok(())
}

/// Load memory context with files ordered by semantic ranking.
/// Files in `ranked_global`/`ranked_local` are loaded in that order.
/// Files not present in rankings are appended at the end in mtime order.
#[instrument(skip_all, fields(token_budget = max_tokens))]
pub fn load_memory_context_ranked(
    max_tokens: usize,
    ranked_global: &[(String, f32)],
    ranked_local: &[(String, f32)],
) -> Result<String> {
    let mut parts = Vec::new();
    let mut estimated_tokens = 0;

    // Always load MEMORY.md first (global)
    let index_path = memory_index_path()?;
    if index_path.exists() {
        let content = std::fs::read_to_string(&index_path)?;
        let tokens = estimate_tokens(&content);
        if tokens < max_tokens {
            parts.push(format!(
                "<memory_file name=\"MEMORY.md\">\n{content}\n</memory_file>"
            ));
            estimated_tokens += tokens;
            debug!("Loaded MEMORY.md ({tokens} estimated tokens)");
        }
    }

    // Load global memory/*.md in ranked order
    let mem_dir = memory_dir()?;
    load_memory_files_ranked(
        &mem_dir,
        "Memory",
        ranked_global,
        max_tokens,
        &mut estimated_tokens,
        &mut parts,
    )?;

    // Load local project memory
    if let Ok(cwd) = std::env::current_dir() {
        let local_mem_dir = cwd.join(".borg").join("memory");
        if local_mem_dir.exists() {
            // Also load local MEMORY.md if present
            let local_index = cwd.join(".borg").join("MEMORY.md");
            if local_index.exists() {
                let content = std::fs::read_to_string(&local_index)?;
                let tokens = estimate_tokens(&content);
                if estimated_tokens + tokens <= max_tokens {
                    parts.push(format!(
                        "<memory_file name=\"Local MEMORY.md\">\n{content}\n</memory_file>"
                    ));
                    estimated_tokens += tokens;
                    debug!("Loaded local MEMORY.md ({tokens} estimated tokens)");
                }
            }
            load_memory_files_ranked(
                &local_mem_dir,
                "Local Memory",
                ranked_local,
                max_tokens,
                &mut estimated_tokens,
                &mut parts,
            )?;
        }
    }

    if parts.is_empty() {
        Ok(String::new())
    } else {
        Ok(parts.join("\n\n"))
    }
}

/// Load memory files in ranked order, appending unranked files by mtime at the end.
fn load_memory_files_ranked(
    dir: &std::path::Path,
    label: &str,
    rankings: &[(String, f32)],
    max_tokens: usize,
    estimated_tokens: &mut usize,
    parts: &mut Vec<String>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    // Collect all .md files in directory
    let all_files: std::collections::HashSet<String> = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().map(|ext| ext == "md").unwrap_or(false))
        .filter_map(|e| {
            e.path()
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
        })
        .collect();

    // Track which files we've loaded
    let mut loaded = std::collections::HashSet::new();

    // First: load ranked files in order
    for (filename, _score) in rankings {
        if !all_files.contains(filename) {
            continue;
        }
        let path = dir.join(filename);
        if !path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        let tokens = estimate_tokens(&content);
        if *estimated_tokens + tokens > max_tokens {
            debug!("Skipping {} (would exceed token budget)", path.display());
            continue;
        }
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        parts.push(format!(
            "<memory_file name=\"{label}: {stem}\">\n{content}\n</memory_file>"
        ));
        *estimated_tokens += tokens;
        loaded.insert(filename.clone());
        debug!(
            "Loaded {}/{filename} ({tokens} estimated tokens, ranked)",
            dir.display()
        );
    }

    // Second: load unranked files by mtime (most recent first)
    let mut unranked: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            e.path().extension().map(|ext| ext == "md").unwrap_or(false)
                && !loaded.contains(
                    &e.path()
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                )
        })
        .collect();
    unranked.sort_by(|a, b| {
        let time_a = a.metadata().and_then(|m| m.modified()).ok();
        let time_b = b.metadata().and_then(|m| m.modified()).ok();
        time_b.cmp(&time_a)
    });

    for entry in unranked {
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
        parts.push(format!(
            "<memory_file name=\"{label}: {filename}\">\n{content}\n</memory_file>"
        ));
        *estimated_tokens += tokens;
        debug!(
            "Loaded {}/{}.md ({tokens} estimated tokens, unranked)",
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
        "IDENTITY.md" => Config::identity_path(),
        "MEMORY.md" => memory_index_path(),
        _ => Ok(memory_dir()?.join(filename)),
    }
}

pub fn write_memory(filename: &str, content: &str, mode: WriteMode) -> Result<String> {
    write_memory_scoped(filename, content, mode, "global")
}

pub fn write_memory_scoped(
    filename: &str,
    content: &str,
    mode: WriteMode,
    scope: &str,
) -> Result<String> {
    let path = if scope == "local" {
        let cwd = std::env::current_dir()?;
        let local_dir = cwd.join(".borg");
        validate_memory_filename(filename)?;
        match filename {
            "MEMORY.md" => local_dir.join("MEMORY.md"),
            _ => local_dir.join("memory").join(filename),
        }
    } else {
        resolve_memory_path(filename)?
    };

    if mode == WriteMode::Append && path.exists() {
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
        let result = write_memory("_test_topic_12345.md", "test content", WriteMode::Overwrite);
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
        write_memory(filename, "line1", WriteMode::Overwrite).unwrap();

        // Append
        write_memory(filename, "line2", WriteMode::Append).unwrap();

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

        write_memory(filename, "original", WriteMode::Overwrite).unwrap();
        write_memory(filename, "replaced", WriteMode::Overwrite).unwrap();

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
        let _result = read_memory("IDENTITY.md");
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
        assert!(dir.to_string_lossy().contains(".borg"));
        assert!(dir.to_string_lossy().ends_with("memory"));
    }

    #[test]
    fn memory_index_path_check() {
        let path = memory_index_path().unwrap();
        assert!(path.to_string_lossy().contains("MEMORY.md"));
    }

    #[test]
    fn resolve_memory_path_identity() {
        let path = resolve_memory_path("IDENTITY.md").unwrap();
        assert!(path.to_string_lossy().contains(".borg"));
        assert!(path.to_string_lossy().ends_with("IDENTITY.md"));
        // Should NOT be inside memory/ subdirectory
        assert!(!path.to_string_lossy().contains("memory/IDENTITY.md"));
    }

    #[test]
    fn resolve_memory_path_memory_index() {
        let path = resolve_memory_path("MEMORY.md").unwrap();
        assert!(path.to_string_lossy().contains(".borg"));
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
    fn test_memory_wraps_in_xml_tags() {
        // Verify the memory format uses XML tags
        let filename = "_test_xml_wrap_12345.md";
        write_memory(filename, "test xml content", WriteMode::Overwrite).unwrap();

        let result = load_memory_context(100_000);
        if let Ok(ctx) = result {
            if ctx.contains("test xml content") {
                assert!(
                    ctx.contains("<memory_file name="),
                    "Memory context should use XML tags"
                );
                assert!(
                    ctx.contains("</memory_file>"),
                    "Memory context should close XML tags"
                );
            }
        }

        // Cleanup
        let path = memory_dir().unwrap().join(filename);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validate_accepts_simple_filenames() {
        assert!(validate_memory_filename("IDENTITY.md").is_ok());
        assert!(validate_memory_filename("MEMORY.md").is_ok());
        assert!(validate_memory_filename("notes.md").is_ok());
        assert!(validate_memory_filename("my-topic.md").is_ok());
    }

    #[test]
    fn load_memory_files_ranked_respects_order() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        std::fs::write(dir.join("a.md"), "content_a").unwrap();
        std::fs::write(dir.join("b.md"), "content_b").unwrap();
        std::fs::write(dir.join("c.md"), "content_c").unwrap();

        // Rank: c first, then a, then b
        let rankings = vec![
            ("c.md".to_string(), 0.9),
            ("a.md".to_string(), 0.5),
            ("b.md".to_string(), 0.3),
        ];

        let mut parts = Vec::new();
        let mut tokens = 0;
        load_memory_files_ranked(dir, "Test", &rankings, 100_000, &mut tokens, &mut parts).unwrap();

        let combined = parts.join("\n");
        let pos_c = combined.find("content_c").unwrap();
        let pos_a = combined.find("content_a").unwrap();
        let pos_b = combined.find("content_b").unwrap();
        assert!(
            pos_c < pos_a,
            "c (rank 0.9) should appear before a (rank 0.5)"
        );
        assert!(
            pos_a < pos_b,
            "a (rank 0.5) should appear before b (rank 0.3)"
        );
    }

    #[test]
    fn load_memory_files_ranked_includes_unranked() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        std::fs::write(dir.join("ranked.md"), "ranked_content").unwrap();
        std::fs::write(dir.join("unranked.md"), "unranked_content").unwrap();

        let rankings = vec![("ranked.md".to_string(), 1.0)];

        let mut parts = Vec::new();
        let mut tokens = 0;
        load_memory_files_ranked(dir, "Test", &rankings, 100_000, &mut tokens, &mut parts).unwrap();

        let combined = parts.join("\n");
        assert!(combined.contains("ranked_content"));
        assert!(
            combined.contains("unranked_content"),
            "Unranked files should be appended"
        );

        // Ranked should appear before unranked
        let pos_r = combined.find("ranked_content").unwrap();
        let pos_u = combined.find("unranked_content").unwrap();
        assert!(pos_r < pos_u);
    }

    #[test]
    fn load_memory_files_ranked_respects_token_budget() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let big_content = "budgetword ".repeat(5000);
        std::fs::write(dir.join("big.md"), &big_content).unwrap();
        std::fs::write(dir.join("small.md"), "tiny").unwrap();

        let rankings = vec![("big.md".to_string(), 1.0), ("small.md".to_string(), 0.5)];

        let mut parts = Vec::new();
        let mut tokens = 0;
        // Budget of 50 tokens — big.md should be skipped, small.md should fit
        load_memory_files_ranked(dir, "Test", &rankings, 50, &mut tokens, &mut parts).unwrap();

        let combined = parts.join("\n");
        assert!(
            !combined.contains("budgetword"),
            "Big file exceeding budget should be skipped"
        );
        assert!(combined.contains("tiny"), "Small file should fit in budget");
    }
}
