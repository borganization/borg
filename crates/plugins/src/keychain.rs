use anyhow::Result;

/// Check if the OS keychain is available.
pub fn available() -> bool {
    if cfg!(target_os = "macos") {
        which::which("security").is_ok()
    } else if cfg!(target_os = "linux") {
        which::which("secret-tool").is_ok()
    } else if cfg!(target_os = "windows") {
        which::which("cmdkey").is_ok()
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
                &format!("Borg {account}"),
                "service",
                service,
                "account",
                account,
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin.write_all(value.as_bytes())?;
            drop(stdin); // close stdin so secret-tool sees EOF
        }
        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("Failed to store via secret-tool (service={service}, account={account})");
        }
    } else if cfg!(target_os = "windows") {
        let target = format!("{service}/{account}");
        let status = std::process::Command::new("cmdkey")
            .arg(format!("/generic:{target}"))
            .arg(format!("/user:{account}"))
            .arg(format!("/pass:{value}"))
            .status()?;
        if !status.success() {
            anyhow::bail!("Failed to store in Windows Credential Manager (target={target})");
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
    } else if cfg!(target_os = "windows") {
        let target = format!("{service}/{account}");
        let _ = std::process::Command::new("cmdkey")
            .arg(format!("/delete:{target}"))
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
    } else if cfg!(target_os = "windows") {
        let target = format!("{service}/{account}");
        std::process::Command::new("cmdkey")
            .arg(format!("/list:{target}"))
            .output()
            .is_ok_and(|o| {
                o.status.success() && !String::from_utf8_lossy(&o.stdout).contains("not found")
            })
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_returns_bool() {
        // On macOS CI, `security` binary should exist; on Linux, `secret-tool` may not.
        // This test just ensures the function doesn't panic.
        let _ = available();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn available_true_on_macos() {
        // macOS always has the `security` binary
        assert!(available());
    }

    #[test]
    fn check_nonexistent_credential_returns_false() {
        // A random service/account pair should not exist
        let result = check("borg-test-nonexistent-svc-12345", "nonexistent-account-xyz");
        assert!(!result);
    }

    #[test]
    fn remove_nonexistent_does_not_panic() {
        // Removing a credential that doesn't exist should silently succeed
        remove("borg-test-nonexistent-svc-12345", "nonexistent-account-xyz");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn store_and_check_round_trip() {
        let service = "borg-keychain-test";
        let account = "test-round-trip";
        let value = "test-secret-value";

        // Clean up first in case of prior failed run
        remove(service, account);

        // Store
        store(service, account, value).expect("store should succeed on macOS");

        // Check exists
        assert!(
            check(service, account),
            "credential should exist after store"
        );

        // Clean up
        remove(service, account);
        assert!(
            !check(service, account),
            "credential should not exist after remove"
        );
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    #[test]
    fn store_fails_on_unsupported_platform() {
        let result = store("svc", "acct", "val");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No keychain available"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn available_true_on_windows() {
        // cmdkey.exe is always present on Windows
        assert!(available());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_store_and_check_round_trip() {
        let service = "borg-keychain-test";
        let account = "test-round-trip-win";
        let value = "test-secret-value";

        // Clean up first in case of prior failed run
        remove(service, account);

        // Store
        store(service, account, value).expect("store should succeed on Windows");

        // Check exists
        assert!(
            check(service, account),
            "credential should exist after store"
        );

        // Clean up
        remove(service, account);
        assert!(
            !check(service, account),
            "credential should not exist after remove"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_check_nonexistent_returns_false() {
        let result = check(
            "borg-test-nonexistent-svc-win-12345",
            "nonexistent-account-xyz",
        );
        assert!(!result);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_remove_nonexistent_does_not_panic() {
        remove(
            "borg-test-nonexistent-svc-win-12345",
            "nonexistent-account-xyz",
        );
    }
}
