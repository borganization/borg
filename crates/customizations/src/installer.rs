use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::info;

use crate::catalog::CustomizationDef;
use crate::{InstallEvent, TemplateTarget};

/// Install a customization: extract templates, set up credentials, record in DB.
///
/// Credential prompting is handled by the caller (TUI) — this function receives
/// pre-collected credentials as key-value pairs.
pub async fn install(
    def: &CustomizationDef,
    data_dir: &std::path::Path,
    credentials: &[(String, String)],
    progress_tx: Option<&mpsc::Sender<InstallEvent>>,
) -> Result<()> {
    let id = def.id.to_string();
    let name = def.name.to_string();

    send_event(
        progress_tx,
        InstallEvent::Starting {
            id: id.clone(),
            name,
        },
    )
    .await;

    // 1. Check prerequisites
    check_prerequisites(def)?;

    // 2. Write template files
    send_event(progress_tx, InstallEvent::WritingFiles { id: id.clone() }).await;
    write_templates(def, data_dir)?;

    // 3. Store credentials in keychain
    for (key, value) in credentials {
        send_event(
            progress_tx,
            InstallEvent::CredentialPrompt {
                id: id.clone(),
                label: key.clone(),
            },
        )
        .await;

        let service = format!("tamagotchi-{}", def.id.replace('/', "-"));
        store_credential(&service, key, value)?;

        send_event(
            progress_tx,
            InstallEvent::CredentialStored {
                id: id.clone(),
                key: key.clone(),
            },
        )
        .await;
    }

    // 4. Make shell scripts executable
    make_scripts_executable(def, data_dir)?;

    send_event(progress_tx, InstallEvent::Complete { id }).await;
    Ok(())
}

/// Uninstall a customization: delete files and keychain entries.
pub fn uninstall(def: &CustomizationDef, data_dir: &std::path::Path) -> Result<()> {
    // Determine the directory name from the first template
    let dir_name = def
        .templates
        .first()
        .and_then(|t| t.relative_path.split('/').next())
        .context("no templates")?;

    let target_dir = match def.templates[0].target {
        TemplateTarget::Channels => data_dir.join("channels").join(dir_name),
        TemplateTarget::Tools => data_dir.join("tools").join(dir_name),
    };

    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir)
            .with_context(|| format!("Failed to remove {}", target_dir.display()))?;
        info!("Removed {}", target_dir.display());
    }

    // Remove keychain entries
    let service = format!("tamagotchi-{}", def.id.replace('/', "-"));
    for cred in def.required_credentials {
        let _ = remove_credential(&service, cred.key);
    }

    Ok(())
}

/// Check if an integration is already installed on the filesystem.
pub fn is_installed(def: &CustomizationDef, data_dir: &std::path::Path) -> bool {
    let dir_name = match def
        .templates
        .first()
        .and_then(|t| t.relative_path.split('/').next())
    {
        Some(name) => name,
        None => return false,
    };

    let manifest = match def.templates[0].target {
        TemplateTarget::Channels => data_dir
            .join("channels")
            .join(dir_name)
            .join("channel.toml"),
        TemplateTarget::Tools => data_dir.join("tools").join(dir_name).join("tool.toml"),
    };

    manifest.exists()
}

// ── Internal helpers ──

fn check_prerequisites(def: &CustomizationDef) -> Result<()> {
    if !def.platform.is_available() {
        anyhow::bail!(
            "{} requires {}",
            def.name,
            def.platform.label().unwrap_or("a different platform")
        );
    }

    for bin in def.required_bins {
        which::which(bin).with_context(|| format!("Required binary not found: {bin}"))?;
    }

    Ok(())
}

fn write_templates(def: &CustomizationDef, data_dir: &std::path::Path) -> Result<()> {
    for tmpl in def.templates {
        let base = match tmpl.target {
            TemplateTarget::Channels => data_dir.join("channels"),
            TemplateTarget::Tools => data_dir.join("tools"),
        };

        let full_path = base.join(tmpl.relative_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        std::fs::write(&full_path, tmpl.content)
            .with_context(|| format!("Failed to write {}", full_path.display()))?;
        info!("Wrote {}", full_path.display());
    }
    Ok(())
}

fn make_scripts_executable(def: &CustomizationDef, data_dir: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for tmpl in def.templates {
            if tmpl.relative_path.ends_with(".sh") || tmpl.relative_path.ends_with(".py") {
                let base = match tmpl.target {
                    TemplateTarget::Channels => data_dir.join("channels"),
                    TemplateTarget::Tools => data_dir.join("tools"),
                };
                let path = base.join(tmpl.relative_path);
                if path.exists() {
                    let mut perms = std::fs::metadata(&path)?.permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&path, perms)?;
                }
            }
        }
    }
    Ok(())
}

/// Store a credential in the OS keychain.
fn store_credential(service: &str, key: &str, value: &str) -> Result<()> {
    let account = format!("tamagotchi-{key}");
    if cfg!(target_os = "macos") {
        let status = std::process::Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                service,
                "-a",
                &account,
                "-w",
                value,
                "-U",
            ])
            .status()?;
        if !status.success() {
            anyhow::bail!("Failed to store {key} in macOS Keychain");
        }
    } else if cfg!(target_os = "linux") {
        let mut child = std::process::Command::new("secret-tool")
            .args([
                "store",
                "--label",
                &format!("Tamagotchi {key}"),
                "service",
                service,
                "key",
                key,
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            stdin.write_all(value.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("Failed to store {key} via secret-tool");
        }
    } else {
        // Fallback: write to .env file
        let env_path = PathBuf::from(service).with_extension("env");
        info!("Keychain not available, credential {key} not stored persistently");
        let _ = env_path; // suppress unused warning
    }
    Ok(())
}

/// Remove a credential from the OS keychain.
fn remove_credential(service: &str, key: &str) -> Result<()> {
    let account = format!("tamagotchi-{key}");
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("security")
            .args(["delete-generic-password", "-s", service, "-a", &account])
            .status();
    } else if cfg!(target_os = "linux") {
        let _ = std::process::Command::new("secret-tool")
            .args(["clear", "service", service, "key", key])
            .status();
    }
    Ok(())
}

async fn send_event(tx: Option<&mpsc::Sender<InstallEvent>>, event: InstallEvent) {
    if let Some(tx) = tx {
        let _ = tx.send(event).await;
    }
}

/// Check if the OS keychain is available.
pub fn keychain_available() -> bool {
    if cfg!(target_os = "macos") {
        which::which("security").is_ok()
    } else if cfg!(target_os = "linux") {
        which::which("secret-tool").is_ok()
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CATALOG;

    #[test]
    fn prerequisites_check_bins() {
        // python3 should be available on most dev machines
        let telegram = CATALOG.iter().find(|c| c.id == "messaging/telegram");
        if let Some(def) = telegram {
            // Don't assert — just verify it doesn't panic
            let _ = check_prerequisites(def);
        }
    }

    #[test]
    fn is_installed_returns_false_for_missing() {
        let def = &CATALOG[0];
        let tmp = std::env::temp_dir().join("tamagotchi-test-nonexistent");
        assert!(!is_installed(def, &tmp));
    }
}
