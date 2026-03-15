use crate::config::Config;
use crate::db::Database;
use crate::skills::load_all_skills;

#[derive(Debug, Clone, PartialEq)]
pub enum CheckStatus {
    Pass,
    Warn(String),
    Fail(String),
}

#[derive(Debug, Clone)]
pub struct DiagnosticCheck {
    pub category: &'static str,
    pub name: String,
    pub status: CheckStatus,
}

#[derive(Debug, Clone)]
pub struct DiagnosticReport {
    pub checks: Vec<DiagnosticCheck>,
}

impl DiagnosticReport {
    pub fn format(&self) -> String {
        let mut output = String::from("Tamagotchi Doctor\n─────────────────\n");
        let mut current_category = "";
        let mut pass_count = 0;
        let mut warn_count = 0;
        let mut fail_count = 0;

        for check in &self.checks {
            if check.category != current_category {
                current_category = check.category;
                output.push_str(&format!("\n{current_category}\n"));
            }
            match &check.status {
                CheckStatus::Pass => {
                    output.push_str(&format!("  ✓ {}\n", check.name));
                    pass_count += 1;
                }
                CheckStatus::Warn(msg) => {
                    output.push_str(&format!("  ⚠ {} — {msg}\n", check.name));
                    warn_count += 1;
                }
                CheckStatus::Fail(msg) => {
                    output.push_str(&format!("  ✗ {} — {msg}\n", check.name));
                    fail_count += 1;
                }
            }
        }

        output.push_str(&format!(
            "\nSummary: {pass_count} passed, {warn_count} warning(s), {fail_count} failed"
        ));
        output
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        let mut pass = 0;
        let mut warn = 0;
        let mut fail = 0;
        for check in &self.checks {
            match &check.status {
                CheckStatus::Pass => pass += 1,
                CheckStatus::Warn(_) => warn += 1,
                CheckStatus::Fail(_) => fail += 1,
            }
        }
        (pass, warn, fail)
    }
}

pub fn run_diagnostics(config: &Config) -> DiagnosticReport {
    let mut checks = Vec::new();

    // Config checks
    check_config(&mut checks);

    // Provider checks
    check_provider(config, &mut checks);

    // Secrets audit
    check_secrets(config, &mut checks);

    // Sandbox checks
    check_sandbox(&mut checks);

    // Tools checks
    check_tools(&mut checks);

    // Skills checks
    check_skills(config, &mut checks);

    // Memory checks
    check_memory(&mut checks);

    // Data directory checks
    check_data_dir(&mut checks);

    // Gateway checks
    check_gateway(config, &mut checks);

    // Budget checks
    check_budget(config, &mut checks);

    // Customizations checks
    check_customizations(&mut checks);

    // Host security checks
    if config.security.host_audit {
        crate::host_audit::run_host_security_checks(&mut checks);
    }

    DiagnosticReport { checks }
}

