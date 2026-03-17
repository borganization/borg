use crate::policy::SandboxPolicy;
use std::path::Path;

/// Build bwrap (Bubblewrap) argument list for Linux sandboxing
pub fn build_bwrap_args(policy: &SandboxPolicy, tool_dir: &Path) -> Vec<String> {
    let tool_dir_str = tool_dir.to_string_lossy().to_string();
    let mut args = Vec::new();

    // Read-only bind mount of the tool directory
    let tool_dir_dest = tool_dir_str.clone();
    args.extend(["--ro-bind".to_string(), tool_dir_str, tool_dir_dest]);

    // Bind standard system paths read-only
    for path in &["/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc"] {
        if Path::new(path).exists() {
            args.extend(["--ro-bind".to_string(), path.to_string(), path.to_string()]);
        }
    }

    // Tmpfs for scratch space
    args.extend(["--tmpfs".to_string(), "/tmp".to_string()]);

    // Proc filesystem
    args.extend(["--proc".to_string(), "/proc".to_string()]);

    // Dev filesystem
    args.extend(["--dev".to_string(), "/dev".to_string()]);

    // Additional read paths
    for path in &policy.fs_read {
        args.extend(["--ro-bind".to_string(), path.clone(), path.clone()]);
    }

    // Additional write paths
    for path in &policy.fs_write {
        args.extend(["--bind".to_string(), path.clone(), path.clone()]);
    }

    // Network isolation (unshare network namespace unless allowed)
    if !policy.network {
        args.push("--unshare-net".to_string());
    }

    // Unshare other namespaces for isolation
    args.push("--unshare-pid".to_string());
    args.push("--die-with-parent".to_string());

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_policy() -> SandboxPolicy {
        SandboxPolicy {
            network: false,
            fs_read: vec![],
            fs_write: vec![],
        }
    }

    #[test]
    fn includes_tool_dir_ro_bind() {
        let args = build_bwrap_args(
            &default_policy(),
            Path::new("/home/user/.borg/tools/my-tool"),
        );
        assert!(args.windows(3).any(|w| w[0] == "--ro-bind"
            && w[1] == "/home/user/.borg/tools/my-tool"
            && w[2] == "/home/user/.borg/tools/my-tool"));
    }

    #[test]
    fn network_isolated_by_default() {
        let args = build_bwrap_args(&default_policy(), Path::new("/tmp/tool"));
        assert!(args.contains(&"--unshare-net".to_string()));
    }

    #[test]
    fn network_allowed_when_policy_permits() {
        let policy = SandboxPolicy {
            network: true,
            fs_read: vec![],
            fs_write: vec![],
        };
        let args = build_bwrap_args(&policy, Path::new("/tmp/tool"));
        assert!(!args.contains(&"--unshare-net".to_string()));
    }

    #[test]
    fn additional_fs_read_paths() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec!["/data/input".to_string()],
            fs_write: vec![],
        };
        let args = build_bwrap_args(&policy, Path::new("/tmp/tool"));
        assert!(args
            .windows(3)
            .any(|w| w[0] == "--ro-bind" && w[1] == "/data/input"));
    }

    #[test]
    fn additional_fs_write_paths() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec![],
            fs_write: vec!["/data/output".to_string()],
        };
        let args = build_bwrap_args(&policy, Path::new("/tmp/tool"));
        assert!(args
            .windows(3)
            .any(|w| w[0] == "--bind" && w[1] == "/data/output"));
    }

    #[test]
    fn always_includes_pid_isolation() {
        let args = build_bwrap_args(&default_policy(), Path::new("/tmp/tool"));
        assert!(args.contains(&"--unshare-pid".to_string()));
        assert!(args.contains(&"--die-with-parent".to_string()));
    }

    #[test]
    fn includes_tmpfs_proc_dev() {
        let args = build_bwrap_args(&default_policy(), Path::new("/tmp/tool"));
        assert!(args.windows(2).any(|w| w[0] == "--tmpfs" && w[1] == "/tmp"));
        assert!(args.windows(2).any(|w| w[0] == "--proc" && w[1] == "/proc"));
        assert!(args.windows(2).any(|w| w[0] == "--dev" && w[1] == "/dev"));
    }
}
