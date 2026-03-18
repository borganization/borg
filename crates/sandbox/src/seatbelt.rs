use crate::policy::SandboxPolicy;
use std::path::Path;

/// Validate that a path is safe for interpolation into a Seatbelt profile.
/// Rejects paths containing characters that could inject arbitrary Seatbelt rules.
fn validate_seatbelt_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') {
        return Err(format!(
            "Sandbox path must be absolute (start with /): {path}"
        ));
    }
    for ch in ['"', '(', ')', '\n', '\r', '\\'] {
        if path.contains(ch) {
            return Err(format!(
                "Sandbox path contains unsafe character {ch:?}: {path}"
            ));
        }
    }
    if !path.is_ascii() {
        return Err(format!(
            "Sandbox path contains non-ASCII characters: {path}"
        ));
    }
    Ok(())
}

/// Generate a macOS Seatbelt profile string for sandbox-exec.
///
/// `runtime_bin` is the absolute path to the runtime binary (e.g. `/usr/bin/python3`)
/// that the tool is allowed to execute. If `None`, process-exec is allowed broadly
/// (less secure, but backwards-compatible).
pub fn generate_profile(
    policy: &SandboxPolicy,
    tool_dir: &Path,
    runtime_bin: Option<&str>,
) -> Result<String, String> {
    // Canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
    let canonical_dir = tool_dir
        .canonicalize()
        .unwrap_or_else(|_| tool_dir.to_path_buf());
    let tool_dir_str = canonical_dir.to_string_lossy();
    validate_seatbelt_path(&tool_dir_str)?;

    let mut profile = String::from("(version 1)\n(deny default)\n");

    // Restrict process-exec to the runtime binary and standard system paths
    if let Some(bin) = runtime_bin {
        validate_seatbelt_path(bin)?;
        profile.push_str(&format!("(allow process-exec (literal \"{bin}\"))\n"));
        // Allow standard system binaries that scripts may invoke (cat, grep, curl, etc.)
        profile.push_str("(allow process-exec (subpath \"/bin\"))\n");
        profile.push_str("(allow process-exec (subpath \"/usr/bin\"))\n");
        profile.push_str("(allow process-exec (subpath \"/usr/local/bin\"))\n");
    } else {
        profile.push_str("(allow process-exec)\n");
    }
    profile.push_str("(allow process-fork)\n");
    profile.push_str("(allow sysctl-read)\n");
    profile.push_str("(allow mach-lookup)\n");

    // Allow unrestricted file reads — runtimes need to read from many system
    // paths for dynamic linking, config, locale, etc. The security boundary is
    // write/exec/network restriction. Sensitive paths are filtered at the
    // manifest level via the blocked_paths security config.
    profile.push_str("(allow file-read*)\n");

    // Allow writing to /dev (stdout/stderr file descriptors)
    profile.push_str("(allow file-write* (subpath \"/dev\"))\n");
    // Allow writing to /tmp
    profile.push_str("(allow file-write* (subpath \"/private/tmp\"))\n");
    profile.push_str("(allow file-write* (subpath \"/tmp\"))\n");
    // Allow writing to the tool's working directory
    profile.push_str(&format!(
        "(allow file-write* (subpath \"{tool_dir_str}\"))\n"
    ));

    // Additional read paths from policy
    for path in &policy.fs_read {
        validate_seatbelt_path(path)?;
        profile.push_str(&format!("(allow file-read* (subpath \"{path}\"))\n"));
    }

    // Additional write paths from policy
    for path in &policy.fs_write {
        validate_seatbelt_path(path)?;
        profile.push_str(&format!("(allow file-write* (subpath \"{path}\"))\n"));
    }

    // Network access
    if policy.network {
        profile.push_str("(allow network*)\n");
    }

    Ok(profile)
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
    fn profile_starts_with_deny_default() {
        let profile = generate_profile(&default_policy(), Path::new("/tmp/tool"), None).unwrap();
        assert!(profile.contains("(deny default)"));
    }

    #[test]
    fn profile_includes_tool_dir() {
        let profile = generate_profile(&default_policy(), Path::new("/my/tool/dir"), None).unwrap();
        // Tool dir should have write access
        assert!(profile.contains("(allow file-write* (subpath \"/my/tool/dir\"))"));
        // Reads are allowed globally
        assert!(profile.contains("(allow file-read*)"));
    }

    #[test]
    fn profile_no_network_by_default() {
        let profile = generate_profile(&default_policy(), Path::new("/tmp/tool"), None).unwrap();
        assert!(!profile.contains("(allow network*)"));
    }

    #[test]
    fn profile_network_when_allowed() {
        let policy = SandboxPolicy {
            network: true,
            fs_read: vec![],
            fs_write: vec![],
        };
        let profile = generate_profile(&policy, Path::new("/tmp/tool"), None).unwrap();
        assert!(profile.contains("(allow network*)"));
    }

    #[test]
    fn profile_additional_read_paths() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec!["/data/input".to_string()],
            fs_write: vec![],
        };
        let profile = generate_profile(&policy, Path::new("/tmp/tool"), None).unwrap();
        assert!(profile.contains("(allow file-read* (subpath \"/data/input\"))"));
    }

    #[test]
    fn profile_additional_write_paths() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec![],
            fs_write: vec!["/data/output".to_string()],
        };
        let profile = generate_profile(&policy, Path::new("/tmp/tool"), None).unwrap();
        assert!(profile.contains("(allow file-write* (subpath \"/data/output\"))"));
    }

    #[test]
    fn profile_broad_exec_without_runtime() {
        let profile = generate_profile(&default_policy(), Path::new("/tmp/tool"), None).unwrap();
        assert!(profile.contains("(allow process-exec)"));
        assert!(profile.contains("(allow process-fork)"));
    }

    #[test]
    fn profile_restricts_exec_to_runtime_binary() {
        let profile = generate_profile(
            &default_policy(),
            Path::new("/tmp/tool"),
            Some("/usr/bin/python3"),
        )
        .unwrap();
        assert!(profile.contains("(allow process-exec (literal \"/usr/bin/python3\"))"));
        assert!(!profile.contains("(allow process-exec)\n"));
        assert!(profile.contains("(allow process-fork)"));
    }

    #[test]
    fn rejects_quote_injection_in_fs_read() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec!["\"))\n(allow default)\n(deny file-read* (subpath \"".to_string()],
            fs_write: vec![],
        };
        let result = generate_profile(&policy, Path::new("/tmp/tool"), None);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_quote_injection_in_fs_write() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec![],
            fs_write: vec!["/foo\"))\n(allow default)".to_string()],
        };
        let result = generate_profile(&policy, Path::new("/tmp/tool"), None);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_relative_path() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec!["relative/path".to_string()],
            fs_write: vec![],
        };
        let result = generate_profile(&policy, Path::new("/tmp/tool"), None);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_parentheses_in_path() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec!["/foo(bar)".to_string()],
            fs_write: vec![],
        };
        let result = generate_profile(&policy, Path::new("/tmp/tool"), None);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_newline_in_path() {
        let policy = SandboxPolicy {
            network: false,
            fs_read: vec!["/foo\nbar".to_string()],
            fs_write: vec![],
        };
        let result = generate_profile(&policy, Path::new("/tmp/tool"), None);
        assert!(result.is_err());
    }

    #[test]
    fn validate_seatbelt_path_accepts_valid() {
        assert!(validate_seatbelt_path("/usr/local/bin").is_ok());
        assert!(validate_seatbelt_path("/tmp/my-tool_dir/v2").is_ok());
    }

    #[test]
    fn validate_seatbelt_path_rejects_non_ascii() {
        assert!(validate_seatbelt_path("/tmp/tëst").is_err());
    }
}
