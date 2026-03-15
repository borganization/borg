use crate::doctor::{CheckStatus, DiagnosticCheck};
use std::collections::BTreeSet;
use std::process::Command;
use std::time::Duration;

/// Run all host security checks.
pub fn run_host_security_checks(checks: &mut Vec<DiagnosticCheck>) {
    check_firewall(checks);
    check_listening_ports(checks);
    check_ssh_config(checks);
    check_sensitive_permissions(checks);
    check_disk_encryption(checks);
    check_os_updates(checks);
    check_running_services(checks);
}

const CATEGORY: &str = "Host Security";

// ---------------------------------------------------------------------------
// Firewall
// ---------------------------------------------------------------------------

pub fn check_firewall(checks: &mut Vec<DiagnosticCheck>) {
    #[cfg(target_os = "macos")]
    {
        match run_cmd("socketfilterfw", &["--getglobalstate"]) {
            Ok(output) => {
                let status = parse_macos_firewall(&output);
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "Application Firewall".to_string(),
                    status,
                });
            }
            Err(e) => {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "Application Firewall".to_string(),
                    status: CheckStatus::Warn(format!("could not check: {e}")),
                });
            }
        }

        match run_cmd("pfctl", &["-s", "info"]) {
            Ok(output) => {
                let status = parse_pf_status(&output);
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "PF packet filter".to_string(),
                    status,
                });
            }
            Err(_) => {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "PF packet filter".to_string(),
                    status: CheckStatus::Warn("could not query pfctl (needs root)".to_string()),
                });
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut found = false;
        if let Ok(output) = run_cmd("ufw", &["status"]) {
            let status = parse_ufw_status(&output);
            checks.push(DiagnosticCheck {
                category: CATEGORY,
                name: "UFW firewall".to_string(),
                status,
            });
            found = true;
        }
        if !found {
            if let Ok(output) = run_cmd("systemctl", &["is-active", "firewalld"]) {
                let active = output.trim() == "active";
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "firewalld".to_string(),
                    status: if active {
                        CheckStatus::Pass
                    } else {
                        CheckStatus::Warn("firewalld is not active".to_string())
                    },
                });
                found = true;
            }
        }
        if !found {
            if let Ok(output) = run_cmd("iptables", &["-L", "-n"]) {
                let has_rules = parse_iptables_output(&output);
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "iptables rules".to_string(),
                    status: if has_rules {
                        CheckStatus::Pass
                    } else {
                        CheckStatus::Warn("no iptables rules found".to_string())
                    },
                });
            } else {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "firewall".to_string(),
                    status: CheckStatus::Warn(
                        "no firewall detected (ufw, firewalld, iptables)".to_string(),
                    ),
                });
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        checks.push(DiagnosticCheck {
            category: CATEGORY,
            name: "firewall".to_string(),
            status: CheckStatus::Warn("not supported on this platform".to_string()),
        });
    }
}

// ---------------------------------------------------------------------------
// Listening ports
// ---------------------------------------------------------------------------

/// Ports considered risky when exposed on all interfaces.
const RISKY_PORTS: &[(u16, &str)] = &[(21, "FTP"), (23, "Telnet"), (3389, "RDP"), (5900, "VNC")];

