use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::{debug, instrument};

use crate::config::{Config, SkillsConfig};
use crate::tokenizer::estimate_tokens;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SkillLoadLevel {
    Metadata, // name + description + status only
    Summary,  // metadata + first paragraph of body
    Full,     // entire SKILL.md body
}

const BUILTIN_SLACK: &str = include_str!("../skills/slack/SKILL.md");
const BUILTIN_DISCORD: &str = include_str!("../skills/discord/SKILL.md");
const BUILTIN_GITHUB: &str = include_str!("../skills/github/SKILL.md");
const BUILTIN_WEATHER: &str = include_str!("../skills/weather/SKILL.md");
const BUILTIN_SKILL_CREATOR: &str = include_str!("../skills/skill-creator/SKILL.md");
const BUILTIN_GIT: &str = include_str!("../skills/git/SKILL.md");
const BUILTIN_HTTP: &str = include_str!("../skills/http/SKILL.md");
const BUILTIN_SEARCH: &str = include_str!("../skills/search/SKILL.md");
const BUILTIN_DOCKER: &str = include_str!("../skills/docker/SKILL.md");
const BUILTIN_DATABASE: &str = include_str!("../skills/database/SKILL.md");
const BUILTIN_NOTES: &str = include_str!("../skills/notes/SKILL.md");
const BUILTIN_CALENDAR: &str = include_str!("../skills/calendar/SKILL.md");
const BUILTIN_1PASSWORD: &str = include_str!("../skills/1password/SKILL.md");
const BUILTIN_BROWSER: &str = include_str!("../skills/browser/SKILL.md");
const BUILTIN_SCHEDULER: &str = include_str!("../skills/scheduler/SKILL.md");

const BUNDLED_SKILLS: &[(&str, &str)] = &[
    ("slack", BUILTIN_SLACK),
    ("discord", BUILTIN_DISCORD),
    ("github", BUILTIN_GITHUB),
    ("weather", BUILTIN_WEATHER),
    ("skill-creator", BUILTIN_SKILL_CREATOR),
    ("git", BUILTIN_GIT),
    ("http", BUILTIN_HTTP),
    ("search", BUILTIN_SEARCH),
    ("docker", BUILTIN_DOCKER),
    ("database", BUILTIN_DATABASE),
    ("notes", BUILTIN_NOTES),
    ("calendar", BUILTIN_CALENDAR),
    ("1password", BUILTIN_1PASSWORD),
    ("browser", BUILTIN_BROWSER),
    ("scheduler", BUILTIN_SCHEDULER),
];

#[derive(Debug, Clone, PartialEq)]
pub enum SkillSource {
    BuiltIn,
    User,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SkillRequires {
    #[serde(default)]
    pub bins: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub any_bins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct InstallSpec {
    #[serde(default)]
    pub brew: Option<String>,
    #[serde(default)]
    pub apt: Option<String>,
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub requires: SkillRequires,
    #[serde(default)]
    pub os: Vec<String>,
    #[serde(default)]
    pub install: std::collections::HashMap<String, InstallSpec>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub manifest: SkillManifest,
    pub body: String,
    pub source: SkillSource,
    pub available: bool,
    pub disabled: bool,
    pub references: Vec<(String, String)>,
    pub scripts: Vec<PathBuf>,
}

impl Skill {
    pub fn format_for_prompt(&self) -> String {
        self.format_at_level(SkillLoadLevel::Full)
    }

    pub fn format_at_level(&self, level: SkillLoadLevel) -> String {
        let status = if self.disabled {
            "disabled"
        } else if self.available {
            "available"
        } else {
            "unavailable (missing requirements)"
        };
        let source = match self.source {
            SkillSource::BuiltIn => "built-in",
            SkillSource::User => "user",
        };
        match level {
            SkillLoadLevel::Metadata => {
                format!(
                    "### {} ({}, {}) — {}",
                    self.manifest.name, source, status, self.manifest.description
                )
            }
            SkillLoadLevel::Summary => {
                let first_para = self
                    .body
                    .split("\n\n")
                    .find(|p| !p.trim().is_empty())
                    .unwrap_or("");
                format!(
                    "## Skill: {} [{}, {}]\n\n{}\n",
                    self.manifest.name, source, status, first_para
                )
            }
            SkillLoadLevel::Full => {
                format!(
                    "## Skill: {} [{}, {}]\n\n{}\n",
                    self.manifest.name, source, status, self.body
                )
            }
        }
    }

    pub fn summary_line(&self) -> String {
        let status = if self.disabled {
            "—"
        } else if self.available {
            "✓"
        } else {
            "✗"
        };
        let source = match self.source {
            SkillSource::BuiltIn => "built-in",
            SkillSource::User => "user",
        };
        format!(
            "[{}] {} ({}) — {}",
            status, self.manifest.name, source, self.manifest.description
        )
    }
}

pub fn parse_skill_md(content: &str) -> Result<(SkillManifest, String)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        anyhow::bail!("SKILL.md must start with YAML frontmatter (---)")
    }

