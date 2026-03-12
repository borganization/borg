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
