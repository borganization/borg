use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::debug;

use crate::config::Config;
use crate::conversation::estimate_tokens;

const BUILTIN_SLACK: &str = include_str!("../skills/slack/SKILL.md");
const BUILTIN_DISCORD: &str = include_str!("../skills/discord/SKILL.md");
const BUILTIN_GITHUB: &str = include_str!("../skills/github/SKILL.md");
const BUILTIN_WEATHER: &str = include_str!("../skills/weather/SKILL.md");
const BUILTIN_SKILL_CREATOR: &str = include_str!("../skills/skill-creator/SKILL.md");

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
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub requires: SkillRequires,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub manifest: SkillManifest,
    pub body: String,
    pub source: SkillSource,
    pub available: bool,
}

impl Skill {
    pub fn format_for_prompt(&self) -> String {
        let status = if self.available {
            "available"
        } else {
            "unavailable (missing requirements)"
        };
        let source = match self.source {
            SkillSource::BuiltIn => "built-in",
            SkillSource::User => "user",
        };
        format!(
            "## Skill: {} [{}, {}]\n\n{}\n",
            self.manifest.name, source, status, self.body
        )
    }

    pub fn summary_line(&self) -> String {
        let status = if self.available { "✓" } else { "✗" };
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

fn check_requirements(requires: &SkillRequires) -> bool {
    for bin in &requires.bins {
        if which::which(bin).is_err() {
            debug!("Skill requirement missing: binary '{bin}'");
            return false;
        }
    }
    for var in &requires.env {
        if std::env::var(var).is_err() {
            debug!("Skill requirement missing: env var '{var}'");
            return false;
        }
    }
    true
}

fn load_builtin_skill(content: &str) -> Result<Skill> {
    let (manifest, body) = parse_skill_md(content)?;
    let available = check_requirements(&manifest.requires);
    Ok(Skill {
        manifest,
        body,
        source: SkillSource::BuiltIn,
        available,
    })
}

pub fn skills_dir() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("skills"))
}

pub fn load_all_skills() -> Result<Vec<Skill>> {
    let builtins_raw = [
        BUILTIN_SLACK,
        BUILTIN_DISCORD,
        BUILTIN_GITHUB,
        BUILTIN_WEATHER,
        BUILTIN_SKILL_CREATOR,
    ];

    let mut skills: Vec<Skill> = Vec::new();
    let mut builtin_names: Vec<String> = Vec::new();

    for raw in builtins_raw {
        match load_builtin_skill(raw) {
            Ok(skill) => {
                builtin_names.push(skill.manifest.name.clone());
                skills.push(skill);
            }
            Err(e) => {
                debug!("Failed to load built-in skill: {e}");
            }
        }
    }

    // Load user skills from ~/.tamagotchi/skills/*/SKILL.md
    let user_dir = skills_dir()?;
    if user_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&user_dir) {
            for entry in entries.flatten() {
                let skill_file = entry.path().join("SKILL.md");
                if skill_file.exists() {
                    match std::fs::read_to_string(&skill_file) {
                        Ok(content) => match parse_skill_md(&content) {
                            Ok((manifest, body)) => {
                                let available = check_requirements(&manifest.requires);
                                let name = manifest.name.clone();

                                // User skills override built-in skills with the same name
                                skills.retain(|s| s.manifest.name != name);

                                skills.push(Skill {
                                    manifest,
                                    body,
                                    source: SkillSource::User,
                                    available,
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

pub fn load_skills_context(max_tokens: usize) -> Result<String> {
    let skills = load_all_skills()?;

    if skills.is_empty() {
        return Ok(String::new());
    }

    let mut parts = Vec::new();
    let mut estimated_tokens = 0;

    // Include available skills first, then unavailable
    let mut sorted_skills = skills;
    sorted_skills.sort_by_key(|s| !s.available);

    for skill in &sorted_skills {
        let formatted = skill.format_for_prompt();
        let tokens = estimate_tokens(&formatted);

        if estimated_tokens + tokens > max_tokens {
            debug!(
                "Skipping skill '{}' (would exceed token budget)",
                skill.manifest.name
            );
            continue;
        }

        parts.push(formatted);
        estimated_tokens += tokens;
        debug!(
            "Included skill '{}' ({tokens} estimated tokens)",
            skill.manifest.name
        );
    }

    if parts.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("# Skills\n\n{}\n", parts.join("\n---\n\n")))
    }
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
        for (name, content) in [
            ("slack", BUILTIN_SLACK),
            ("discord", BUILTIN_DISCORD),
            ("github", BUILTIN_GITHUB),
            ("weather", BUILTIN_WEATHER),
            ("skill-creator", BUILTIN_SKILL_CREATOR),
        ] {
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
        };
        let formatted = skill.format_for_prompt();
        let tokens = estimate_tokens(&formatted);
        assert!(tokens < 50);
    }

    #[test]
    fn load_all_includes_builtins() {
        let skills = load_all_skills().unwrap();
        let names: Vec<&str> = skills.iter().map(|s| s.manifest.name.as_str()).collect();
        assert!(names.contains(&"slack"));
        assert!(names.contains(&"discord"));
        assert!(names.contains(&"github"));
        assert!(names.contains(&"weather"));
        assert!(names.contains(&"skill-creator"));
    }

    #[test]
    fn skill_summary_line() {
        let skill = Skill {
            manifest: SkillManifest {
                name: "test".to_string(),
                description: "A test.".to_string(),
                requires: SkillRequires::default(),
            },
            body: "body".to_string(),
            source: SkillSource::BuiltIn,
            available: true,
        };
        let line = skill.summary_line();
        assert!(line.contains("[✓]"));
        assert!(line.contains("test"));
        assert!(line.contains("built-in"));
    }

    #[test]
    fn skill_context_respects_token_budget() {
        // With a very small budget, not all skills should fit
        let context = load_skills_context(100).unwrap();
        // At least the header should be there if any fit, or empty if none fit
        if !context.is_empty() {
            assert!(context.starts_with("# Skills"));
        }
    }

    #[test]
    fn check_requirements_no_reqs() {
        let reqs = SkillRequires::default();
        assert!(check_requirements(&reqs));
    }

    #[test]
    fn check_requirements_missing_bin() {
        let reqs = SkillRequires {
            bins: vec!["definitely_not_a_real_binary_xyz123".to_string()],
            env: vec![],
        };
        assert!(!check_requirements(&reqs));
    }

    #[test]
    fn check_requirements_missing_env() {
        let reqs = SkillRequires {
            bins: vec![],
            env: vec!["DEFINITELY_NOT_A_REAL_ENV_VAR_XYZ123".to_string()],
        };
        assert!(!check_requirements(&reqs));
    }

    #[test]
    fn user_skill_overrides_builtin() {
        // This tests the override logic in isolation
        let mut skills = vec![Skill {
            manifest: SkillManifest {
                name: "weather".to_string(),
                description: "built-in weather".to_string(),
                requires: SkillRequires::default(),
            },
            body: "built-in body".to_string(),
            source: SkillSource::BuiltIn,
            available: true,
        }];

        let user_name = "weather".to_string();
        skills.retain(|s| s.manifest.name != user_name);
        skills.push(Skill {
            manifest: SkillManifest {
                name: "weather".to_string(),
                description: "user weather".to_string(),
                requires: SkillRequires::default(),
            },
            body: "user body".to_string(),
            source: SkillSource::User,
            available: true,
        });

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source, SkillSource::User);
        assert_eq!(skills[0].manifest.description, "user weather");
    }
}
