use crate::config::Config;
use crate::db::Database;
use crate::provider::Provider;
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

impl DiagnosticCheck {
    pub fn pass(category: &'static str, name: impl Into<String>) -> Self {
        Self {
            category,
            name: name.into(),
            status: CheckStatus::Pass,
        }
    }

    pub fn warn(category: &'static str, name: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            category,
            name: name.into(),
            status: CheckStatus::Warn(msg.into()),
        }
    }

    pub fn fail(category: &'static str, name: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            category,
            name: name.into(),
            status: CheckStatus::Fail(msg.into()),
        }
    }

    pub fn with_status(
        category: &'static str,
        name: impl Into<String>,
        status: CheckStatus,
    ) -> Self {
        Self {
            category,
            name: name.into(),
            status,
        }
    }

    /// Create a check from a Result: pass on Ok, fail on Err.
    pub fn from_result<T, E: std::fmt::Display>(
        category: &'static str,
        name: impl Into<String>,
        result: Result<T, E>,
    ) -> Self {
        match result {
            Ok(_) => Self::pass(category, name),
            Err(e) => Self::fail(category, name, format!("{e}")),
        }
    }
}

pub struct DiagnosticReport {
    pub checks: Vec<DiagnosticCheck>,
}

impl DiagnosticReport {
    pub fn format(&self) -> String {
        let mut output = String::from("Borg Doctor\n───────────\n");
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

    // Embeddings checks
    check_embeddings(config, &mut checks);

    // Data directory checks
    check_data_dir(&mut checks);

    // Gateway checks
    check_gateway(config, &mut checks);

    // Budget checks
    check_budget(config, &mut checks);

    // Plugins checks
    check_plugins(&mut checks);

    // Browser checks
    check_browser(config, &mut checks);

    // Agent config checks
    check_agents(config, &mut checks);

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
                checks.push(DiagnosticCheck::pass("Config", "config.toml exists"));
                match Config::load() {
                    Ok(_) => {
                        checks.push(DiagnosticCheck::pass("Config", "config.toml valid"));
                    }
                    Err(e) => {
                        checks.push(DiagnosticCheck::fail(
                            "Config",
                            "config.toml valid",
                            format!("{e}"),
                        ));
                    }
                }
            } else {
                checks.push(DiagnosticCheck::warn(
                    "Config",
                    "config.toml exists",
                    "not found, using defaults",
                ));
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck::fail(
                "Config",
                "data directory",
                format!("{e}"),
            ));
        }
    }
}

fn check_provider(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    match config.resolve_provider() {
        Ok((provider, _key)) => {
            if provider.requires_api_key() {
                checks.push(DiagnosticCheck::pass("Provider", "API key set"));
            } else if Provider::ollama_available() {
                checks.push(DiagnosticCheck::pass("Provider", "Ollama server reachable"));
            } else {
                checks.push(DiagnosticCheck::warn(
                    "Provider",
                    "Ollama server",
                    "not reachable at localhost:11434 — run `ollama serve`",
                ));
            }
            checks.push(DiagnosticCheck::pass(
                "Provider",
                format!("Provider: {provider}"),
            ));
        }
        Err(e) => {
            checks.push(DiagnosticCheck::fail(
                "Provider",
                "API key set",
                format!("{e}"),
            ));
        }
    }

    checks.push(DiagnosticCheck::warn(
        "Provider",
        "API connectivity",
        "skipped (use --online for live check)",
    ));
}

