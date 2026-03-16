use anyhow::Result;

/// Check if the OS keychain is available.
pub fn available() -> bool {
    if cfg!(target_os = "macos") {
        which::which("security").is_ok()
    } else if cfg!(target_os = "linux") {
        which::which("secret-tool").is_ok()
    } else {
        false
    }
}

/// Store a credential in the OS keychain.
pub fn store(service: &str, account: &str, value: &str) -> Result<()> {
    if cfg!(target_os = "macos") {
        let status = std::process::Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                service,
                "-a",
                account,
                "-w",
                value,
                "-U",
            ])
            .status()?;
        if !status.success() {
            anyhow::bail!(
                "Failed to store in macOS Keychain (service={service}, account={account})"
            );
        }
    } else if cfg!(target_os = "linux") {
        let mut child = std::process::Command::new("secret-tool")
            .args([
                "store",
                "--label",
                &format!("Tamagotchi {account}"),
                "service",
                service,
                "account",
                account,
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            stdin.write_all(value.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("Failed to store via secret-tool (service={service}, account={account})");
        }
    } else {
        anyhow::bail!("No keychain available on this platform");
    }
    Ok(())
}

/// Remove a credential from the OS keychain. Errors are silently ignored.
pub fn remove(service: &str, account: &str) {
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("security")
            .args(["delete-generic-password", "-s", service, "-a", account])
            .status();
    } else if cfg!(target_os = "linux") {
        let _ = std::process::Command::new("secret-tool")
            .args(["clear", "service", service, "account", account])
            .status();
    }
}

/// Check if a credential exists in the OS keychain.
pub fn check(service: &str, account: &str) -> bool {
    if cfg!(target_os = "macos") {
        std::process::Command::new("security")
            .args(["find-generic-password", "-s", service, "-a", account, "-w"])
            .output()
            .is_ok_and(|o| o.status.success())
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("secret-tool")
            .args(["lookup", "service", service, "account", account])
            .output()
            .is_ok_and(|o| o.status.success())
    } else {
        false
    }
}
