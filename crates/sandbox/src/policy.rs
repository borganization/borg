use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxPolicy {
    pub network: bool,
    pub fs_read: Vec<String>,
    pub fs_write: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SandboxCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Check if a path matches any blocked path pattern.
/// Blocked patterns are basename fragments (e.g., ".ssh", "credentials").
fn is_path_blocked(path: &str, blocked_paths: &[String]) -> bool {
    for blocked in blocked_paths {
        // Match if the path contains the blocked pattern as a path component
        if path.contains(blocked) {
            return true;
        }
    }
    false
}

/// Filter out paths that match the security blocklist.
fn filter_blocked(paths: &[String], blocked_paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| !is_path_blocked(p, blocked_paths))
        .cloned()
        .collect()
}

/// Expand `~` prefix to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{rest}", home.display());
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().to_string();
        }
    }
    path.to_string()
}

/// Expand tilde in all paths.
fn expand_tilde_paths(paths: &[String]) -> Vec<String> {
    paths.iter().map(|p| expand_tilde(p)).collect()
}

impl SandboxPolicy {
    /// Return a copy of this policy with blocked paths removed from fs_read/fs_write.
    pub fn with_blocked_paths_filtered(&self, blocked_paths: &[String]) -> Self {
        Self {
            network: self.network,
            fs_read: filter_blocked(&self.fs_read, blocked_paths),
            fs_write: filter_blocked(&self.fs_write, blocked_paths),
        }
    }

    /// Return a copy with `~` expanded to the user's home directory in all paths.
    pub fn with_tildes_expanded(&self) -> Self {
        Self {
            network: self.network,
            fs_read: expand_tilde_paths(&self.fs_read),
            fs_write: expand_tilde_paths(&self.fs_write),
        }
    }

