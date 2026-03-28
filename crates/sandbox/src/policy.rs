use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxPolicy {
    pub network: bool,
    pub fs_read: Vec<String>,
    pub fs_write: Vec<String>,
    /// Glob patterns to deny read access to (e.g. `["~/.borg/**"]`).
    /// Evaluated after fs_read allows; deny takes precedence.
    #[serde(default)]
    pub deny_read: Vec<String>,
    /// Paths to deny write access to, even if not in fs_write.
    /// Always includes `~/.borg/` to protect agent config from tool writes.
    #[serde(default)]
    pub deny_write: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SandboxCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Check if a path matches any blocked path pattern.
/// Blocked patterns are matched against individual path components (split by `/`)
/// to avoid false positives from substring matching (e.g., `.aws_backup` matching `.aws`).
fn is_path_blocked(path: &str, blocked_paths: &[String]) -> bool {
    // Extract normal path components (skips `.`, `..`, root, and prefix components)
    let normalized = std::path::Path::new(path);
    let components: Vec<&str> = normalized
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    for blocked in blocked_paths {
        // Check if any path component matches or starts with the blocked pattern
        for component in &components {
            if *component == blocked.as_str() || component.starts_with(&format!("{blocked}/")) {
                return true;
            }
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
    /// Filter blocked paths from fs_read/fs_write, consuming self.
    pub fn with_blocked_paths_filtered(mut self, blocked_paths: &[String]) -> Self {
        self.fs_read = filter_blocked(&self.fs_read, blocked_paths);
        self.fs_write = filter_blocked(&self.fs_write, blocked_paths);
        self
    }

    /// Expand `~` to the user's home directory in all paths, consuming self.
    pub fn with_tildes_expanded(mut self) -> Self {
        self.fs_read = expand_tilde_paths(&self.fs_read);
        self.fs_write = expand_tilde_paths(&self.fs_write);
        self.deny_read = expand_tilde_paths(&self.deny_read);
        self.deny_write = expand_tilde_paths(&self.deny_write);
        self
    }

    /// Apply smart defaults based on policy properties, consuming self.
    /// For example, `network == true` implies TLS certificate paths should be readable.
    /// Idempotent: does not add paths that are already present.
    pub fn with_defaults_applied(mut self) -> Self {
        if self.network {
            for tls_path in &["/etc/ssl", "/etc/ssl/certs"] {
                let s = tls_path.to_string();
                if !self.fs_read.contains(&s) {
                    self.fs_read.push(s);
                }
            }
        }
        self
    }

    /// Add `~/.borg/` to deny_write to protect agent config, consuming self.
    pub fn with_borg_dir_protected(mut self) -> Self {
        let borg_dir = dirs::home_dir()
            .map(|h| format!("{}/.borg", h.display()))
            .unwrap_or_else(|| {
                tracing::warn!("Could not determine home directory; using literal '~/.borg' which may not resolve");
                "~/.borg".to_string()
            });

        if !self.deny_write.iter().any(|p| p == &borg_dir) {
            self.deny_write.push(borg_dir);
        }
        self
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
                tracing::error!(
                    "Failed to generate sandbox profile: {e}. Refusing to run unsandboxed."
                );
                // Fail closed: return a command that immediately exits with error
                return SandboxCommand {
                    program: "/bin/sh".to_string(),
                    args: vec![
                        "-c".to_string(),
                        format!("echo 'Sandbox profile generation failed: {e}' >&2; exit 1"),
                    ],
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
            fs_read: vec![
                "/home/user/.ssh".into(),
                "/data/public".into(),
                "/home/user/.aws/config".into(),
            ],
            fs_write: vec!["/tmp/output".into(), "/home/user/credentials".into()],
            ..Default::default()
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
            ..Default::default()
        };
        let expected_fs_read = policy.fs_read.clone();
        let filtered = policy.with_blocked_paths_filtered(&[]);
        assert_eq!(filtered.fs_read, expected_fs_read);
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
    fn is_path_blocked_no_false_positive_on_prefix() {
        let blocked = vec![".aws".into()];
        // `.aws_backup` is a different component and should NOT be blocked
        assert!(!is_path_blocked("/home/user/.aws_backup/data", &blocked));
        // `.aws` itself should still be blocked
        assert!(is_path_blocked("/home/user/.aws/config", &blocked));
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
    fn with_borg_dir_protected_adds_deny_write() {
        let policy = SandboxPolicy::default();
        let protected = policy.with_borg_dir_protected();
        assert!(!protected.deny_write.is_empty());
        assert!(protected.deny_write[0].ends_with("/.borg"));
    }

    #[test]
    fn with_borg_dir_protected_is_idempotent() {
        let policy = SandboxPolicy::default();
        let protected = policy.with_borg_dir_protected().with_borg_dir_protected();
        // Should not duplicate the entry
        let borg_count = protected
            .deny_write
            .iter()
            .filter(|p| p.ends_with("/.borg"))
            .count();
        assert_eq!(borg_count, 1);
    }

    #[test]
    fn deny_read_preserved_through_filter() {
        let policy = SandboxPolicy {
            deny_read: vec!["/secret/data".into()],
            ..Default::default()
        };
        let filtered = policy.with_blocked_paths_filtered(&[]);
        assert_eq!(filtered.deny_read, vec!["/secret/data".to_string()]);
    }

    // -- with_defaults_applied --

    #[test]
    fn defaults_applied_adds_tls_for_network() {
        let policy = SandboxPolicy {
            network: true,
            fs_read: vec!["/data".into()],
            ..Default::default()
        };
        let applied = policy.with_defaults_applied();
        assert!(
            applied.fs_read.contains(&"/etc/ssl".to_string()),
            "network=true should add /etc/ssl"
        );
        assert!(
            applied.fs_read.contains(&"/etc/ssl/certs".to_string()),
            "network=true should add /etc/ssl/certs"
        );
        assert!(
            applied.fs_read.contains(&"/data".to_string()),
            "original paths preserved"
        );
    }

    #[test]
    fn defaults_applied_no_tls_without_network() {
        let policy = SandboxPolicy {
            network: false,
            ..Default::default()
        };
        let applied = policy.with_defaults_applied();
        assert!(
            !applied.fs_read.contains(&"/etc/ssl".to_string()),
            "network=false should not add TLS paths"
        );
    }

    #[test]
    fn defaults_applied_is_idempotent() {
        let policy = SandboxPolicy {
            network: true,
            fs_read: vec!["/etc/ssl".into()],
            ..Default::default()
        };
        let applied = policy.with_defaults_applied();
        let count = applied.fs_read.iter().filter(|p| *p == "/etc/ssl").count();
        assert_eq!(count, 1, "should not duplicate existing /etc/ssl");
    }

    #[test]
    fn defaults_applied_twice_is_idempotent() {
        let policy = SandboxPolicy {
            network: true,
            ..Default::default()
        };
        let once = policy.with_defaults_applied();
        let once_len = once.fs_read.len();
        let twice = once.with_defaults_applied();
        assert_eq!(
            once_len,
            twice.fs_read.len(),
            "applying defaults twice should not add duplicates"
        );
    }

    #[test]
    fn defaults_applied_preserves_other_fields() {
        let policy = SandboxPolicy {
            network: true,
            fs_read: vec!["/custom".into()],
            fs_write: vec!["/tmp/out".into()],
            deny_read: vec!["/secret".into()],
            deny_write: vec!["/protected".into()],
        };
        let applied = policy.with_defaults_applied();
        assert!(applied.network);
        assert_eq!(applied.fs_write, vec!["/tmp/out".to_string()]);
        assert_eq!(applied.deny_read, vec!["/secret".to_string()]);
        assert_eq!(applied.deny_write, vec!["/protected".to_string()]);
    }

    #[test]
    fn defaults_then_blocked_filter_overrides_defaults() {
        let policy = SandboxPolicy {
            network: true,
            ..Default::default()
        };
        // Hypothetically block /etc/ssl
        let blocked = vec!["ssl".to_string()];
        let result = policy
            .with_defaults_applied()
            .with_blocked_paths_filtered(&blocked);
        assert!(
            !result.fs_read.contains(&"/etc/ssl".to_string()),
            "blocked paths filter should override defaults"
        );
    }

    #[test]
    fn with_tildes_expanded_applies_to_all_paths() {
        let home = dirs::home_dir().expect("home dir must exist for test");
        let home_str = home.to_string_lossy();

        let policy = SandboxPolicy {
            fs_read: vec!["~/Library/Messages".into(), "/etc/ssl".into()],
            fs_write: vec!["~/.borg/channels/imessage".into()],
            ..Default::default()
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