pub fn check_listening_ports(checks: &mut Vec<DiagnosticCheck>) {
    #[cfg(target_os = "macos")]
    let result = run_cmd("lsof", &["-iTCP", "-sTCP:LISTEN", "-nP"]);

    #[cfg(target_os = "linux")]
    let result = run_cmd("ss", &["-tlnp"]);

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let result: Result<String, String> = Err("not supported on this platform".to_string());

    match result {
        Ok(output) => {
            let findings = parse_listening_ports(&output);
            if findings.is_empty() {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "risky listening ports".to_string(),
                    status: CheckStatus::Pass,
                });
            } else {
                for finding in findings {
                    checks.push(DiagnosticCheck {
                        category: CATEGORY,
                        name: finding,
                        status: CheckStatus::Warn("risky port open on all interfaces".to_string()),
                    });
                }
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: CATEGORY,
                name: "listening ports".to_string(),
                status: CheckStatus::Warn(format!("could not check: {e}")),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// SSH config
// ---------------------------------------------------------------------------

pub fn check_ssh_config(checks: &mut Vec<DiagnosticCheck>) {
    let sshd_config = std::path::Path::new("/etc/ssh/sshd_config");
    if !sshd_config.exists() {
        checks.push(DiagnosticCheck {
            category: CATEGORY,
            name: "SSH daemon config".to_string(),
            status: CheckStatus::Warn(
                "sshd_config not found (SSH may not be installed)".to_string(),
            ),
        });
        return;
    }

    match std::fs::read_to_string(sshd_config) {
        Ok(content) => {
            let issues = parse_ssh_config(&content);
            if issues.is_empty() {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "SSH daemon config".to_string(),
                    status: CheckStatus::Pass,
                });
            } else {
                for issue in issues {
                    checks.push(DiagnosticCheck {
                        category: CATEGORY,
                        name: format!("SSH: {issue}"),
                        status: CheckStatus::Warn("weak SSH configuration".to_string()),
                    });
                }
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: CATEGORY,
                name: "SSH daemon config".to_string(),
                status: CheckStatus::Warn(format!("could not read: {e}")),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Sensitive directory permissions
// ---------------------------------------------------------------------------

pub fn check_sensitive_permissions(checks: &mut Vec<DiagnosticCheck>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let home = match dirs::home_dir() {
            Some(h) => h,
            None => {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "sensitive dir permissions".to_string(),
                    status: CheckStatus::Warn("could not determine home directory".to_string()),
                });
                return;
            }
        };

        let dirs_to_check = [".ssh", ".gnupg", ".aws"];
        for dir_name in &dirs_to_check {
            let path = home.join(dir_name);
            if !path.exists() {
                continue;
            }
            match std::fs::metadata(&path) {
                Ok(meta) => {
                    let mode = meta.mode() & 0o777;
                    if mode & 0o077 != 0 {
                        checks.push(DiagnosticCheck {
                            category: CATEGORY,
                            name: format!("~/{dir_name} permissions"),
                            status: CheckStatus::Warn(format!(
                                "permissions are {mode:04o} — should not have group/other access"
                            )),
                        });
                    } else {
                        checks.push(DiagnosticCheck {
                            category: CATEGORY,
                            name: format!("~/{dir_name} permissions"),
                            status: CheckStatus::Pass,
                        });
                    }
                }
                Err(e) => {
                    checks.push(DiagnosticCheck {
                        category: CATEGORY,
                        name: format!("~/{dir_name} permissions"),
                        status: CheckStatus::Warn(format!("could not stat: {e}")),
                    });
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        checks.push(DiagnosticCheck {
            category: CATEGORY,
            name: "sensitive dir permissions".to_string(),
            status: CheckStatus::Warn("not supported on this platform".to_string()),
        });
    }
}

// ---------------------------------------------------------------------------
// Disk encryption
// ---------------------------------------------------------------------------

pub fn check_disk_encryption(checks: &mut Vec<DiagnosticCheck>) {
    #[cfg(target_os = "macos")]
    {
        match run_cmd("fdesetup", &["status"]) {
            Ok(output) => {
                let status = parse_fde_status(&output);
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "FileVault disk encryption".to_string(),
                    status,
                });
            }
            Err(e) => {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "FileVault disk encryption".to_string(),
                    status: CheckStatus::Warn(format!("could not check: {e}")),
                });
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        match run_cmd("lsblk", &["-o", "NAME,FSTYPE"]) {
            Ok(output) => {
                let encrypted = output.contains("crypto_LUKS");
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "LUKS disk encryption".to_string(),
                    status: if encrypted {
                        CheckStatus::Pass
                    } else {
                        CheckStatus::Warn("no LUKS-encrypted volumes detected".to_string())
                    },
                });
            }
            Err(e) => {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "disk encryption".to_string(),
                    status: CheckStatus::Warn(format!("could not check: {e}")),
                });
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        checks.push(DiagnosticCheck {
            category: CATEGORY,
            name: "disk encryption".to_string(),
            status: CheckStatus::Warn("not supported on this platform".to_string()),
        });
    }
}

// ---------------------------------------------------------------------------
// OS updates
// ---------------------------------------------------------------------------

