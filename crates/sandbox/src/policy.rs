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
