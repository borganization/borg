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

impl SandboxPolicy {
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
        let profile = generate_profile(self, tool_dir);

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
}