fn check_secrets(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    if config.llm.api_key.is_some() {
        checks.push(DiagnosticCheck::pass("Secrets", "API key via SecretRef"));
    }

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
                            "plaintext API keys in .env — consider using SecretRef in config.toml (e.g., api_key = { source = \"exec\", command = \"security\", args = [\"find-generic-password\", \"-s\", \"borg\", \"-w\"] })"
                        };
                        checks.push(DiagnosticCheck::warn(
                            "Secrets",
                            "plaintext keys in .env",
                            hint,
                        ));
                    } else {
                        checks.push(DiagnosticCheck::pass(
                            "Secrets",
                            "no plaintext API keys in .env",
                        ));
                    }
                }
                Err(_) => {
                    checks.push(DiagnosticCheck::warn(
                        "Secrets",
                        ".env readable",
                        "could not read .env file",
                    ));
                }
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if let Ok(meta) = std::fs::metadata(&env_path) {
                    let mode = meta.mode() & 0o777;
                    if mode & 0o077 != 0 {
                        checks.push(DiagnosticCheck::warn(
                            "Secrets",
                            ".env file permissions",
                            format!("permissions are {mode:04o} — should be 0600 or stricter"),
                        ));
                    } else {
                        checks.push(DiagnosticCheck::pass("Secrets", ".env file permissions"));
                    }
                }
            }
        }
    }

    if !config.llm.api_keys.is_empty() {
        let resolved_count = config
            .llm
            .api_keys
            .iter()
            .filter(|sr| sr.resolve().is_ok())
            .count();
        let name = format!(
            "multi-key fallback: {resolved_count}/{} keys resolvable",
            config.llm.api_keys.len()
        );
        if resolved_count > 0 {
            checks.push(DiagnosticCheck::pass("Secrets", name));
        } else {
            checks.push(DiagnosticCheck::warn(
                "Secrets",
                name,
                "no fallback keys could be resolved",
            ));
        }
    }
}

fn check_sandbox(checks: &mut Vec<DiagnosticCheck>) {
    if cfg!(target_os = "macos") {
        if which::which("sandbox-exec").is_ok() {
            checks.push(DiagnosticCheck::pass("Sandbox", "sandbox-exec available"));
        } else {
            checks.push(DiagnosticCheck::warn(
                "Sandbox",
                "sandbox-exec available",
                "not found",
            ));
        }
    } else if cfg!(target_os = "linux") {
        if which::which("bwrap").is_ok() {
            checks.push(DiagnosticCheck::pass("Sandbox", "bwrap available"));
        } else {
            checks.push(DiagnosticCheck::warn(
                "Sandbox",
                "bwrap available",
                "not found — sandboxing disabled",
            ));
        }
    } else {
        checks.push(DiagnosticCheck::warn(
            "Sandbox",
            "sandbox support",
            "not available on this platform",
        ));
    }
}

fn check_tools(checks: &mut Vec<DiagnosticCheck>) {
    match Config::tools_dir() {
        Ok(tools_dir) => {
            if tools_dir.exists() {
                checks.push(DiagnosticCheck::pass("Tools", "tools directory exists"));
            } else {
                checks.push(DiagnosticCheck::warn(
                    "Tools",
                    "tools directory exists",
                    "not found",
                ));
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck::fail(
                "Tools",
                "tools directory",
                format!("{e}"),
            ));
        }
    }

    match borg_tools::registry::ToolRegistry::new() {
        Ok(registry) => {
            let tools = registry.list_tools();
            checks.push(DiagnosticCheck::pass(
                "Tools",
                format!("tool manifests valid ({} tools)", tools.len()),
            ));
        }
        Err(e) => {
            checks.push(DiagnosticCheck::fail(
                "Tools",
                "tool manifests valid",
                format!("{e}"),
            ));
        }
    }
}

fn check_skills(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    let resolved_creds = config.resolve_credentials();
    match load_all_skills(&resolved_creds, &config.skills) {
        Ok(skills) => {
            let available = skills.iter().filter(|s| s.available).count();
            let total = skills.len();
            checks.push(DiagnosticCheck::pass(
                "Skills",
                format!("{available}/{total} skills available"),
            ));

            let missing: Vec<String> = skills
                .iter()
                .filter(|s| !s.available)
                .map(|s| s.manifest.name.clone())
                .collect();
            if !missing.is_empty() {
                checks.push(DiagnosticCheck::warn(
                    "Skills",
                    format!("missing requirements: {}", missing.join(", ")),
                    "some skills unavailable",
                ));
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck::fail(
                "Skills",
                "skills loading",
                format!("{e}"),
            ));
        }
    }
}

