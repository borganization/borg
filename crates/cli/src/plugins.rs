use anyhow::{Context, Result};

use borg_core::config::Config;

pub struct IntegrationDef {
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub credentials: &'static [CredentialSpec],
    pub is_channel: bool,
}

pub struct CredentialSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
}

pub const INTEGRATIONS: &[IntegrationDef] = &[
    IntegrationDef {
        name: "telegram",
        description: "Telegram Bot API",
        category: "messaging",
        credentials: &[CredentialSpec {
            key: "TELEGRAM_BOT_TOKEN",
            label: "Bot Token",
            help: "Get from @BotFather on Telegram",
        }],
        is_channel: true,
    },
    IntegrationDef {
        name: "slack",
        description: "Slack Bot API",
        category: "messaging",
        credentials: &[
            CredentialSpec {
                key: "SLACK_BOT_TOKEN",
                label: "Bot Token",
                help: "Get from Slack App settings > OAuth & Permissions",
            },
            CredentialSpec {
                key: "SLACK_SIGNING_SECRET",
                label: "Signing Secret",
                help: "Get from Slack App settings > Basic Information",
            },
        ],
        is_channel: true,
    },
    IntegrationDef {
        name: "twilio",
        description: "WhatsApp + SMS via Twilio",
        category: "messaging",
        credentials: &[
            CredentialSpec {
                key: "TWILIO_ACCOUNT_SID",
                label: "Account SID",
                help: "Get from Twilio Console",
            },
            CredentialSpec {
                key: "TWILIO_AUTH_TOKEN",
                label: "Auth Token",
                help: "Get from Twilio Console",
            },
            CredentialSpec {
                key: "TWILIO_PHONE_NUMBER",
                label: "Phone Number",
                help: "Your Twilio phone number (e.g. +1234567890)",
            },
        ],
        is_channel: true,
    },
    IntegrationDef {
        name: "gmail",
        description: "Gmail API",
        category: "email",
        credentials: &[CredentialSpec {
            key: "GMAIL_API_KEY",
            label: "API Key",
            help: "Get from Google Cloud Console",
        }],
        is_channel: false,
    },
    IntegrationDef {
        name: "outlook",
        description: "Outlook via Microsoft Graph",
        category: "email",
        credentials: &[CredentialSpec {
            key: "MS_GRAPH_TOKEN",
            label: "Access Token",
            help: "Get from Azure Portal > App Registrations",
        }],
        is_channel: false,
    },
    IntegrationDef {
        name: "google-calendar",
        description: "Google Calendar API",
        category: "productivity",
        credentials: &[CredentialSpec {
            key: "GOOGLE_CALENDAR_TOKEN",
            label: "Access Token",
            help: "Get from Google Cloud Console",
        }],
        is_channel: false,
    },
    IntegrationDef {
        name: "notion",
        description: "Notion API",
        category: "productivity",
        credentials: &[CredentialSpec {
            key: "NOTION_API_KEY",
            label: "API Key",
            help: "Get from notion.so/my-integrations",
        }],
        is_channel: false,
    },
    IntegrationDef {
        name: "linear",
        description: "Linear GraphQL API",
        category: "productivity",
        credentials: &[CredentialSpec {
            key: "LINEAR_API_KEY",
            label: "API Key",
            help: "Get from Linear Settings > API",
        }],
        is_channel: false,
    },
];

/// Look up an integration by name.
pub fn find_integration(name: &str) -> Option<&'static IntegrationDef> {
    INTEGRATIONS.iter().find(|i| i.name == name)
}

/// Check if an integration's credentials are already configured.
fn is_configured(def: &IntegrationDef, config: &Config) -> bool {
    if def.credentials.is_empty() {
        return true;
    }
    def.credentials.iter().all(|cred| {
        config.resolve_credential_or_env(cred.key).is_some()
            || borg_customizations::keychain::check(&format!("borg-{}", def.name), cred.key)
    })
}

