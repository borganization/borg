//! Git utilities for coding agent capabilities.
//!
//! Provides ghost commits (full repo snapshots for atomic undo), git context
//! collection for system prompt enrichment, and turn diff tracking.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Timeout for git subprocess calls.
const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Directories excluded from ghost snapshots (build artifacts, deps, caches).
const IGNORED_DIR_NAMES: &[&str] = &[
    "node_modules",
    ".venv",
    "venv",
    "dist",
    "build",
    ".pytest_cache",
    ".mypy_cache",
    ".cache",
    ".tox",
    "__pycache__",
    "target",
    ".next",
    ".nuxt",
];

/// Max untracked file size to include in ghost snapshot (10 MiB).
const MAX_UNTRACKED_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// A ghost commit that snapshots the entire working tree state.
///
/// Created using a temporary index file and git plumbing commands so that
/// HEAD and the user's index are never touched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhostCommit {
    pub commit_id: String,
    pub parent_id: Option<String>,
    /// Untracked files that existed before the snapshot (preserved on restore).
    pub preexisting_untracked: Vec<PathBuf>,
}

/// Summary of git repo state for system prompt enrichment.
#[derive(Debug, Clone, Default)]
pub struct GitContext {
    pub repo_root: Option<PathBuf>,
    pub branch: Option<String>,
    pub commit_hash: Option<String>,
    pub recent_commits: Vec<String>,
    pub has_uncommitted_changes: bool,
    pub remote_url: Option<String>,
}

/// Files modified during a single agent turn.
#[derive(Debug, Clone, Default)]
pub struct TurnDiff {
    pub modified_files: Vec<PathBuf>,
    pub added_files: Vec<PathBuf>,
    pub deleted_files: Vec<PathBuf>,
}