fn check_memory(checks: &mut Vec<DiagnosticCheck>) {
    match Config::memory_index_path() {
        Ok(path) => {
            if path.exists() {
                checks.push(DiagnosticCheck::pass("Memory", "MEMORY.md exists"));
            } else {
                checks.push(DiagnosticCheck::warn(
                    "Memory",
                    "MEMORY.md exists",
                    "not found",
                ));
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck::fail("Memory", "MEMORY.md", format!("{e}")));
        }
    }

    match Config::identity_path() {
        Ok(path) => {
            if path.exists() {
                checks.push(DiagnosticCheck::pass("Memory", "IDENTITY.md exists"));
            } else {
                checks.push(DiagnosticCheck::warn(
                    "Memory",
                    "IDENTITY.md exists",
                    "not found",
                ));
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck::fail(
                "Memory",
                "IDENTITY.md",
                format!("{e}"),
            ));
        }
    }
}

fn check_data_dir(checks: &mut Vec<DiagnosticCheck>) {
    match Config::data_dir() {
        Ok(data_dir) => {
            if data_dir.exists() {
                let test_file = data_dir.join(".doctor_write_test");
                match std::fs::write(&test_file, "test") {
                    Ok(()) => {
                        let _ = std::fs::remove_file(&test_file);
                        checks.push(DiagnosticCheck::pass("Data", "data directory writable"));
                    }
                    Err(e) => {
                        checks.push(DiagnosticCheck::fail(
                            "Data",
                            "data directory writable",
                            format!("{e}"),
                        ));
                    }
                }
            } else {
                checks.push(DiagnosticCheck::warn(
                    "Data",
                    "data directory exists",
                    "~/.borg not found — run `borg init`",
                ));
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck::fail(
                "Data",
                "data directory",
                format!("{e}"),
            ));
        }
    }

    checks.push(DiagnosticCheck::from_result(
        "Data",
        "database accessible",
        Database::open(),
    ));
}

fn check_gateway(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    checks.push(DiagnosticCheck::pass(
        "Gateway",
        format!(
            "gateway config: {}:{}",
            config.gateway.host, config.gateway.port
        ),
    ));

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

                checks.push(DiagnosticCheck::pass(
                    "Gateway",
                    format!("{count} channel(s) found"),
                ));

                for error in errors {
                    checks.push(DiagnosticCheck::fail("Gateway", "channel manifest", error));
                }
            } else {
                checks.push(DiagnosticCheck::warn(
                    "Gateway",
                    "channels directory",
                    "not found",
                ));
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck::fail(
                "Gateway",
                "channels directory",
                format!("{e}"),
            ));
        }
    }
}

fn check_budget(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    let limit = config.budget.monthly_token_limit;
    if limit == 0 {
        checks.push(DiagnosticCheck::warn(
            "Budget",
            "monthly limit",
            "unlimited (no budget set)",
        ));
        return;
    }

    match Database::open() {
        Ok(db) => match db.monthly_token_total() {
            Ok(used) => {
                let pct = (used as f64 / limit as f64 * 100.0) as u64;
                let name = format!("monthly usage: {used}/{limit} ({pct}%)");
                if used >= limit {
                    checks.push(DiagnosticCheck::fail("Budget", name, "budget exceeded"));
                } else if (used as f64 / limit as f64) >= config.budget.warning_threshold {
                    checks.push(DiagnosticCheck::warn("Budget", name, "approaching limit"));
                } else {
                    checks.push(DiagnosticCheck::pass("Budget", name));
                }
            }
            Err(e) => {
                checks.push(DiagnosticCheck::warn(
                    "Budget",
                    "monthly usage",
                    format!("could not read: {e}"),
                ));
            }
        },
        Err(e) => {
            checks.push(DiagnosticCheck::warn(
                "Budget",
                "monthly usage",
                format!("database unavailable: {e}"),
            ));
        }
    }
}