/// Set up an integration: prompt for credentials, store in keychain, update config.
pub fn add_integration(name: &str) -> Result<()> {
    let def =
        find_integration(name).ok_or_else(|| anyhow::anyhow!("Unknown integration: {name}"))?;

    let config = Config::load().unwrap_or_default();

    if is_configured(def, &config) {
        println!("{} is already configured.", def.description);
        println!("Run `borg remove {name}` first to reconfigure.");
        return Ok(());
    }

    if def.credentials.is_empty() {
        println!("{} requires no credentials.", def.description);
        return Ok(());
    }

    let use_keychain = borg_customizations::keychain::available();
    let data_dir = Config::data_dir()?;
    let config_path = data_dir.join("config.toml");

    // Read existing config TOML (or start fresh)
    let mut config_str = if config_path.exists() {
        std::fs::read_to_string(&config_path)?
    } else {
        String::new()
    };

    let service_name = format!("borg-{name}");

    for cred in def.credentials {
        eprint!("{} ({}): ", cred.label, cred.help);
        let value = read_line_masked()?;

        if value.trim().is_empty() {
            anyhow::bail!("Credential cannot be empty");
        }

        if use_keychain {
            borg_customizations::keychain::store(&service_name, cred.key, value.trim())
                .with_context(|| format!("Failed to store {} in keychain", cred.key))?;

            // Add SecretRef to config
            let entry = if cfg!(target_os = "macos") {
                format!(
                    "\n[credentials]\n{} = {{ source = \"exec\", command = \"security\", args = [\"find-generic-password\", \"-s\", \"{}\", \"-a\", \"{}\", \"-w\"] }}\n",
                    cred.key, service_name, cred.key,
                )
            } else {
                format!(
                    "\n[credentials]\n{} = {{ source = \"exec\", command = \"secret-tool\", args = [\"lookup\", \"service\", \"{}\", \"account\", \"{}\"] }}\n",
                    cred.key, service_name, cred.key,
                )
            };

            if !config_str.contains(&format!("{} =", cred.key)) {
                append_credential_to_config(&mut config_str, cred.key, &entry);
            }
        } else {
            // Fall back to .env file
            let env_path = data_dir.join(".env");
            let mut env_content = if env_path.exists() {
                std::fs::read_to_string(&env_path)?
            } else {
                String::new()
            };
            // Remove existing entry for this key to avoid duplicates
            let prefix = format!("{}=", cred.key);
            let filtered: String = env_content
                .lines()
                .filter(|line| !line.starts_with(&prefix))
                .collect::<Vec<_>>()
                .join("\n");
            env_content = if filtered.is_empty() {
                String::new()
            } else {
                filtered + "\n"
            };
            env_content.push_str(&format!("{}={}\n", cred.key, value.trim()));
            std::fs::write(&env_path, &env_content)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    // Enable gateway if this is a channel integration
    if def.is_channel
        && !config_str.contains("gateway.enabled")
        && !config_str.contains("[gateway]")
    {
        config_str.push_str("\n[gateway]\nenabled = true\n");
    }

    std::fs::write(&config_path, &config_str)?;

    println!();
    println!("{} configured successfully.", def.description);
    if def.is_channel {
        println!("Gateway will start automatically when you run `borg`.");
    }

    Ok(())
}

/// Remove an integration's credentials.
pub fn remove_integration(name: &str) -> Result<()> {
    let def =
        find_integration(name).ok_or_else(|| anyhow::anyhow!("Unknown integration: {name}"))?;

    let service_name = format!("borg-{name}");

    for cred in def.credentials {
        borg_customizations::keychain::remove(&service_name, cred.key);
    }

    // Also clean up .env file if it exists
    let data_dir = Config::data_dir()?;
    let env_path = data_dir.join(".env");
    if env_path.exists() {
        let content = std::fs::read_to_string(&env_path)?;
        let filtered: Vec<&str> = content
            .lines()
            .filter(|line| {
                !def.credentials
                    .iter()
                    .any(|cred| line.starts_with(&format!("{}=", cred.key)))
            })
            .collect();
        std::fs::write(&env_path, filtered.join("\n") + "\n")?;
    }

    println!("{} credentials removed.", def.description);
    Ok(())
}

/// List all integrations with their configured/unconfigured status.
pub fn list_integrations() -> Result<()> {
    let config = Config::load().unwrap_or_default();

    let categories = ["messaging", "email", "productivity"];
    let category_labels = ["Messaging", "Email", "Productivity"];

    for (cat, label) in categories.iter().zip(category_labels.iter()) {
        let items: Vec<_> = INTEGRATIONS.iter().filter(|i| i.category == *cat).collect();
        if items.is_empty() {
            continue;
        }

        println!("{label}:");
        for def in &items {
            let configured = is_configured(def, &config);
            let icon = if configured { "\u{2713}" } else { "\u{2717}" };
            let hint = if configured {
                String::new()
            } else {
                format!("  (borg add {})", def.name)
            };
            println!("  {} {:<18} {}{}", icon, def.name, def.description, hint);
        }
        println!();
    }

    // iMessage note (macOS-only, no credentials needed)
    #[cfg(target_os = "macos")]
    println!("Note: iMessage is built-in on macOS (no setup needed).");

    Ok(())
}

/// Read a line from stdin (no echo for secrets isn't trivial without a TUI lib,
/// so we just read normally — the onboarding TUI handles masking properly).
fn read_line_masked() -> Result<String> {
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("Failed to read input")?;
    Ok(input.trim().to_string())
}

/// Append a credential entry to the config string, merging into existing [credentials] section if present.
fn append_credential_to_config(config_str: &mut String, key: &str, entry: &str) {
    if config_str.contains("[credentials]") {
        // Find the [credentials] section and append after it
        if let Some(pos) = config_str.find("[credentials]") {
            // Find the end of the line after [credentials]
            let after_header = pos + "[credentials]".len();
            let insert_pos = config_str[after_header..]
                .find('\n')
                .map(|p| after_header + p + 1)
                .unwrap_or(config_str.len());

            // Extract just the key = value part (skip the \n[credentials]\n prefix)
            let key_line = entry.lines().find(|l| l.starts_with(key)).unwrap_or(entry);
            config_str.insert_str(insert_pos, &format!("{key_line}\n"));
        }
    } else {
        config_str.push_str(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_known_integration() {
        assert!(find_integration("telegram").is_some());
        assert!(find_integration("slack").is_some());
        assert!(find_integration("gmail").is_some());
        assert!(find_integration("notion").is_some());
        assert!(find_integration("linear").is_some());
    }

    #[test]
    fn find_unknown_integration() {
        assert!(find_integration("nonexistent").is_none());
    }

    #[test]
    fn all_integrations_have_names_and_descriptions() {
        for def in INTEGRATIONS {
            assert!(!def.name.is_empty());
            assert!(!def.description.is_empty());
            assert!(!def.category.is_empty());
        }
    }

    #[test]
    fn channel_integrations_have_credentials() {
        for def in INTEGRATIONS {
            if def.is_channel && def.name != "imessage" {
                assert!(
                    !def.credentials.is_empty(),
                    "channel {} should have credentials",
                    def.name
                );
            }
        }
    }

    #[test]
    fn append_credential_new_section() {
        let mut config = "[llm]\nmodel = \"test\"\n".to_string();
        let entry = "\n[credentials]\nMY_KEY = { source = \"env\", var = \"MY_KEY\" }\n";
        append_credential_to_config(&mut config, "MY_KEY", entry);
        assert!(config.contains("[credentials]"));
        assert!(config.contains("MY_KEY"));
    }

    #[test]
    fn append_credential_existing_section() {
        let mut config = "[llm]\nmodel = \"test\"\n\n[credentials]\nOLD = \"old\"\n".to_string();
        let entry = "\n[credentials]\nNEW_KEY = { source = \"env\", var = \"NEW_KEY\" }\n";
        append_credential_to_config(&mut config, "NEW_KEY", entry);
        // Should only have one [credentials] section
        assert_eq!(config.matches("[credentials]").count(), 1);
        assert!(config.contains("NEW_KEY"));
        assert!(config.contains("OLD"));
    }
}