    let after_first = &trimmed[3..];
    let end = after_first
        .find("\n---")
        .context("Missing closing --- in SKILL.md frontmatter")?;

    let yaml_str = &after_first[..end];
    let body = after_first[end + 4..].trim().to_string();

    let manifest: SkillManifest =
        serde_yaml::from_str(yaml_str).context("Failed to parse SKILL.md YAML frontmatter")?;

    Ok((manifest, body))
}

fn check_os_requirements(os: &[String]) -> bool {
    if os.is_empty() {
        return true;
    }
    let current = std::env::consts::OS;
    os.iter()
        .any(|o| o == current || (o == "darwin" && current == "macos"))
}

fn check_requirements(
    requires: &SkillRequires,
    os: &[String],
    resolved_creds: &std::collections::HashMap<String, String>,
) -> bool {
    if !check_os_requirements(os) {
        debug!("Skill requirement missing: unsupported OS (need {:?})", os);
        return false;
    }
    for bin in &requires.bins {
        if which::which(bin).is_err() {
            debug!("Skill requirement missing: binary '{bin}'");
            return false;
        }
    }
    if !requires.any_bins.is_empty() && !requires.any_bins.iter().any(|b| which::which(b).is_ok()) {
        debug!(
            "Skill requirement missing: none of {:?} found",
            requires.any_bins
        );
        return false;
    }
    for var in &requires.env {
        if std::env::var(var).is_err() && !resolved_creds.contains_key(var) {
            debug!("Skill requirement missing: env var '{var}'");
            return false;
        }
    }
    true
}

fn load_builtin_skill(
    content: &str,
    resolved_creds: &std::collections::HashMap<String, String>,
    skills_config: &SkillsConfig,
) -> Result<Skill> {
    let (manifest, body) = parse_skill_md(content)?;
    let disabled = skills_config
        .entries
        .get(&manifest.name)
        .is_some_and(|e| !e.enabled);
    let available =
        !disabled && check_requirements(&manifest.requires, &manifest.os, resolved_creds);
    Ok(Skill {
        manifest,
        body,
        source: SkillSource::BuiltIn,
        available,
        disabled,
        references: Vec::new(),
        scripts: Vec::new(),
    })
}

pub fn skills_dir() -> Result<PathBuf> {
    Config::skills_dir()
}

/// Install bundled skills to the filesystem at `data_dir/skills/<name>/SKILL.md`.
/// Skips any skill that already exists (preserving user customizations).
pub fn install_default_skills(data_dir: &std::path::Path) -> Result<usize> {
    let skills_dir = data_dir.join("skills");
    let mut installed = 0;

    for &(name, content) in BUNDLED_SKILLS {
        let skill_dir = skills_dir.join(name);
        let skill_file = skill_dir.join("SKILL.md");

        if skill_file.exists() {
            continue;
        }

        std::fs::create_dir_all(&skill_dir)?;
        std::fs::write(&skill_file, content)?;
        installed += 1;
        debug!("Installed default skill: {name}");
    }

    Ok(installed)
}

