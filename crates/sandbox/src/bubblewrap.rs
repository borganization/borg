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
    use std::path::PathBuf;

    fn default_policy() -> SandboxPolicy {
        SandboxPolicy {
            network: false,
            fs_read: vec![],
            fs_write: vec![],
        }
    }

    #[test]
    fn tool_dir_mounted_read_only() {
        let tool_dir = PathBuf::from("/home/user/.tamagotchi/tools/my_tool");
        let args = build_bwrap_args(&default_policy(), &tool_dir);
        let joined = args.join(" ");
        assert!(joined.contains("--ro-bind /home/user/.tamagotchi/tools/my_tool"));
    }

    #[test]
    fn system_paths_mounted_read_only() {
        let tool_dir = PathBuf::from("/tmp/tool");
        let args = build_bwrap_args(&default_policy(), &tool_dir);
        let joined = args.join(" ");
        // At least /usr and /bin should exist on Linux
        for path in &["/usr", "/bin"] {
            if Path::new(path).exists() {
                assert!(
                    joined.contains(&format!("--ro-bind {path} {path}")),
                    "expected {path} to be ro-bind mounted"
                );
            }
        }
    }

    #[test]
    fn tmpfs_proc_dev_present() {
        let args = build_bwrap_args(&default_policy(), Path::new("/tmp/tool"));
        let joined = args.join(" ");
        assert!(joined.contains("--tmpfs /tmp"));
        assert!(joined.contains("--proc /proc"));
        assert!(joined.contains("--dev /dev"));
    }

    #[test]
    fn custom_fs_read_paths_ro_bind() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec!["/data/readonly".to_string()],
            fs_write: vec![],
        };
        let args = build_bwrap_args(&policy, Path::new("/tmp/tool"));
        let joined = args.join(" ");
        assert!(joined.contains("--ro-bind /data/readonly /data/readonly"));
    }

    #[test]
    fn custom_fs_write_paths_rw_bind() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec![],
            fs_write: vec!["/data/writable".to_string()],
        };
        let args = build_bwrap_args(&policy, Path::new("/tmp/tool"));
        let joined = args.join(" ");
        assert!(joined.contains("--bind /data/writable /data/writable"));
    }

    #[test]
    fn network_unshared_when_disabled() {
        let args = build_bwrap_args(&default_policy(), Path::new("/tmp/tool"));
        assert!(args.contains(&"--unshare-net".to_string()));
    }

    #[test]
    fn network_not_unshared_when_enabled() {
        let policy = SandboxPolicy {
            network: true,
            fs_read: vec![],
            fs_write: vec![],
        };
        let args = build_bwrap_args(&policy, Path::new("/tmp/tool"));
        assert!(!args.contains(&"--unshare-net".to_string()));
    }

    #[test]
    fn pid_unshare_and_die_with_parent() {
        let args = build_bwrap_args(&default_policy(), Path::new("/tmp/tool"));
        assert!(args.contains(&"--unshare-pid".to_string()));
        assert!(args.contains(&"--die-with-parent".to_string()));
    }
}
