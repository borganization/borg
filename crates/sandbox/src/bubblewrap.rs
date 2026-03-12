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