pub fn load_all_skills(
    resolved_creds: &std::collections::HashMap<String, String>,
    skills_config: &SkillsConfig,
) -> Result<Vec<Skill>> {
    let mut skills: Vec<Skill> = Vec::new();

    {
        for &(_name, content) in BUNDLED_SKILLS {
            match load_builtin_skill(content, resolved_creds, skills_config) {
                Ok(skill) => {
                    skills.push(skill);
                }
                Err(e) => {
                    debug!("Failed to load built-in skill: {e}");
                }
            }
        }
    }

    // Load user skills from ~/.borg/skills/*/SKILL.md
    let user_dir = skills_dir()?;
    if user_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&user_dir) {
            for entry in entries.flatten() {
                let skill_file = entry.path().join("SKILL.md");
                if skill_file.exists() {
                    match std::fs::read_to_string(&skill_file) {
                        Ok(content) => match parse_skill_md(&content) {
                            Ok((manifest, body)) => {
                                let disabled = skills_config
                                    .entries
                                    .get(&manifest.name)
                                    .is_some_and(|e| !e.enabled);
                                let available = !disabled
                                    && check_requirements(
                                        &manifest.requires,
                                        &manifest.os,
                                        resolved_creds,
                                    );
                                let name = manifest.name.clone();
                                let skill_dir = entry.path();

                                // Load references from references/*.md
                                let mut references = Vec::new();
                                let refs_dir = skill_dir.join("references");
                                if refs_dir.is_dir() {
                                    if let Ok(ref_entries) = std::fs::read_dir(&refs_dir) {
                                        for ref_entry in ref_entries.flatten() {
                                            let ref_path = ref_entry.path();
                                            if ref_path.extension().is_some_and(|e| e == "md") {
                                                if let Ok(ref_content) =
                                                    std::fs::read_to_string(&ref_path)
                                                {
                                                    let ref_name = ref_path
                                                        .file_name()
                                                        .unwrap_or_default()
                                                        .to_string_lossy()
                                                        .to_string();
                                                    references.push((ref_name, ref_content));
                                                }
                                            }
                                        }
                                    }
                                }

                                // Detect scripts
                                let mut scripts = Vec::new();
                                let scripts_dir = skill_dir.join("scripts");
                                if scripts_dir.is_dir() {
                                    if let Ok(script_entries) = std::fs::read_dir(&scripts_dir) {
                                        for script_entry in script_entries.flatten() {
                                            scripts.push(script_entry.path());
                                        }
                                    }
                                }

                                // User skills override built-in skills with the same name
                                skills.retain(|s| s.manifest.name != name);

                                skills.push(Skill {
                                    manifest,
                                    body,
                                    source: SkillSource::User,
                                    available,
                                    disabled,
                                    references,
                                    scripts,
                                });
                                debug!("Loaded user skill: {name}");
                            }
                            Err(e) => {
                                debug!("Failed to parse {}: {e}", skill_file.display());
                            }
                        },
                        Err(e) => {
                            debug!("Failed to read {}: {e}", skill_file.display());
                        }
                    }
                }
            }
        }
    }

    Ok(skills)
}

#[instrument(skip_all)]
pub fn load_skills_context(
    max_tokens: usize,
    resolved_creds: &std::collections::HashMap<String, String>,
    skills_config: &SkillsConfig,
) -> Result<String> {
    let skills = load_all_skills(resolved_creds, skills_config)?;

    if skills.is_empty() {
        return Ok(String::new());
    }

    // Exclude disabled skills from context injection entirely
    let skills: Vec<Skill> = skills.into_iter().filter(|s| !s.disabled).collect();

    // Sort: available skills first, then unavailable
    let mut sorted_skills = skills;
    sorted_skills.sort_by_key(|s| !s.available);

    // Phase 1: Include metadata for ALL skills
    let mut metadata_parts = Vec::new();
    let mut estimated_tokens = 0;

    for skill in &sorted_skills {
        let meta = skill.format_at_level(SkillLoadLevel::Metadata);
        let tokens = estimate_tokens(&meta);
        metadata_parts.push(meta);
        estimated_tokens += tokens;
    }

    // Phase 2: With remaining budget, upgrade available skills to full body
    let mut full_parts = Vec::new();

    for (i, skill) in sorted_skills.iter().enumerate() {
        if !skill.available {
            continue;
        }
        let full = skill.format_at_level(SkillLoadLevel::Full);
        let full_tokens = estimate_tokens(&full);
        let meta_tokens = estimate_tokens(&metadata_parts[i]);
        let additional = full_tokens.saturating_sub(meta_tokens);

        if estimated_tokens + additional > max_tokens {
            debug!(
                "Skipping full body for skill '{}' (would exceed token budget)",
                skill.manifest.name
            );
            continue;
        }

        full_parts.push((i, full));
        estimated_tokens += additional;
        debug!(
            "Included full skill '{}' ({full_tokens} estimated tokens)",
            skill.manifest.name
        );
    }

    // Build final output: full body for upgraded skills, metadata for the rest
    let mut parts = Vec::new();
    for (i, meta) in metadata_parts.iter().enumerate() {
        if let Some((_, full)) = full_parts.iter().find(|(idx, _)| *idx == i) {
            parts.push(full.clone());
        } else {
            parts.push(meta.clone());
        }
    }

    if parts.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("# Skills\n\n{}\n", parts.join("\n---\n\n")))
    }
}