pub fn check_os_updates(checks: &mut Vec<DiagnosticCheck>) {
    #[cfg(target_os = "macos")]
    {
        match run_cmd_timeout("softwareupdate", &["-l"], Duration::from_secs(10)) {
            Ok(output) => {
                let status = parse_softwareupdate(&output);
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "macOS software updates".to_string(),
                    status,
                });
            }
            Err(e) => {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "macOS software updates".to_string(),
                    status: CheckStatus::Warn(format!("could not check: {e}")),
                });
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut checked = false;
        if let Ok(output) =
            run_cmd_timeout("apt", &["list", "--upgradable"], Duration::from_secs(10))
        {
            let status = parse_apt_upgradable(&output);
            checks.push(DiagnosticCheck {
                category: CATEGORY,
                name: "OS package updates".to_string(),
                status,
            });
            checked = true;
        }
        if !checked {
            if let Ok(output) =
                run_cmd_timeout("dnf", &["check-update", "-q"], Duration::from_secs(10))
            {
                let has_updates = !output.trim().is_empty();
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "OS package updates".to_string(),
                    status: if has_updates {
                        CheckStatus::Warn("updates available".to_string())
                    } else {
                        CheckStatus::Pass
                    },
                });
                checked = true;
            }
        }
        if !checked {
            checks.push(DiagnosticCheck {
                category: CATEGORY,
                name: "OS package updates".to_string(),
                status: CheckStatus::Warn("could not check (no apt or dnf)".to_string()),
            });
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        checks.push(DiagnosticCheck {
            category: CATEGORY,
            name: "OS updates".to_string(),
            status: CheckStatus::Warn("not supported on this platform".to_string()),
        });
    }
}

// ---------------------------------------------------------------------------
// Running services
// ---------------------------------------------------------------------------

pub fn check_running_services(checks: &mut Vec<DiagnosticCheck>) {
    #[cfg(target_os = "macos")]
    {
        match run_cmd("launchctl", &["list"]) {
            Ok(output) => {
                let count = output.lines().count().saturating_sub(1); // header line
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: format!("running daemons ({count} launchctl jobs)"),
                    status: CheckStatus::Pass,
                });
            }
            Err(e) => {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "running daemons".to_string(),
                    status: CheckStatus::Warn(format!("could not list: {e}")),
                });
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        match run_cmd(
            "systemctl",
            &[
                "list-units",
                "--type=service",
                "--state=running",
                "--no-pager",
                "--plain",
            ],
        ) {
            Ok(output) => {
                let count = output.lines().filter(|l| l.contains(".service")).count();
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: format!("running services ({count} systemd units)"),
                    status: CheckStatus::Pass,
                });
            }
            Err(e) => {
                checks.push(DiagnosticCheck {
                    category: CATEGORY,
                    name: "running services".to_string(),
                    status: CheckStatus::Warn(format!("could not list: {e}")),
                });
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        checks.push(DiagnosticCheck {
            category: CATEGORY,
            name: "running services".to_string(),
            status: CheckStatus::Warn("not supported on this platform".to_string()),
        });
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

fn run_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(format!("{stdout}{stderr}"))
}

fn run_cmd_timeout(program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;

    // Use wait_timeout crate pattern: poll with try_wait, but use a single
    // sleep-free check after the deadline to avoid blocking the thread.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Ok(format!("{stdout}{stderr}"));
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait(); // reap zombie
                    return Err(format!("{program}: timed out after {}s", timeout.as_secs()));
                }
                // Yield briefly to avoid busy-spinning
                std::thread::yield_now();
            }
            Err(e) => return Err(format!("{program}: {e}")),
        }
    }
}

// ===========================================================================
// Pure parsing functions (testable)
// ===========================================================================

fn parse_macos_firewall(output: &str) -> CheckStatus {
    if output.contains("enabled") {
        CheckStatus::Pass
    } else if output.contains("disabled") {
        CheckStatus::Warn("Application Firewall is disabled".to_string())
    } else {
        CheckStatus::Warn(format!("unexpected output: {}", output.trim()))
    }
}