fn check_config(checks: &mut Vec<DiagnosticCheck>) {
    match Config::data_dir() {
        Ok(data_dir) => {
            let config_path = data_dir.join("config.toml");
            if config_path.exists() {
                checks.push(DiagnosticCheck {
                    category: "Config",
                    name: "config.toml exists".to_string(),
                    status: CheckStatus::Pass,
                });
                match Config::load() {
                    Ok(_) => {
                        checks.push(DiagnosticCheck {
                            category: "Config",
                            name: "config.toml valid".to_string(),
                            status: CheckStatus::Pass,
                        });
                    }
                    Err(e) => {
                        checks.push(DiagnosticCheck {
                            category: "Config",
                            name: "config.toml valid".to_string(),
                            status: CheckStatus::Fail(format!("{e}")),
                        });
                    }
                }
            } else {
                checks.push(DiagnosticCheck {
                    category: "Config",
                    name: "config.toml exists".to_string(),
                    status: CheckStatus::Warn("not found, using defaults".to_string()),
                });
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Config",
                name: "data directory".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }
}

fn check_provider(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    match config.resolve_provider() {
        Ok((provider, _key)) => {
            checks.push(DiagnosticCheck {
                category: "Provider",
                name: "API key set".to_string(),
                status: CheckStatus::Pass,
            });
            checks.push(DiagnosticCheck {
                category: "Provider",
                name: format!("Provider: {provider}"),
                status: CheckStatus::Pass,
            });
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Provider",
                name: "API key set".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }

    checks.push(DiagnosticCheck {
        category: "Provider",
        name: "API connectivity".to_string(),
        status: CheckStatus::Warn("skipped (use --online for live check)".to_string()),
    });
}

fn check_secrets(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    // Check if api_key SecretRef is configured (more secure than plaintext .env)
    if config.llm.api_key.is_some() {
        checks.push(DiagnosticCheck {
            category: "Secrets",
            name: "API key via SecretRef".to_string(),
            status: CheckStatus::Pass,
        });
    }

    // Check for plaintext API keys in .env file
    if let Ok(data_dir) = Config::data_dir() {
        let env_path = data_dir.join(".env");
        if env_path.exists() {
            match std::fs::read_to_string(&env_path) {
                Ok(contents) => {
                    let has_plaintext_keys = contents.lines().any(|line| {
                        let trimmed = line.trim();
                        !trimmed.is_empty()
                            && !trimmed.starts_with('#')
                            && trimmed.contains('=')
                            && (trimmed.contains("API_KEY") || trimmed.contains("api_key"))
                    });

                    if has_plaintext_keys {
                        let hint = if config.llm.api_key.is_some() {
                            "plaintext .env still present — consider removing it"
                        } else {
                            "plaintext API keys in .env — consider using SecretRef in config.toml (e.g., api_key = { source = \"exec\", command = \"security\", args = [\"find-generic-password\", \"-s\", \"tamagotchi\", \"-w\"] })"
                        };
                        checks.push(DiagnosticCheck {
                            category: "Secrets",
                            name: "plaintext keys in .env".to_string(),
                            status: CheckStatus::Warn(hint.to_string()),
                        });
                    } else {
                        checks.push(DiagnosticCheck {
                            category: "Secrets",
                            name: "no plaintext API keys in .env".to_string(),
                            status: CheckStatus::Pass,
                        });
                    }
                }
                Err(_) => {
                    checks.push(DiagnosticCheck {
                        category: "Secrets",
                        name: ".env readable".to_string(),
                        status: CheckStatus::Warn("could not read .env file".to_string()),
                    });
                }
            }

            // Check file permissions on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if let Ok(meta) = std::fs::metadata(&env_path) {
                    let mode = meta.mode() & 0o777;
                    if mode & 0o077 != 0 {
                        checks.push(DiagnosticCheck {
                            category: "Secrets",
                            name: ".env file permissions".to_string(),
                            status: CheckStatus::Warn(format!(
                                "permissions are {mode:04o} — should be 0600 or stricter"
                            )),
                        });
                    } else {
                        checks.push(DiagnosticCheck {
                            category: "Secrets",
                            name: ".env file permissions".to_string(),
                            status: CheckStatus::Pass,
                        });
                    }
                }
            }
        }
    }

    // Check for multi-key fallback
    if !config.llm.api_keys.is_empty() {
        let resolved_count = config
            .llm
            .api_keys
            .iter()
            .filter(|sr| sr.resolve().is_ok())
            .count();
        checks.push(DiagnosticCheck {
            category: "Secrets",
            name: format!(
                "multi-key fallback: {resolved_count}/{} keys resolvable",
                config.llm.api_keys.len()
            ),
            status: if resolved_count > 0 {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn("no fallback keys could be resolved".to_string())
            },
        });
    }
}

fn check_sandbox(checks: &mut Vec<DiagnosticCheck>) {
    if cfg!(target_os = "macos") {
        match which::which("sandbox-exec") {
            Ok(_) => {
                checks.push(DiagnosticCheck {
                    category: "Sandbox",
                    name: "sandbox-exec available".to_string(),
                    status: CheckStatus::Pass,
                });
            }
            Err(_) => {
                checks.push(DiagnosticCheck {
                    category: "Sandbox",
                    name: "sandbox-exec available".to_string(),
                    status: CheckStatus::Warn("not found".to_string()),
                });
            }
        }
    } else if cfg!(target_os = "linux") {
        match which::which("bwrap") {
            Ok(_) => {
                checks.push(DiagnosticCheck {
                    category: "Sandbox",
                    name: "bwrap available".to_string(),
                    status: CheckStatus::Pass,
                });
            }
            Err(_) => {
                checks.push(DiagnosticCheck {
                    category: "Sandbox",
                    name: "bwrap available".to_string(),
                    status: CheckStatus::Warn("not found — sandboxing disabled".to_string()),
                });
            }
        }
    } else {
        checks.push(DiagnosticCheck {
            category: "Sandbox",
            name: "sandbox support".to_string(),
            status: CheckStatus::Warn("not available on this platform".to_string()),
        });
    }
}

fn check_tools(checks: &mut Vec<DiagnosticCheck>) {
    match Config::tools_dir() {
        Ok(tools_dir) => {
            if tools_dir.exists() {
                checks.push(DiagnosticCheck {
                    category: "Tools",
                    name: "tools directory exists".to_string(),
                    status: CheckStatus::Pass,
                });
            } else {
                checks.push(DiagnosticCheck {
                    category: "Tools",
                    name: "tools directory exists".to_string(),
                    status: CheckStatus::Warn("not found".to_string()),
                });
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Tools",
                name: "tools directory".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }

    match tamagotchi_tools::registry::ToolRegistry::new() {
        Ok(registry) => {
            let tools = registry.list_tools();
            checks.push(DiagnosticCheck {
                category: "Tools",
                name: format!("tool manifests valid ({} tools)", tools.len()),
                status: CheckStatus::Pass,
            });
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Tools",
                name: "tool manifests valid".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }
}

fn check_skills(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    let resolved_creds = config.resolve_credentials();
    match load_all_skills(&resolved_creds) {
        Ok(skills) => {
            let available = skills.iter().filter(|s| s.available).count();
            let total = skills.len();
            checks.push(DiagnosticCheck {
                category: "Skills",
                name: format!("{available}/{total} skills available"),
                status: CheckStatus::Pass,
            });

            let missing: Vec<String> = skills
                .iter()
                .filter(|s| !s.available)
                .map(|s| s.manifest.name.clone())
                .collect();
            if !missing.is_empty() {
                checks.push(DiagnosticCheck {
                    category: "Skills",
                    name: format!("missing requirements: {}", missing.join(", ")),
                    status: CheckStatus::Warn("some skills unavailable".to_string()),
                });
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Skills",
                name: "skills loading".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }
}

fn check_memory(checks: &mut Vec<DiagnosticCheck>) {
    match Config::memory_index_path() {
        Ok(path) => {
            if path.exists() {
                checks.push(DiagnosticCheck {
                    category: "Memory",
                    name: "MEMORY.md exists".to_string(),
                    status: CheckStatus::Pass,
                });
            } else {
                checks.push(DiagnosticCheck {
                    category: "Memory",
                    name: "MEMORY.md exists".to_string(),
                    status: CheckStatus::Warn("not found".to_string()),
                });
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Memory",
                name: "MEMORY.md".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }

    match Config::soul_path() {
        Ok(path) => {
            if path.exists() {
                checks.push(DiagnosticCheck {
                    category: "Memory",
                    name: "SOUL.md exists".to_string(),
                    status: CheckStatus::Pass,
                });
            } else {
                checks.push(DiagnosticCheck {
                    category: "Memory",
                    name: "SOUL.md exists".to_string(),
                    status: CheckStatus::Warn("not found".to_string()),
                });
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Memory",
                name: "SOUL.md".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }
}

fn check_data_dir(checks: &mut Vec<DiagnosticCheck>) {
    match Config::data_dir() {
        Ok(data_dir) => {
            if data_dir.exists() {
                // Test writability
                let test_file = data_dir.join(".doctor_write_test");
                match std::fs::write(&test_file, "test") {
                    Ok(()) => {
                        let _ = std::fs::remove_file(&test_file);
                        checks.push(DiagnosticCheck {
                            category: "Data",
                            name: "data directory writable".to_string(),
                            status: CheckStatus::Pass,
                        });
                    }
                    Err(e) => {
                        checks.push(DiagnosticCheck {
                            category: "Data",
                            name: "data directory writable".to_string(),
                            status: CheckStatus::Fail(format!("{e}")),
                        });
                    }
                }
            } else {
                checks.push(DiagnosticCheck {
                    category: "Data",
                    name: "data directory exists".to_string(),
                    status: CheckStatus::Warn(
                        "~/.tamagotchi not found — run `tamagotchi init`".to_string(),
                    ),
                });
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Data",
                name: "data directory".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }

    match Database::open() {
        Ok(_) => {
            checks.push(DiagnosticCheck {
                category: "Data",
                name: "database accessible".to_string(),
                status: CheckStatus::Pass,
            });
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Data",
                name: "database accessible".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }
}

fn check_gateway(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    if !config.gateway.enabled {
        checks.push(DiagnosticCheck {
            category: "Gateway",
            name: "gateway enabled".to_string(),
            status: CheckStatus::Warn("disabled".to_string()),
        });
        return;
    }

    checks.push(DiagnosticCheck {
        category: "Gateway",
        name: format!(
            "gateway config: {}:{}",
            config.gateway.host, config.gateway.port
        ),
        status: CheckStatus::Pass,
    });

    match Config::channels_dir() {
        Ok(channels_dir) => {
            if channels_dir.exists() {
                let mut count = 0;
                let mut errors = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&channels_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            let manifest_path = path.join("channel.toml");
                            if manifest_path.exists() {
                                match std::fs::read_to_string(&manifest_path) {
                                    Ok(content) => match toml::from_str::<toml::Value>(&content) {
                                        Ok(_) => count += 1,
                                        Err(e) => {
                                            errors.push(format!("{}: {e}", manifest_path.display()))
                                        }
                                    },
                                    Err(e) => {
                                        errors.push(format!("{}: {e}", manifest_path.display()))
                                    }
                                }
                            }
                        }
                    }
                }

                checks.push(DiagnosticCheck {
                    category: "Gateway",
                    name: format!("{count} channel(s) found"),
                    status: CheckStatus::Pass,
                });

                for error in errors {
                    checks.push(DiagnosticCheck {
                        category: "Gateway",
                        name: "channel manifest".to_string(),
                        status: CheckStatus::Fail(error),
                    });
                }
            } else {
                checks.push(DiagnosticCheck {
                    category: "Gateway",
                    name: "channels directory".to_string(),
                    status: CheckStatus::Warn("not found".to_string()),
                });
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Gateway",
                name: "channels directory".to_string(),
                status: CheckStatus::Fail(format!("{e}")),
            });
        }
    }
}

fn check_budget(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    let limit = config.budget.monthly_token_limit;
    if limit == 0 {
        checks.push(DiagnosticCheck {
            category: "Budget",
            name: "monthly limit".to_string(),
            status: CheckStatus::Warn("unlimited (no budget set)".to_string()),
        });
        return;
    }

    match Database::open() {
        Ok(db) => match db.monthly_token_total() {
            Ok(used) => {
                let pct = (used as f64 / limit as f64 * 100.0) as u64;
                let name = format!("monthly usage: {used}/{limit} ({pct}%)");
                if used >= limit {
                    checks.push(DiagnosticCheck {
                        category: "Budget",
                        name,
                        status: CheckStatus::Fail("budget exceeded".to_string()),
                    });
                } else if (used as f64 / limit as f64) >= config.budget.warning_threshold {
                    checks.push(DiagnosticCheck {
                        category: "Budget",
                        name,
                        status: CheckStatus::Warn("approaching limit".to_string()),
                    });
                } else {
                    checks.push(DiagnosticCheck {
                        category: "Budget",
                        name,
                        status: CheckStatus::Pass,
                    });
                }
            }
            Err(e) => {
                checks.push(DiagnosticCheck {
                    category: "Budget",
                    name: "monthly usage".to_string(),
                    status: CheckStatus::Warn(format!("could not read: {e}")),
                });
            }
        },
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Budget",
                name: "monthly usage".to_string(),
                status: CheckStatus::Warn(format!("database unavailable: {e}")),
            });
        }
    }
}

fn check_customizations(checks: &mut Vec<DiagnosticCheck>) {
    match Database::open() {
        Ok(db) => match db.list_customizations() {
            Ok(customizations) => {
                if customizations.is_empty() {
                    checks.push(DiagnosticCheck {
                        category: "Customizations",
                        name: "installed integrations".to_string(),
                        status: CheckStatus::Warn("none installed (use /customize)".to_string()),
                    });
                } else {
                    let verified = customizations
                        .iter()
                        .filter(|c| c.verified_at.is_some())
                        .count();
                    checks.push(DiagnosticCheck {
                        category: "Customizations",
                        name: format!(
                            "{} integration(s) installed, {verified} verified",
                            customizations.len()
                        ),
                        status: CheckStatus::Pass,
                    });

                    for c in &customizations {
                        let status = if c.verified_at.is_some() {
                            CheckStatus::Pass
                        } else {
                            CheckStatus::Warn("not verified".to_string())
                        };
                        checks.push(DiagnosticCheck {
                            category: "Customizations",
                            name: format!("{} ({})", c.name, c.kind),
                            status,
                        });

                        // Integrity check
                        if let Ok(data_dir) = Config::data_dir() {
                            if let Ok(result) =
                                crate::integrity::verify_integrity(&db, &c.id, &data_dir)
                            {
                                let integrity_status = if result.ok {
                                    CheckStatus::Pass
                                } else {
                                    let mut issues = Vec::new();
                                    if !result.tampered.is_empty() {
                                        issues.push(format!(
                                            "tampered: {}",
                                            result.tampered.join(", ")
                                        ));
                                    }
                                    if !result.missing.is_empty() {
                                        issues.push(format!(
                                            "missing: {}",
                                            result.missing.join(", ")
                                        ));
                                    }
                                    CheckStatus::Fail(issues.join("; "))
                                };
                                checks.push(DiagnosticCheck {
                                    category: "Customizations",
                                    name: format!("{} file integrity", c.name),
                                    status: integrity_status,
                                });
                            }
                        }
                    }
                }
            }
            Err(e) => {
                checks.push(DiagnosticCheck {
                    category: "Customizations",
                    name: "customizations table".to_string(),
                    status: CheckStatus::Warn(format!("could not query: {e}")),
                });
            }
        },
        Err(e) => {
            checks.push(DiagnosticCheck {
                category: "Customizations",
                name: "database".to_string(),
                status: CheckStatus::Warn(format!("unavailable: {e}")),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_format_output() {
        let report = DiagnosticReport {
            checks: vec![
                DiagnosticCheck {
                    category: "Config",
                    name: "config.toml exists".to_string(),
                    status: CheckStatus::Pass,
                },
                DiagnosticCheck {
                    category: "Config",
                    name: "config.toml valid".to_string(),
                    status: CheckStatus::Pass,
                },
                DiagnosticCheck {
                    category: "Provider",
                    name: "API key set".to_string(),
                    status: CheckStatus::Fail("not found".to_string()),
                },
                DiagnosticCheck {
                    category: "Sandbox",
                    name: "sandbox-exec".to_string(),
                    status: CheckStatus::Warn("not available".to_string()),
                },
            ],
        };
        let output = report.format();
        assert!(output.contains("Tamagotchi Doctor"));
        assert!(output.contains("✓ config.toml exists"));
        assert!(output.contains("✗ API key set"));
        assert!(output.contains("⚠ sandbox-exec"));
        assert!(output.contains("2 passed, 1 warning(s), 1 failed"));
    }

    #[test]
    fn report_counts() {
        let report = DiagnosticReport {
            checks: vec![
                DiagnosticCheck {
                    category: "Test",
                    name: "pass".to_string(),
                    status: CheckStatus::Pass,
                },
                DiagnosticCheck {
                    category: "Test",
                    name: "warn".to_string(),
                    status: CheckStatus::Warn("w".to_string()),
                },
                DiagnosticCheck {
                    category: "Test",
                    name: "fail".to_string(),
                    status: CheckStatus::Fail("f".to_string()),
                },
            ],
        };
        assert_eq!(report.counts(), (1, 1, 1));
    }

    #[test]
    fn empty_report() {
        let report = DiagnosticReport { checks: vec![] };
        let output = report.format();
        assert!(output.contains("0 passed"));
        assert_eq!(report.counts(), (0, 0, 0));
    }

    #[test]
    fn run_diagnostics_produces_checks() {
        let config = Config::default();
        let report = run_diagnostics(&config);
        // Should always produce at least the config and provider checks
        assert!(!report.checks.is_empty());
        // Should have at least config, provider, sandbox, tools, skills, memory, data categories
        let categories: std::collections::HashSet<&str> =
            report.checks.iter().map(|c| c.category).collect();
        assert!(categories.contains("Config"));
        assert!(categories.contains("Provider"));
    }
}