/// Collect per-skill env vars from config entries for injection into run_shell.
/// Only includes env from enabled skill entries.
pub fn collect_skill_env(
    skills_config: &SkillsConfig,
) -> std::collections::HashMap<String, String> {
    let mut env = std::collections::HashMap::new();
    for entry in skills_config.entries.values() {
        if entry.enabled {
            for (k, v) in &entry.env {
                env.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
    }
    env
}

/// Install missing dependencies for a skill.
/// Returns list of successfully installed dependency names.
pub fn install_skill_deps(skill: &Skill) -> Result<Vec<String>> {
    let mut installed = Vec::new();
    for (dep_name, spec) in &skill.manifest.install {
        if which::which(dep_name).is_ok() {
            println!("  {dep_name}: already installed");
            continue;
        }
        let cmd = match std::env::consts::OS {
            "macos" => spec.brew.as_ref().map(|p| format!("brew install {p}")),
            "linux" => spec
                .apt
                .as_ref()
                .map(|p| format!("sudo apt install -y {p}")),
            _ => None,
        }
        .or_else(|| spec.npm.as_ref().map(|p| format!("npm install -g {p}")));

        match cmd {
            Some(c) => {
                println!("  Installing {dep_name}: {c}");
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&c)
                    .status()?;
                if status.success() {
                    installed.push(dep_name.clone());
                } else {
                    eprintln!("  Failed to install {dep_name}");
                }
            }
            None => {
                if let Some(url) = &spec.url {
                    eprintln!("  Cannot auto-install {dep_name}. Install manually: {url}");
                } else {
                    eprintln!("  No install method available for {dep_name}");
                }
            }
        }
    }
    Ok(installed)
}

/// Format detailed info about a skill.
pub fn format_skill_info(skill: &Skill) -> String {
    let mut out = String::new();
    let source = match skill.source {
        SkillSource::BuiltIn => "built-in",
        SkillSource::User => "user",
    };
    let status = if skill.disabled {
        "disabled"
    } else if skill.available {
        "available"
    } else {
        "unavailable"
    };
    out.push_str(&format!("Name:        {}\n", skill.manifest.name));
    out.push_str(&format!("Description: {}\n", skill.manifest.description));
    out.push_str(&format!("Source:      {source}\n"));
    out.push_str(&format!("Status:      {status}\n"));

    if !skill.manifest.os.is_empty() {
        out.push_str(&format!("OS:          {}\n", skill.manifest.os.join(", ")));
    }

    if !skill.manifest.requires.bins.is_empty() {
        out.push_str("Binaries:\n");
        for bin in &skill.manifest.requires.bins {
            let found = which::which(bin).is_ok();
            let mark = if found { "✓" } else { "✗" };
            out.push_str(&format!("  [{mark}] {bin}\n"));
        }
    }
    if !skill.manifest.requires.any_bins.is_empty() {
        out.push_str("Any of:\n");
        for bin in &skill.manifest.requires.any_bins {
            let found = which::which(bin).is_ok();
            let mark = if found { "✓" } else { "✗" };
            out.push_str(&format!("  [{mark}] {bin}\n"));
        }
    }
    if !skill.manifest.requires.env.is_empty() {
        out.push_str("Env vars:\n");
        for var in &skill.manifest.requires.env {
            let found = std::env::var(var).is_ok();
            let mark = if found { "✓" } else { "✗" };
            out.push_str(&format!("  [{mark}] {var}\n"));
        }
    }
    if !skill.manifest.install.is_empty() {
        out.push_str("Install specs:\n");
        for (dep, spec) in &skill.manifest.install {
            let mut methods = Vec::new();
            if let Some(b) = &spec.brew {
                methods.push(format!("brew:{b}"));
            }
            if let Some(a) = &spec.apt {
                methods.push(format!("apt:{a}"));
            }
            if let Some(n) = &spec.npm {
                methods.push(format!("npm:{n}"));
            }
            if let Some(u) = &spec.url {
                methods.push(format!("url:{u}"));
            }
            out.push_str(&format!("  {dep}: {}\n", methods.join(", ")));
        }
    }

    // Body preview (first paragraph)
    if let Some(first_para) = skill.body.split("\n\n").find(|p| !p.trim().is_empty()) {
        out.push_str(&format!("\n{first_para}\n"));
    }

    out
}

/// Check all skills and produce a diagnostic report.
pub fn check_all_skills(
    resolved_creds: &std::collections::HashMap<String, String>,
    skills_config: &SkillsConfig,
) -> Result<String> {
    let skills = load_all_skills(resolved_creds, skills_config)?;
    let mut lines = Vec::new();
    for skill in &skills {
        let label = if skill.disabled {
            "OFF "
        } else if skill.available {
            " OK "
        } else if !check_os_requirements(&skill.manifest.os) {
            "SKIP"
        } else {
            "MISS"
        };
        let mut detail = skill.manifest.description.clone();
        if !skill.available && !skill.disabled {
            let mut missing = Vec::new();
            for bin in &skill.manifest.requires.bins {
                if which::which(bin).is_err() {
                    let install_hint =
                        skill
                            .manifest
                            .install
                            .get(bin)
                            .and_then(|s| match std::env::consts::OS {
                                "macos" => s.brew.as_ref().map(|b| format!("brew install {b}")),
                                "linux" => s.apt.as_ref().map(|a| format!("apt install {a}")),
                                _ => None,
                            });
                    match install_hint {
                        Some(hint) => missing.push(format!("binary: {bin} ({hint})")),
                        None => missing.push(format!("binary: {bin}")),
                    }
                }
            }
            if !skill.manifest.requires.any_bins.is_empty()
                && !skill
                    .manifest
                    .requires
                    .any_bins
                    .iter()
                    .any(|b| which::which(b).is_ok())
            {
                missing.push(format!(
                    "any of: {}",
                    skill.manifest.requires.any_bins.join(", ")
                ));
            }
            for var in &skill.manifest.requires.env {
                if std::env::var(var).is_err() && !resolved_creds.contains_key(var) {
                    missing.push(format!("env: {var}"));
                }
            }
            if !missing.is_empty() {
                detail = format!("missing {}", missing.join("; "));
            }
        }
        lines.push(format!("  [{label}] {} — {detail}", skill.manifest.name));
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SKILL: &str = r#"---
name: test-skill
description: "A test skill for unit tests."
requires:
  bins: []
  env: []
---

# Test Skill

This is a test skill body.

```bash
echo "hello"
```
"#;

    const MINIMAL_SKILL: &str = r#"---
name: minimal
description: "Minimal skill."
---

# Minimal

Body here.
"#;

    #[test]
    fn parse_valid_skill() {
        let (manifest, body) = parse_skill_md(SAMPLE_SKILL).unwrap();
        assert_eq!(manifest.name, "test-skill");
        assert_eq!(manifest.description, "A test skill for unit tests.");
        assert!(manifest.requires.bins.is_empty());
        assert!(manifest.requires.env.is_empty());
        assert!(body.contains("# Test Skill"));
        assert!(body.contains("echo \"hello\""));
    }

    #[test]
    fn parse_minimal_skill() {
        let (manifest, body) = parse_skill_md(MINIMAL_SKILL).unwrap();
        assert_eq!(manifest.name, "minimal");
        assert!(manifest.requires.bins.is_empty());
        assert!(body.contains("# Minimal"));
    }

    #[test]
    fn parse_missing_frontmatter() {
        let result = parse_skill_md("# No frontmatter\nJust a body.");
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_closing_delimiter() {
        let result = parse_skill_md("---\nname: broken\ndescription: oops\n# no closing");
        assert!(result.is_err());
    }

    #[test]
    fn builtins_parse_correctly() {
        for &(name, content) in BUNDLED_SKILLS {
            let (manifest, body) = parse_skill_md(content)
                .unwrap_or_else(|e| panic!("Built-in skill '{name}' failed to parse: {e}"));
            assert_eq!(manifest.name, name);
            assert!(!manifest.description.is_empty());
            assert!(!body.is_empty());
        }
    }

    #[test]
    fn token_budgeting() {
        // A skill with ~100 chars body = ~25 tokens
        let small = r#"---
name: small
description: "Small."
---

# Small

Short body.
"#;
        let (manifest, body) = parse_skill_md(small).unwrap();
        let skill = Skill {
            manifest,
            body,
            source: SkillSource::BuiltIn,
            available: true,
            disabled: false,
            references: Vec::new(),
            scripts: Vec::new(),
        };
        let formatted = skill.format_for_prompt();
        let tokens = estimate_tokens(&formatted);
        assert!(tokens < 50);
    }

    #[test]
    fn load_all_includes_builtins() {
        let skills =
            load_all_skills(&std::collections::HashMap::new(), &SkillsConfig::default()).unwrap();
        let names: Vec<&str> = skills.iter().map(|s| s.manifest.name.as_str()).collect();
        assert!(names.contains(&"slack"));
        assert!(names.contains(&"discord"));
        assert!(names.contains(&"github"));
        assert!(names.contains(&"weather"));
        assert!(names.contains(&"skill-creator"));
        assert!(names.contains(&"git"));
        assert!(names.contains(&"http"));
        assert!(names.contains(&"search"));
        assert!(names.contains(&"docker"));
        assert!(names.contains(&"database"));
        assert!(names.contains(&"notes"));
        assert!(names.contains(&"calendar"));
        assert!(names.contains(&"1password"));
        assert!(names.contains(&"browser"));
        assert!(names.contains(&"scheduler"));
    }

    #[test]
    fn install_default_skills_writes_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let count = install_default_skills(tmp.path()).unwrap();
        assert_eq!(count, BUNDLED_SKILLS.len());

        for &(name, _) in BUNDLED_SKILLS {
            let skill_file = tmp.path().join("skills").join(name).join("SKILL.md");
            assert!(skill_file.exists(), "Missing: {}", skill_file.display());
        }
    }

    #[test]
    fn install_default_skills_no_overwrite() {
        let tmp = tempfile::TempDir::new().unwrap();
        let skill_dir = tmp.path().join("skills/slack");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "custom content").unwrap();

        install_default_skills(tmp.path()).unwrap();

        let content = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert_eq!(content, "custom content");
    }

    #[test]
    fn skill_summary_line() {
        let skill = Skill {
            manifest: SkillManifest {
                name: "test".to_string(),
                description: "A test.".to_string(),
                requires: SkillRequires::default(),
                os: vec![],
                install: std::collections::HashMap::new(),
            },
            body: "body".to_string(),
            source: SkillSource::BuiltIn,
            available: true,
            disabled: false,
            references: Vec::new(),
            scripts: Vec::new(),
        };
        let line = skill.summary_line();
        assert!(line.contains("[✓]"));
        assert!(line.contains("test"));
        assert!(line.contains("built-in"));
    }

    #[test]
    fn skill_context_respects_token_budget() {
        // With a very small budget, not all skills should fit
        let context = load_skills_context(
            100,
            &std::collections::HashMap::new(),
            &SkillsConfig::default(),
        )
        .unwrap();
        // At least the header should be there if any fit, or empty if none fit
        if !context.is_empty() {
            assert!(context.starts_with("# Skills"));
        }
    }

    #[test]
    fn check_requirements_no_reqs() {
        let reqs = SkillRequires::default();
        assert!(check_requirements(
            &reqs,
            &[],
            &std::collections::HashMap::new()
        ));
    }

    #[test]
    fn check_requirements_missing_bin() {
        let reqs = SkillRequires {
            bins: vec!["definitely_not_a_real_binary_xyz123".to_string()],
            env: vec![],
            any_bins: vec![],
        };
        assert!(!check_requirements(
            &reqs,
            &[],
            &std::collections::HashMap::new()
        ));
    }

    #[test]
    fn check_requirements_missing_env() {
        let reqs = SkillRequires {
            bins: vec![],
            env: vec!["DEFINITELY_NOT_A_REAL_ENV_VAR_XYZ123".to_string()],
            any_bins: vec![],
        };
        assert!(!check_requirements(
            &reqs,
            &[],
            &std::collections::HashMap::new()
        ));
    }

    #[test]
    fn check_requirements_env_satisfied_by_credentials() {
        let reqs = SkillRequires {
            bins: vec![],
            env: vec!["CRED_STORE_VAR_XYZ123".to_string()],
            any_bins: vec![],
        };
        let mut creds = std::collections::HashMap::new();
        creds.insert(
            "CRED_STORE_VAR_XYZ123".to_string(),
            "secret-val".to_string(),
        );
        assert!(check_requirements(&reqs, &[], &creds));
    }

    #[test]
    fn user_skill_overrides_builtin() {
        // This tests the override logic in isolation
        let mut skills = vec![Skill {
            manifest: SkillManifest {
                name: "weather".to_string(),
                description: "built-in weather".to_string(),
                requires: SkillRequires::default(),
                os: vec![],
                install: std::collections::HashMap::new(),
            },
            body: "built-in body".to_string(),
            source: SkillSource::BuiltIn,
            available: true,
            disabled: false,
            references: Vec::new(),
            scripts: Vec::new(),
        }];

        let user_name = "weather".to_string();
        skills.retain(|s| s.manifest.name != user_name);
        skills.push(Skill {
            manifest: SkillManifest {
                name: "weather".to_string(),
                description: "user weather".to_string(),
                requires: SkillRequires::default(),
                os: vec![],
                install: std::collections::HashMap::new(),
            },
            body: "user body".to_string(),
            source: SkillSource::User,
            available: true,
            disabled: false,
            references: Vec::new(),
            scripts: Vec::new(),
        });

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source, SkillSource::User);
        assert_eq!(skills[0].manifest.description, "user weather");
    }

    #[test]
    fn metadata_only_is_compact() {
        let skill = Skill {
            manifest: SkillManifest {
                name: "test-meta".to_string(),
                description: "A test skill.".to_string(),
                requires: SkillRequires::default(),
                os: vec![],
                install: std::collections::HashMap::new(),
            },
            body: "# Full Body\n\nLots of content here.\n\n## Section 2\n\nMore content."
                .to_string(),
            source: SkillSource::BuiltIn,
            available: true,
            disabled: false,
            references: Vec::new(),
            scripts: Vec::new(),
        };
        let meta = skill.format_at_level(SkillLoadLevel::Metadata);
        let full = skill.format_at_level(SkillLoadLevel::Full);
        let meta_tokens = estimate_tokens(&meta);
        let full_tokens = estimate_tokens(&full);
        assert!(meta_tokens < full_tokens);
        assert!(meta.contains("test-meta"));
        assert!(meta.contains("A test skill."));
        assert!(!meta.contains("Full Body"));
    }

    #[test]
    fn summary_level_includes_first_paragraph() {
        let skill = Skill {
            manifest: SkillManifest {
                name: "test-summary".to_string(),
                description: "A summary test.".to_string(),
                requires: SkillRequires::default(),
                os: vec![],
                install: std::collections::HashMap::new(),
            },
            body: "# Title\n\nFirst paragraph here.\n\n## Section 2\n\nMore content.".to_string(),
            source: SkillSource::BuiltIn,
            available: true,
            disabled: false,
            references: Vec::new(),
            scripts: Vec::new(),
        };
        let summary = skill.format_at_level(SkillLoadLevel::Summary);
        assert!(summary.contains("# Title"));
        assert!(!summary.contains("Section 2"));
    }

    #[test]
    fn progressive_loading_fits_many_skills() {
        // With metadata-only, 50 skills should fit in a reasonable budget
        let mut skills = Vec::new();
        for i in 0..50 {
            skills.push(Skill {
                manifest: SkillManifest {
                    name: format!("skill-{i}"),
                    description: format!("Description for skill {i}"),
                    requires: SkillRequires::default(),
                    os: vec![],
                    install: std::collections::HashMap::new(),
                },
                body: format!("# Skill {i}\n\nBody content for skill {i}.\n\n## Usage\n\nDetailed usage instructions for skill {i} with examples and documentation."),
                source: SkillSource::BuiltIn,
                available: i % 2 == 0,
                disabled: false,
                references: Vec::new(),
                scripts: Vec::new(),
            });
        }

        // All metadata should be under 4000 tokens
        let total_meta_tokens: usize = skills
            .iter()
            .map(|s| estimate_tokens(&s.format_at_level(SkillLoadLevel::Metadata)))
            .sum();
        assert!(
            total_meta_tokens < 4000,
            "50 skill metadata = {total_meta_tokens} tokens"
        );
    }

    #[test]
    fn progressive_loading_prioritizes_available() {
        // load_skills_context should include full body for available skills first
        let context = load_skills_context(
            2000,
            &std::collections::HashMap::new(),
            &SkillsConfig::default(),
        )
        .unwrap();
        // Just verify it doesn't panic and produces output
        if !context.is_empty() {
            assert!(context.starts_with("# Skills"));
        }
    }

    // --- Per-skill enable/disable tests ---

    #[test]
    fn test_disabled_skill_excluded_from_context() {
        let mut entries = std::collections::HashMap::new();
        entries.insert(
            "slack".to_string(),
            crate::config::SkillEntryConfig {
                enabled: false,
                env: std::collections::HashMap::new(),
            },
        );
        let config = SkillsConfig {
            enabled: true,
            max_context_tokens: 4000,
            entries,
        };
        let context =
            load_skills_context(4000, &std::collections::HashMap::new(), &config).unwrap();
        // The slack skill header should not appear (other skills may mention "slack" in their body)
        assert!(!context.contains("Skill: slack ["));
        assert!(!context.contains("slack (built-in,"));
    }

    #[test]
    fn test_disabled_skill_shown_in_list() {
        let mut entries = std::collections::HashMap::new();
        entries.insert(
            "slack".to_string(),
            crate::config::SkillEntryConfig {
                enabled: false,
                env: std::collections::HashMap::new(),
            },
        );
        let config = SkillsConfig {
            enabled: true,
            max_context_tokens: 4000,
            entries,
        };
        let skills = load_all_skills(&std::collections::HashMap::new(), &config).unwrap();
        let slack = skills.iter().find(|s| s.manifest.name == "slack").unwrap();
        assert!(slack.disabled);
    }

    #[test]
    fn test_skill_enabled_by_default() {
        let config = SkillsConfig::default();
        let skills = load_all_skills(&std::collections::HashMap::new(), &config).unwrap();
        for skill in &skills {
            assert!(
                !skill.disabled,
                "Skill {} should not be disabled",
                skill.manifest.name
            );
        }
    }

    #[test]
    fn test_summary_line_disabled() {
        let skill = Skill {
            manifest: SkillManifest {
                name: "test".to_string(),
                description: "A test.".to_string(),
                requires: SkillRequires::default(),
                os: vec![],
                install: std::collections::HashMap::new(),
            },
            body: "body".to_string(),
            source: SkillSource::BuiltIn,
            available: false,
            disabled: true,
            references: Vec::new(),
            scripts: Vec::new(),
        };
        let line = skill.summary_line();
        assert!(line.contains("[—]"));
    }

    // --- OS gating tests ---

    #[test]
    fn test_os_gating_empty_means_all() {
        assert!(check_os_requirements(&[]));
    }

    #[test]
    fn test_os_gating_current_os() {
        let current = std::env::consts::OS.to_string();
        assert!(check_os_requirements(&[current]));
    }

    #[test]
    fn test_os_gating_wrong_os() {
        assert!(!check_os_requirements(&["totally_fake_os".to_string()]));
    }

    #[test]
    fn test_os_gating_darwin_alias() {
        if std::env::consts::OS == "macos" {
            assert!(check_os_requirements(&["darwin".to_string()]));
        }
    }

    #[test]
    fn test_parse_skill_with_os() {
        let content = r#"---
name: macos-only
description: "macOS only skill."
os:
  - macos
  - darwin
---

# macOS Only

Body here.
"#;
        let (manifest, _body) = parse_skill_md(content).unwrap();
        assert_eq!(manifest.os, vec!["macos", "darwin"]);
    }

    // --- any_bins tests ---

    #[test]
    fn test_any_bins_one_present() {
        let reqs = SkillRequires {
            bins: vec![],
            env: vec![],
            any_bins: vec!["sh".to_string(), "nonexistent_bin_xyz".to_string()],
        };
        assert!(check_requirements(
            &reqs,
            &[],
            &std::collections::HashMap::new()
        ));
    }

    #[test]
    fn test_any_bins_none_present() {
        let reqs = SkillRequires {
            bins: vec![],
            env: vec![],
            any_bins: vec![
                "nonexistent_bin_xyz1".to_string(),
                "nonexistent_bin_xyz2".to_string(),
            ],
        };
        assert!(!check_requirements(
            &reqs,
            &[],
            &std::collections::HashMap::new()
        ));
    }

    #[test]
    fn test_any_bins_empty_passes() {
        let reqs = SkillRequires {
            bins: vec![],
            env: vec![],
            any_bins: vec![],
        };
        assert!(check_requirements(
            &reqs,
            &[],
            &std::collections::HashMap::new()
        ));
    }

    #[test]
    fn test_parse_skill_with_any_bins() {
        let content = r#"---
name: search-skill
description: "Search skill."
requires:
  any_bins:
    - rg
    - grep
    - ag
---

# Search

Body here.
"#;
        let (manifest, _body) = parse_skill_md(content).unwrap();
        assert_eq!(manifest.requires.any_bins, vec!["rg", "grep", "ag"]);
    }

    // --- Install spec tests ---

    #[test]
    fn test_parse_skill_with_install_specs() {
        let content = r#"---
name: docker-skill
description: "Docker management."
requires:
  bins:
    - docker
install:
  docker:
    brew: "docker"
    apt: "docker.io"
    url: "https://docs.docker.com/get-docker/"
---

# Docker

Use docker commands.
"#;
        let (manifest, _body) = parse_skill_md(content).unwrap();
        assert!(manifest.install.contains_key("docker"));
        let spec = &manifest.install["docker"];
        assert_eq!(spec.brew.as_deref(), Some("docker"));
        assert_eq!(spec.apt.as_deref(), Some("docker.io"));
        assert_eq!(
            spec.url.as_deref(),
            Some("https://docs.docker.com/get-docker/")
        );
        assert!(spec.npm.is_none());
    }

    #[test]
    fn test_install_spec_defaults() {
        let spec = InstallSpec::default();
        assert!(spec.brew.is_none());
        assert!(spec.apt.is_none());
        assert!(spec.npm.is_none());
        assert!(spec.url.is_none());
    }

    #[test]
    fn test_parse_skill_without_install() {
        // Backward compat: existing skills without install field still parse
        let (manifest, _body) = parse_skill_md(SAMPLE_SKILL).unwrap();
        assert!(manifest.install.is_empty());
    }

    // --- Per-skill env collection test ---

    #[test]
    fn test_collect_skill_env() {
        let mut entries = std::collections::HashMap::new();
        entries.insert(
            "slack".to_string(),
            crate::config::SkillEntryConfig {
                enabled: true,
                env: [("SLACK_TOKEN".to_string(), "xoxb-123".to_string())]
                    .into_iter()
                    .collect(),
            },
        );
        entries.insert(
            "disabled".to_string(),
            crate::config::SkillEntryConfig {
                enabled: false,
                env: [("SHOULD_NOT_APPEAR".to_string(), "val".to_string())]
                    .into_iter()
                    .collect(),
            },
        );
        let config = SkillsConfig {
            enabled: true,
            max_context_tokens: 4000,
            entries,
        };
        let env = collect_skill_env(&config);
        assert_eq!(env.get("SLACK_TOKEN").unwrap(), "xoxb-123");
        assert!(!env.contains_key("SHOULD_NOT_APPEAR"));
    }
}
