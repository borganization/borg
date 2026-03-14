use crate::policy::SandboxPolicy;
use std::path::Path;

/// Generate a macOS Seatbelt profile string for sandbox-exec
pub fn generate_profile(policy: &SandboxPolicy, tool_dir: &Path) -> String {
    let tool_dir_str = tool_dir.to_string_lossy();
    let mut profile = String::from("(version 1)\n(deny default)\n");

    // Always allow basic process operations
    profile.push_str("(allow process-exec)\n");
    profile.push_str("(allow process-fork)\n");
    profile.push_str("(allow sysctl-read)\n");
    profile.push_str("(allow mach-lookup)\n");

    // Allow reading the tool directory
    profile.push_str(&format!(
        "(allow file-read* (subpath \"{tool_dir_str}\"))\n"
    ));

    // Allow reading standard system paths
    profile.push_str("(allow file-read* (subpath \"/usr\"))\n");
    profile.push_str("(allow file-read* (subpath \"/lib\"))\n");
    profile.push_str("(allow file-read* (subpath \"/System\"))\n");
    profile.push_str("(allow file-read* (subpath \"/Library\"))\n");
    profile.push_str("(allow file-read* (subpath \"/dev\"))\n");
    profile.push_str("(allow file-read* (subpath \"/private/tmp\"))\n");
    profile.push_str("(allow file-read* (subpath \"/private/var\"))\n");
    profile.push_str("(allow file-read* (subpath \"/bin\"))\n");
    profile.push_str("(allow file-read* (subpath \"/sbin\"))\n");

    // Allow writing to /tmp
    profile.push_str("(allow file-write* (subpath \"/private/tmp\"))\n");
    profile.push_str("(allow file-write* (subpath \"/tmp\"))\n");

    // Additional read paths from policy
    for path in &policy.fs_read {
        profile.push_str(&format!("(allow file-read* (subpath \"{path}\"))\n"));
    }

    // Additional write paths from policy
    for path in &policy.fs_write {
        profile.push_str(&format!("(allow file-write* (subpath \"{path}\"))\n"));
    }

    // Network access
    if policy.network {
        profile.push_str("(allow network*)\n");
    }

    profile
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn profile_starts_with_version_and_deny() {
        let policy = SandboxPolicy::default();
        let profile = generate_profile(&policy, &PathBuf::from("/tmp/tool"));
        assert!(profile.starts_with("(version 1)\n(deny default)\n"));
    }

    #[test]
    fn profile_allows_process_ops() {
        let policy = SandboxPolicy::default();
        let profile = generate_profile(&policy, &PathBuf::from("/tmp/tool"));
        assert!(profile.contains("(allow process-exec)"));
        assert!(profile.contains("(allow process-fork)"));
        assert!(profile.contains("(allow sysctl-read)"));
        assert!(profile.contains("(allow mach-lookup)"));
    }

    #[test]
    fn tool_dir_readable() {
        let policy = SandboxPolicy::default();
        let profile = generate_profile(&policy, &PathBuf::from("/home/user/tools/my_tool"));
        assert!(profile.contains("(allow file-read* (subpath \"/home/user/tools/my_tool\"))"));
    }

    #[test]
    fn system_paths_readable() {
        let policy = SandboxPolicy::default();
        let profile = generate_profile(&policy, &PathBuf::from("/tmp/tool"));
        for path in &["/usr", "/lib", "/System", "/Library", "/bin", "/sbin"] {
            assert!(
                profile.contains(&format!("(allow file-read* (subpath \"{path}\"))")),
                "expected {path} to be readable"
            );
        }
    }

    #[test]
    fn tmp_writable() {
        let policy = SandboxPolicy::default();
        let profile = generate_profile(&policy, &PathBuf::from("/tmp/tool"));
        assert!(profile.contains("(allow file-write* (subpath \"/private/tmp\"))"));
        assert!(profile.contains("(allow file-write* (subpath \"/tmp\"))"));
    }

    #[test]
    fn custom_read_paths_included() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec!["/data/custom".to_string()],
            fs_write: vec![],
        };
        let profile = generate_profile(&policy, &PathBuf::from("/tmp/tool"));
        assert!(profile.contains("(allow file-read* (subpath \"/data/custom\"))"));
    }

    #[test]
    fn custom_write_paths_included() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec![],
            fs_write: vec!["/data/output".to_string()],
        };
        let profile = generate_profile(&policy, &PathBuf::from("/tmp/tool"));
        assert!(profile.contains("(allow file-write* (subpath \"/data/output\"))"));
    }

    #[test]
    fn network_allowed_when_enabled() {
        let policy = SandboxPolicy {
            network: true,
            fs_read: vec![],
            fs_write: vec![],
        };
        let profile = generate_profile(&policy, &PathBuf::from("/tmp/tool"));
        assert!(profile.contains("(allow network*)"));
    }

    #[test]
    fn network_denied_when_disabled() {
        let policy = SandboxPolicy::default();
        let profile = generate_profile(&policy, &PathBuf::from("/tmp/tool"));
        assert!(!profile.contains("(allow network*)"));
    }
}
