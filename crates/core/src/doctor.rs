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

    // Sandbox checks
    check_sandbox(&mut checks);

    // Tools checks
    check_tools(&mut checks);

    // Skills checks
    check_skills(&mut checks);

    // Memory checks
    check_memory(&mut checks);

    // Data directory checks
    check_data_dir(&mut checks);

    // Budget checks
    check_budget(config, &mut checks);

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

fn check_skills(checks: &mut Vec<DiagnosticCheck>) {
    match load_all_skills() {
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