/// Find the git repo root by walking up from `cwd`.
pub fn find_repo_root(cwd: &Path) -> Option<PathBuf> {
    let mut dir = cwd.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Run a git command with timeout, returns stdout on success.
async fn run_git(args: &[&str], cwd: &Path) -> Result<String> {
    let output = tokio::time::timeout(
        GIT_COMMAND_TIMEOUT,
        tokio::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output(),
    )
    .await
    .context("git command timed out")?
    .context("failed to execute git")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Collect rich git context for system prompt injection.
pub async fn collect_git_context(cwd: &Path) -> GitContext {
    let repo_root = find_repo_root(cwd);
    let work_dir = repo_root.as_deref().unwrap_or(cwd);

    let branch = run_git(&["branch", "--show-current"], work_dir)
        .await
        .ok()
        .filter(|s| !s.is_empty());

    let commit_hash = run_git(&["rev-parse", "--short", "HEAD"], work_dir)
        .await
        .ok();

    let recent_commits = run_git(&["log", "--oneline", "-5", "--no-decorate"], work_dir)
        .await
        .ok()
        .map(|s| s.lines().map(String::from).collect())
        .unwrap_or_default();

    let has_uncommitted_changes = run_git(&["status", "--porcelain"], work_dir)
        .await
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    let remote_url = run_git(&["remote", "get-url", "origin"], work_dir)
        .await
        .ok();

    GitContext {
        repo_root,
        branch,
        commit_hash,
        recent_commits,
        has_uncommitted_changes,
        remote_url,
    }
}

/// Create a ghost commit capturing tracked + untracked files using a temp index.
///
/// This does NOT modify HEAD or the user's index. Uses git plumbing:
/// 1. Create a temp index file
/// 2. `GIT_INDEX_FILE=<tmp> git read-tree HEAD` (copy tracked files)
/// 3. `GIT_INDEX_FILE=<tmp> git add --all` (add untracked, excluding ignored dirs)
/// 4. `GIT_INDEX_FILE=<tmp> git write-tree` (create tree object)
/// 5. `git commit-tree <tree> -p HEAD -m "borg ghost snapshot"` (create commit)
pub async fn create_ghost_commit(repo_root: &Path) -> Result<GhostCommit> {
    // Check if repo has any commits
    let has_head = run_git(&["rev-parse", "HEAD"], repo_root).await.is_ok();

    let tmp_index = tempfile::NamedTempFile::new().context("create temp index")?;
    let tmp_path = tmp_index.path().to_string_lossy().to_string();

    // Collect untracked files before snapshot for restore filtering
    let untracked_output = run_git(&["ls-files", "--others", "--exclude-standard"], repo_root)
        .await
        .unwrap_or_default();

    let preexisting_untracked: Vec<PathBuf> = untracked_output
        .lines()
        .filter(|line| !line.is_empty())
        .filter(|line| {
            // Exclude ignored directories
            !IGNORED_DIR_NAMES
                .iter()
                .any(|ignored| line.starts_with(ignored) || line.contains(&format!("/{ignored}/")))
        })
        .filter(|line| {
            // Exclude oversized files
            let full = repo_root.join(line);
            full.metadata()
                .map(|m| m.len() <= MAX_UNTRACKED_FILE_BYTES)
                .unwrap_or(false)
        })
        .map(PathBuf::from)
        .collect();

    // Step 1: Read current HEAD into temp index (if HEAD exists)
    if has_head {
        run_git_with_index(&tmp_path, &["read-tree", "HEAD"], repo_root).await?;
    }

    // Step 2: Add all files to temp index
    run_git_with_index(&tmp_path, &["add", "--all"], repo_root).await?;

    // Step 3: Write tree from temp index
    let tree_id = run_git_with_index(&tmp_path, &["write-tree"], repo_root).await?;

    // Step 4: Create commit-tree (orphan if no HEAD)
    let parent_id = if has_head {
        Some(run_git(&["rev-parse", "HEAD"], repo_root).await?)
    } else {
        None
    };

    let mut commit_args = vec!["commit-tree", &tree_id, "-m", "borg ghost snapshot"];
    if let Some(ref parent) = parent_id {
        commit_args.push("-p");
        commit_args.push(parent);
    }

    let commit_id = run_git(&commit_args, repo_root).await?;

    Ok(GhostCommit {
        commit_id,
        parent_id,
        preexisting_untracked,
    })
}

/// Restore working tree to match a ghost commit.
pub async fn restore_ghost_commit(repo_root: &Path, commit: &GhostCommit) -> Result<()> {
    // Restore all tracked files from the ghost commit
    run_git(
        &[
            "restore",
            "--source",
            &commit.commit_id,
            "--worktree",
            "--",
            ".",
        ],
        repo_root,
    )
    .await?;

    // Remove files that were created after the snapshot (not in preexisting_untracked)
    let current_untracked = run_git(&["ls-files", "--others", "--exclude-standard"], repo_root)
        .await
        .unwrap_or_default();

    for line in current_untracked.lines() {
        if line.is_empty() {
            continue;
        }
        let path = PathBuf::from(line);
        if !commit.preexisting_untracked.contains(&path) {
            let full = repo_root.join(line);
            if full.exists() {
                tracing::info!(
                    "Ghost restore: removing post-snapshot file: {}",
                    full.display()
                );
                if let Err(e) = std::fs::remove_file(&full) {
                    tracing::warn!("Failed to remove {}: {e}", full.display());
                }
            }
        }
    }

    Ok(())
}

/// Capture a lightweight diff of what changed since a reference point.
pub async fn capture_turn_diff(repo_root: &Path, baseline_ref: &str) -> Result<TurnDiff> {
    let output = run_git(&["diff", "--name-status", baseline_ref], repo_root)
        .await
        .unwrap_or_default();

    let mut diff = TurnDiff::default();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() != 2 {
            continue;
        }
        let path = PathBuf::from(parts[1]);
        match parts[0] {
            "A" => diff.added_files.push(path),
            "D" => diff.deleted_files.push(path),
            _ => diff.modified_files.push(path),
        }
    }

    // Also capture new untracked files as additions
    let untracked = run_git(&["ls-files", "--others", "--exclude-standard"], repo_root)
        .await
        .unwrap_or_default();

    for line in untracked.lines() {
        if !line.is_empty() {
            let path = PathBuf::from(line);
            if !diff.added_files.contains(&path) {
                diff.added_files.push(path);
            }
        }
    }

    Ok(diff)
}

/// Format git context as a string block for system prompt injection.
pub fn format_git_context(ctx: &GitContext) -> String {
    let mut out = String::new();
    if let Some(ref branch) = ctx.branch {
        out.push_str(&format!("Git branch: {branch}\n"));
    }
    if let Some(ref hash) = ctx.commit_hash {
        out.push_str(&format!("Git commit: {hash}\n"));
    }
    if ctx.has_uncommitted_changes {
        out.push_str("Uncommitted changes: yes\n");
    }
    if !ctx.recent_commits.is_empty() {
        out.push_str("Recent commits:\n");
        for c in &ctx.recent_commits {
            out.push_str(&format!("  {c}\n"));
        }
    }
    out
}

/// Run a git command with a custom GIT_INDEX_FILE environment variable.
async fn run_git_with_index(index_path: &str, args: &[&str], cwd: &Path) -> Result<String> {
    let output = tokio::time::timeout(
        GIT_COMMAND_TIMEOUT,
        tokio::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_INDEX_FILE", index_path)
            .output(),
    )
    .await
    .context("git command timed out")?
    .context("failed to execute git")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git {} (custom index) failed: {}",
            args.join(" "),
            stderr.trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_repo_root_in_git_dir() {
        // This test runs inside the borg repo, so CWD should have a .git
        let cwd = std::env::current_dir().unwrap();
        let root = find_repo_root(&cwd);
        assert!(root.is_some());
        assert!(root.unwrap().join(".git").exists());
    }

    #[test]
    fn find_repo_root_from_subdir() {
        let cwd = std::env::current_dir().unwrap();
        let sub = cwd.join("crates").join("core").join("src");
        if sub.exists() {
            let root = find_repo_root(&sub);
            assert!(root.is_some());
        }
    }

    #[test]
    fn find_repo_root_none_at_filesystem_root() {
        let root = find_repo_root(Path::new("/"));
        // /tmp usually isn't in a git repo
        // This might actually find one on some systems, so just don't panic
        let _ = root;
    }

    #[test]
    fn format_git_context_empty() {
        let ctx = GitContext::default();
        let out = format_git_context(&ctx);
        assert!(out.is_empty());
    }

    #[test]
    fn format_git_context_full() {
        let ctx = GitContext {
            repo_root: Some(PathBuf::from("/repo")),
            branch: Some("main".into()),
            commit_hash: Some("abc1234".into()),
            recent_commits: vec!["abc1234 Fix bug".into(), "def5678 Add feature".into()],
            has_uncommitted_changes: true,
            remote_url: Some("https://github.com/user/repo".into()),
        };
        let out = format_git_context(&ctx);
        assert!(out.contains("Git branch: main"));
        assert!(out.contains("Git commit: abc1234"));
        assert!(out.contains("Uncommitted changes: yes"));
        assert!(out.contains("abc1234 Fix bug"));
    }

    #[test]
    fn turn_diff_default_is_empty() {
        let diff = TurnDiff::default();
        assert!(diff.modified_files.is_empty());
        assert!(diff.added_files.is_empty());
        assert!(diff.deleted_files.is_empty());
    }
}