    pub fn wrap_command(
        &self,
        program: &str,
        args: &[String],
        tool_dir: &std::path::Path,
    ) -> SandboxCommand {
        if cfg!(target_os = "macos") {
            self.wrap_seatbelt(program, args, tool_dir)
        } else if cfg!(target_os = "linux") {
            self.wrap_bubblewrap(program, args, tool_dir)
        } else {
            // No sandboxing on other platforms
            SandboxCommand {
                program: program.to_string(),
                args: args.to_vec(),
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn wrap_seatbelt(
        &self,
        program: &str,
        args: &[String],
        tool_dir: &std::path::Path,
    ) -> SandboxCommand {
        use crate::seatbelt::generate_profile;
        let profile = match generate_profile(self, tool_dir, Some(program)) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to generate sandbox profile: {e}. Running unsandboxed.");
                return SandboxCommand {
                    program: program.to_string(),
                    args: args.to_vec(),
                };
            }
        };

        let mut sandbox_args = vec!["-p".to_string(), profile, program.to_string()];
        sandbox_args.extend(args.iter().cloned());

        SandboxCommand {
            program: "sandbox-exec".to_string(),
            args: sandbox_args,
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn wrap_seatbelt(
        &self,
        program: &str,
        args: &[String],
        _tool_dir: &std::path::Path,
    ) -> SandboxCommand {
        SandboxCommand {
            program: program.to_string(),
            args: args.to_vec(),
        }
    }

    #[cfg(target_os = "linux")]
    fn wrap_bubblewrap(
        &self,
        program: &str,
        args: &[String],
        tool_dir: &std::path::Path,
    ) -> SandboxCommand {
        use crate::bubblewrap::build_bwrap_args;
        let mut bwrap_args = build_bwrap_args(self, tool_dir);
        bwrap_args.push(program.to_string());
        bwrap_args.extend(args.iter().cloned());

        SandboxCommand {
            program: "bwrap".to_string(),
            args: bwrap_args,
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn wrap_bubblewrap(
        &self,
        program: &str,
        args: &[String],
        _tool_dir: &std::path::Path,
    ) -> SandboxCommand {
        SandboxCommand {
            program: program.to_string(),
            args: args.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn default_policy_values() {
        let policy = SandboxPolicy::default();
        assert!(!policy.network);
        assert!(policy.fs_read.is_empty());
        assert!(policy.fs_write.is_empty());
    }

    #[test]
    fn wrap_command_returns_sandbox_command() {
        let policy = SandboxPolicy::default();
        let args = vec!["script.py".to_string()];
        let cmd = policy.wrap_command("python3", &args, Path::new("/tmp/tool"));
        // On Linux, should wrap with bwrap
        // On macOS, should wrap with sandbox-exec
        // On other, should pass through
        assert!(!cmd.program.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_wraps_with_bwrap() {
        let policy = SandboxPolicy::default();
        let args = vec!["script.py".to_string()];
        let cmd = policy.wrap_command("python3", &args, Path::new("/tmp/tool"));
        assert_eq!(cmd.program, "bwrap");
        // The original program and args should be at the end
        assert!(cmd.args.contains(&"python3".to_string()));
        assert!(cmd.args.contains(&"script.py".to_string()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_preserves_original_args_order() {
        let policy = SandboxPolicy::default();
        let args = vec!["arg1".to_string(), "arg2".to_string()];
        let cmd = policy.wrap_command("node", &args, Path::new("/tmp/tool"));
        // Original program and args should appear at the end after bwrap flags
        let node_pos = cmd.args.iter().position(|a| a == "node").unwrap();
        let arg1_pos = cmd.args.iter().position(|a| a == "arg1").unwrap();
        let arg2_pos = cmd.args.iter().position(|a| a == "arg2").unwrap();
        assert!(node_pos < arg1_pos);
        assert!(arg1_pos < arg2_pos);
    }

    #[test]
    fn filter_blocked_paths_removes_sensitive() {
        let blocked = vec![".ssh".into(), ".aws".into(), "credentials".into()];
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec![
                "/home/user/.ssh".into(),
                "/data/public".into(),
                "/home/user/.aws/config".into(),
            ],
            fs_write: vec!["/tmp/output".into(), "/home/user/credentials".into()],
        };
        let filtered = policy.with_blocked_paths_filtered(&blocked);
        assert_eq!(filtered.fs_read, vec!["/data/public".to_string()]);
        assert_eq!(filtered.fs_write, vec!["/tmp/output".to_string()]);
    }

    #[test]
    fn filter_blocked_paths_empty_blocklist() {
        let policy = SandboxPolicy {
            network: true,
            fs_read: vec!["/home/user/.ssh".into()],
            fs_write: vec![],
        };
        let filtered = policy.with_blocked_paths_filtered(&[]);
        assert_eq!(filtered.fs_read, policy.fs_read);
        assert!(filtered.network);
    }

    #[test]
    fn is_path_blocked_matches() {
        let blocked = vec![".ssh".into(), ".env".into()];
        assert!(is_path_blocked("/home/user/.ssh/id_rsa", &blocked));
        assert!(is_path_blocked("/project/.env", &blocked));
        assert!(!is_path_blocked("/home/user/code", &blocked));
    }

    #[test]
    fn expand_tilde_expands_home_prefix() {
        let home = dirs::home_dir().expect("home dir must exist for test");
        let home_str = home.to_string_lossy();

        assert_eq!(
            expand_tilde("~/Library/Messages"),
            format!("{home_str}/Library/Messages")
        );
        assert_eq!(
            expand_tilde("~/.borg/channels"),
            format!("{home_str}/.borg/channels")
        );
    }

    #[test]
    fn expand_tilde_preserves_absolute_paths() {
        assert_eq!(expand_tilde("/usr/local/bin"), "/usr/local/bin");
        assert_eq!(expand_tilde("/tmp/test"), "/tmp/test");
    }

    #[test]
    fn expand_tilde_preserves_bare_tilde() {
        let home = dirs::home_dir().expect("home dir must exist for test");
        assert_eq!(expand_tilde("~"), home.to_string_lossy().to_string());
    }

    #[test]
    fn expand_tilde_does_not_expand_mid_path() {
        // Only leading ~ should be expanded
        assert_eq!(expand_tilde("/data/~user"), "/data/~user");
    }

    #[test]
    fn with_tildes_expanded_applies_to_all_paths() {
        let home = dirs::home_dir().expect("home dir must exist for test");
        let home_str = home.to_string_lossy();

        let policy = SandboxPolicy {
            network: false,
            fs_read: vec!["~/Library/Messages".into(), "/etc/ssl".into()],
            fs_write: vec!["~/.borg/channels/imessage".into()],
        };
        let expanded = policy.with_tildes_expanded();

        assert_eq!(
            expanded.fs_read,
            vec![
                format!("{home_str}/Library/Messages"),
                "/etc/ssl".to_string(),
            ]
        );
        assert_eq!(
            expanded.fs_write,
            vec![format!("{home_str}/.borg/channels/imessage"),]
        );
        assert!(!expanded.network);
    }
}