fn check_plugins(checks: &mut Vec<DiagnosticCheck>) {
    match Database::open() {
        Ok(db) => match db.list_plugins() {
            Ok(plugins) => {
                if plugins.is_empty() {
                    checks.push(DiagnosticCheck::warn(
                        "Plugins",
                        "installed integrations",
                        "none installed (use /plugins)",
                    ));
                } else {
                    let verified = plugins.iter().filter(|c| c.verified_at.is_some()).count();
                    checks.push(DiagnosticCheck::pass(
                        "Plugins",
                        format!(
                            "{} integration(s) installed, {verified} verified",
                            plugins.len()
                        ),
                    ));

                    for c in &plugins {
                        let name = format!("{} ({})", c.name, c.kind);
                        if c.verified_at.is_some() {
                            checks.push(DiagnosticCheck::pass("Plugins", name));
                        } else {
                            checks.push(DiagnosticCheck::warn("Plugins", name, "not verified"));
                        }

                        if let Ok(data_dir) = Config::data_dir() {
                            if let Ok(result) =
                                crate::integrity::verify_integrity(&db, &c.id, &data_dir)
                            {
                                let integrity_name = format!("{} file integrity", c.name);
                                if result.ok {
                                    checks.push(DiagnosticCheck::pass("Plugins", integrity_name));
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
                                    checks.push(DiagnosticCheck::fail(
                                        "Plugins",
                                        integrity_name,
                                        issues.join("; "),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                checks.push(DiagnosticCheck::warn(
                    "Plugins",
                    "plugins table",
                    format!("could not query: {e}"),
                ));
            }
        },
        Err(e) => {
            checks.push(DiagnosticCheck::warn(
                "Plugins",
                "database",
                format!("unavailable: {e}"),
            ));
        }
    }
}

fn check_browser(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    if !config.browser.enabled {
        checks.push(DiagnosticCheck::warn(
            "Browser",
            "browser automation",
            "disabled in config",
        ));
        return;
    }

    let detection = crate::browser::find_chrome(config.browser.executable.as_deref());
    if let Some(ref exe) = detection.executable {
        checks.push(DiagnosticCheck::pass(
            "Browser",
            format!("Chrome detected: {}", exe.display()),
        ));
    } else {
        checks.push(DiagnosticCheck::warn(
            "Browser",
            "Chrome/Chromium detection",
            "no Chrome-like browser found",
        ));
    }

    if detection.executable.is_some() {
        checks.push(DiagnosticCheck::pass(
            "Browser",
            "native CDP browser tool available",
        ));
    }

    if crate::browser::detect_agent_browser() {
        checks.push(DiagnosticCheck::pass(
            "Browser",
            "agent-browser CLI also available (legacy)",
        ));
    }

    checks.push(DiagnosticCheck::pass(
        "Browser",
        format!(
            "config: headless={}, cdp_port={}, timeout={}ms",
            config.browser.headless, config.browser.cdp_port, config.browser.timeout_ms
        ),
    ));
}

fn check_agents(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    if !config.agents.enabled {
        checks.push(DiagnosticCheck::warn(
            "Agents",
            "multi-agent system",
            "disabled",
        ));
        return;
    }
    checks.push(DiagnosticCheck::pass(
        "Agents",
        format!(
            "multi-agent enabled (depth={}, children={}, concurrent={})",
            config.agents.max_spawn_depth,
            config.agents.max_children_per_agent,
            config.agents.max_concurrent,
        ),
    ));
    if config.agents.max_spawn_depth > 3 {
        checks.push(DiagnosticCheck::warn(
            "Agents",
            "spawn depth",
            format!(
                "max_spawn_depth={} is high (recommended ≤3)",
                config.agents.max_spawn_depth
            ),
        ));
    }
    let roles = crate::multi_agent::roles::list_all_roles();
    checks.push(DiagnosticCheck::pass(
        "Agents",
        format!("{} role(s) available", roles.len()),
    ));
}

fn check_embeddings(config: &Config, checks: &mut Vec<DiagnosticCheck>) {
    if !config.memory.embeddings.enabled {
        checks.push(DiagnosticCheck::warn(
            "Embeddings",
            "semantic memory search",
            "disabled in config",
        ));
        return;
    }

    match crate::embeddings::EmbeddingProvider::from_config(&config.memory.embeddings) {
        Some(provider) => {
            checks.push(DiagnosticCheck::pass(
                "Embeddings",
                format!(
                    "provider: {} (model: {})",
                    provider.endpoint, provider.model
                ),
            ));
        }
        None => {
            checks.push(DiagnosticCheck::warn(
                "Embeddings",
                "embedding provider",
                "no embedding-capable API key found, using recency-based loading",
            ));
            return;
        }
    }

    match Database::open() {
        Ok(db) => {
            let global_count = db.count_embeddings("global").unwrap_or(0);

            // Count memory files
            let file_count = Config::memory_dir()
                .ok()
                .and_then(|dir| std::fs::read_dir(dir).ok())
                .map(|entries| {
                    entries
                        .filter_map(std::result::Result::ok)
                        .filter(|e| e.path().extension().map(|ext| ext == "md").unwrap_or(false))
                        .count()
                })
                .unwrap_or(0);

            let name = format!("embeddings: {global_count}/{file_count} memory files indexed");
            if file_count == 0 || global_count >= file_count {
                checks.push(DiagnosticCheck::pass("Embeddings", name));
            } else {
                checks.push(DiagnosticCheck::warn(
                    "Embeddings",
                    name,
                    "some memory files not yet embedded (will be indexed on next write)",
                ));
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck::warn(
                "Embeddings",
                "embedding count",
                format!("database unavailable: {e}"),
            ));
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
                DiagnosticCheck::pass("Config", "config.toml exists"),
                DiagnosticCheck::pass("Config", "config.toml valid"),
                DiagnosticCheck::fail("Provider", "API key set", "not found"),
                DiagnosticCheck::warn("Sandbox", "sandbox-exec", "not available"),
            ],
        };
        let output = report.format();
        assert!(output.contains("Borg Doctor"));
        assert!(output.contains("✓ config.toml exists"));
        assert!(output.contains("✗ API key set"));
        assert!(output.contains("⚠ sandbox-exec"));
        assert!(output.contains("2 passed, 1 warning(s), 1 failed"));
    }

    #[test]
    fn report_counts() {
        let report = DiagnosticReport {
            checks: vec![
                DiagnosticCheck::pass("Test", "pass"),
                DiagnosticCheck::warn("Test", "warn", "w"),
                DiagnosticCheck::fail("Test", "fail", "f"),
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

    #[test]
    fn diagnostic_check_pass_fields() {
        let check = DiagnosticCheck::pass("Config", "config.toml exists");
        assert_eq!(check.category, "Config");
        assert_eq!(check.name, "config.toml exists");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[test]
    fn diagnostic_check_fail_includes_detail() {
        let check = DiagnosticCheck::fail("Provider", "API key", "not found in env");
        assert_eq!(check.category, "Provider");
        assert!(matches!(check.status, CheckStatus::Fail(ref msg) if msg == "not found in env"));
    }

    #[test]
    fn diagnostic_check_warn_includes_detail() {
        let check = DiagnosticCheck::warn("Sandbox", "sandbox-exec", "not available on Linux");
        assert_eq!(check.category, "Sandbox");
        assert!(
            matches!(check.status, CheckStatus::Warn(ref msg) if msg == "not available on Linux")
        );
    }

    #[test]
    fn report_format_groups_consecutive_same_category() {
        let report = DiagnosticReport {
            checks: vec![
                DiagnosticCheck::pass("Config", "check A"),
                DiagnosticCheck::pass("Config", "check C"),
                DiagnosticCheck::pass("Provider", "check B"),
            ],
        };
        let output = report.format();
        // Consecutive Config checks should appear under a single Config heading
        let config_heading_count = output.matches("\nConfig\n").count();
        assert_eq!(
            config_heading_count, 1,
            "Consecutive same-category checks should share one heading"
        );
        // check A and check C should both appear before Provider
        let check_a_pos = output.find("check A").unwrap();
        let check_c_pos = output.find("check C").unwrap();
        let provider_pos = output.find("Provider").unwrap();
        assert!(check_a_pos < check_c_pos);
        assert!(check_c_pos < provider_pos);
    }

    #[test]
    fn run_diagnostics_includes_browser_category() {
        let config = Config::default();
        let report = run_diagnostics(&config);
        let categories: std::collections::HashSet<&str> =
            report.checks.iter().map(|c| c.category).collect();
        assert!(categories.contains("Browser"));
    }

    #[test]
    fn browser_disabled_produces_warn() {
        let mut checks = Vec::new();
        let mut config = Config::default();
        config.browser.enabled = false;
        check_browser(&config, &mut checks);
        assert_eq!(checks.len(), 1);
        assert!(matches!(checks[0].status, CheckStatus::Warn(_)));
        assert_eq!(checks[0].category, "Browser");
    }
}