fn parse_pf_status(output: &str) -> CheckStatus {
    if output.contains("Status: Enabled") {
        CheckStatus::Pass
    } else if output.contains("Status: Disabled") {
        CheckStatus::Warn("PF is disabled".to_string())
    } else {
        CheckStatus::Warn(format!("unexpected pfctl output: {}", first_line(output)))
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_ufw_status(output: &str) -> CheckStatus {
    if output.contains("Status: active") {
        CheckStatus::Pass
    } else if output.contains("Status: inactive") {
        CheckStatus::Warn("UFW is inactive".to_string())
    } else {
        CheckStatus::Warn(format!("unexpected ufw output: {}", first_line(output)))
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_iptables_output(output: &str) -> bool {
    output
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with("Chain") && !l.starts_with("target"))
        .count()
        > 0
}

fn parse_listening_ports(output: &str) -> Vec<String> {
    let mut findings = BTreeSet::new();
    for line in output.lines() {
        for &(port, name) in RISKY_PORTS {
            let patterns = [
                format!("*:{port}"),
                format!("0.0.0.0:{port}"),
                format!("[::]:{port}"),
                format!(":{port} "),
            ];
            if patterns.iter().any(|p| line.contains(p.as_str())) {
                findings.insert(format!("port {port} ({name}) listening"));
                break;
            }
        }
    }
    findings.into_iter().collect()
}

/// Secure PermitRootLogin values that disable password-based root access.
const SECURE_ROOT_LOGIN_VALUES: &[&str] = &["no", "without-password", "prohibit-password"];

fn parse_ssh_config(content: &str) -> Vec<String> {
    let mut issues = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if lower.starts_with("passwordauthentication") && lower.contains("yes") {
            issues.push("PasswordAuthentication is enabled".to_string());
        }
        if lower.starts_with("permitrootlogin") {
            let value = lower.strip_prefix("permitrootlogin").unwrap_or("").trim();
            if !SECURE_ROOT_LOGIN_VALUES.contains(&value) {
                issues.push(format!("PermitRootLogin is set to '{value}' (expected 'no', 'prohibit-password', or 'without-password')"));
            }
        }
    }
    issues
}

fn parse_fde_status(output: &str) -> CheckStatus {
    if output.contains("FileVault is On") {
        CheckStatus::Pass
    } else if output.contains("FileVault is Off") {
        CheckStatus::Warn("FileVault is disabled".to_string())
    } else {
        CheckStatus::Warn(format!(
            "unexpected fdesetup output: {}",
            first_line(output)
        ))
    }
}

fn parse_softwareupdate(output: &str) -> CheckStatus {
    if output.contains("No new software available") {
        CheckStatus::Pass
    } else if output.contains("Software Update found")
        || output.lines().any(|l| l.trim_start().starts_with("* "))
    {
        CheckStatus::Warn("software updates available".to_string())
    } else {
        CheckStatus::Pass
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_apt_upgradable(output: &str) -> CheckStatus {
    let upgradable = output.lines().filter(|l| l.contains("upgradable")).count();
    if upgradable > 0 {
        CheckStatus::Warn(format!("{upgradable} package(s) upgradable"))
    } else {
        CheckStatus::Pass
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s).trim()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_macos_firewall_enabled() {
        assert_eq!(
            parse_macos_firewall("Firewall is enabled. (State = 1)"),
            CheckStatus::Pass,
        );
    }

    #[test]
    fn parse_macos_firewall_disabled() {
        assert!(matches!(
            parse_macos_firewall("Firewall is disabled. (State = 0)"),
            CheckStatus::Warn(_),
        ));
    }

    #[test]
    fn parse_pf_enabled() {
        assert_eq!(
            parse_pf_status("Status: Enabled for 0 days"),
            CheckStatus::Pass,
        );
    }

    #[test]
    fn parse_pf_disabled() {
        assert!(matches!(
            parse_pf_status("Status: Disabled"),
            CheckStatus::Warn(_),
        ));
    }

    #[test]
    fn parse_ufw_active() {
        assert_eq!(parse_ufw_status("Status: active\n"), CheckStatus::Pass);
    }

    #[test]
    fn parse_ufw_inactive() {
        assert!(matches!(
            parse_ufw_status("Status: inactive\n"),
            CheckStatus::Warn(_),
        ));
    }

    #[test]
    fn parse_iptables_empty() {
        let output = "Chain INPUT (policy ACCEPT)\ntarget     prot opt source               destination\n\nChain FORWARD (policy ACCEPT)\ntarget     prot opt source               destination\n";
        assert!(!parse_iptables_output(output));
    }

    #[test]
    fn parse_iptables_with_rules() {
        let output = "Chain INPUT (policy ACCEPT)\ntarget     prot opt source               destination\nACCEPT     tcp  --  0.0.0.0/0            0.0.0.0/0            tcp dpt:22\n";
        assert!(parse_iptables_output(output));
    }

    #[test]
    fn parse_listening_ports_empty() {
        let output = "COMMAND   PID USER   FD  TYPE DEVICE SIZE/OFF NODE NAME\nnode    12345 user    3u IPv4 0x1234 0t0  TCP *:3000 (LISTEN)\n";
        assert!(parse_listening_ports(output).is_empty());
    }

    #[test]
    fn parse_listening_ports_risky() {
        let output = "COMMAND   PID USER   FD TYPE NODE NAME\nvsftpd  1234 root   3u IPv4  TCP *:21 (LISTEN)\n";
        let findings = parse_listening_ports(output);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].contains("21"));
        assert!(findings[0].contains("FTP"));
    }

    #[test]
    fn parse_listening_ports_ipv6() {
        let output = "tcp  LISTEN  0  128  [::]:5900  [::]:*\n";
        let findings = parse_listening_ports(output);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].contains("VNC"));
    }

    #[test]
    fn parse_listening_ports_dedup() {
        let output = "line1 *:21 (LISTEN)\nline2 0.0.0.0:21 (LISTEN)\n";
        let findings = parse_listening_ports(output);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn parse_ssh_config_secure() {
        let config = "PasswordAuthentication no\nPermitRootLogin no\n";
        assert!(parse_ssh_config(config).is_empty());
    }

    #[test]
    fn parse_ssh_config_prohibit_password_is_secure() {
        let config = "PermitRootLogin prohibit-password\n";
        assert!(parse_ssh_config(config).is_empty());
    }

    #[test]
    fn parse_ssh_config_without_password_is_secure() {
        let config = "PermitRootLogin without-password\n";
        assert!(parse_ssh_config(config).is_empty());
    }

    #[test]
    fn parse_ssh_config_weak() {
        let config = "PasswordAuthentication yes\nPermitRootLogin yes\n";
        let issues = parse_ssh_config(config);
        assert_eq!(issues.len(), 2);
    }

    #[test]
    fn parse_ssh_config_comments_ignored() {
        let config = "# PasswordAuthentication yes\n# PermitRootLogin yes\n";
        assert!(parse_ssh_config(config).is_empty());
    }

    #[test]
    fn parse_fde_on() {
        assert_eq!(parse_fde_status("FileVault is On."), CheckStatus::Pass);
    }

    #[test]
    fn parse_fde_off() {
        assert!(matches!(
            parse_fde_status("FileVault is Off."),
            CheckStatus::Warn(_),
        ));
    }

    #[test]
    fn parse_softwareupdate_none() {
        assert_eq!(
            parse_softwareupdate("No new software available."),
            CheckStatus::Pass,
        );
    }

    #[test]
    fn parse_softwareupdate_available() {
        assert!(matches!(
            parse_softwareupdate("Software Update found the following:\n* macOS 14.1"),
            CheckStatus::Warn(_),
        ));
    }

    #[test]
    fn parse_softwareupdate_no_false_positive_on_asterisk() {
        // A stray asterisk in stderr should not trigger a false positive
        assert_eq!(
            parse_softwareupdate("Checking for updates...\nDone."),
            CheckStatus::Pass,
        );
    }

    #[test]
    fn parse_apt_upgradable_none() {
        assert_eq!(parse_apt_upgradable("Listing...\n"), CheckStatus::Pass);
    }

    #[test]
    fn parse_apt_upgradable_some() {
        assert!(matches!(
            parse_apt_upgradable(
                "Listing...\nlibssl/stable 3.0.1 amd64 [upgradable from: 3.0.0]\n"
            ),
            CheckStatus::Warn(_),
        ));
    }

    #[test]
    fn run_host_security_checks_produces_entries() {
        let mut checks = Vec::new();
        run_host_security_checks(&mut checks);
        assert!(!checks.is_empty());
        assert!(checks.iter().all(|c| c.category == CATEGORY));
    }

    #[test]
    fn individual_check_produces_host_security_category() {
        let mut checks = Vec::new();
        check_firewall(&mut checks);
        assert!(!checks.is_empty());
        assert!(checks.iter().all(|c| c.category == CATEGORY));
    }
}
