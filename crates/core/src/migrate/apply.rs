use std::path::Path;

use anyhow::{Context, Result};

use crate::config::Config;

use super::{MigrationPlan, MigrationResult, SourceData};

/// Apply a migration plan to the Borg data directory.
pub fn apply_plan(
    plan: &MigrationPlan,
    data: &SourceData,
    borg_data_dir: &Path,
) -> Result<MigrationResult> {
    let mut result = MigrationResult {
        config_changes_applied: 0,
        credentials_added: 0,
        memory_files_copied: 0,
        persona_copied: false,
        skills_copied: 0,
        warnings: Vec::new(),
    };

    // Apply config changes
    apply_config_changes(plan, borg_data_dir, &mut result)?;

    // Apply credentials
    apply_credentials(plan, data, borg_data_dir, &mut result)?;

    // Copy memory files
    apply_memory_files(plan, borg_data_dir, &mut result)?;

    // Copy persona
    apply_persona(plan, borg_data_dir, &mut result)?;

    // Copy skills
    apply_skills(plan, borg_data_dir, &mut result)?;

    Ok(result)
}

fn apply_config_changes(
    plan: &MigrationPlan,
    borg_data_dir: &Path,
    result: &mut MigrationResult,
) -> Result<()> {
    let active_changes: Vec<_> = plan.config_changes.iter().filter(|c| !c.skipped).collect();

    if active_changes.is_empty() {
        return Ok(());
    }

    let config_path = borg_data_dir.join("config.toml");
    let mut config = Config::load_from(&config_path).unwrap_or_default();

    for change in &active_changes {
        match apply_config_value(&mut config, &change.key, &change.new_value) {
            Ok(()) => result.config_changes_applied += 1,
            Err(e) => result.warnings.push(format!("Config {}: {e}", change.key)),
        }
    }

    let content = toml::to_string_pretty(&config).with_context(|| "Failed to serialize config")?;
    std::fs::write(&config_path, content)
        .with_context(|| format!("Failed to write {}", config_path.display()))?;

    Ok(())
}

fn apply_config_value(config: &mut Config, key: &str, value: &str) -> Result<()> {
    match key {
        "llm.model" => config.llm.model = value.to_string(),
        "llm.provider" => config.llm.provider = Some(value.to_string()),
        "tools.default_timeout_ms" => {
            config.tools.default_timeout_ms = value
                .parse()
                .with_context(|| format!("Invalid timeout: {value}"))?;
        }
        "browser.headless" => {
            config.browser.headless = value
                .parse()
                .with_context(|| format!("Invalid bool: {value}"))?;
        }
        "user.timezone" => config.user.timezone = Some(value.to_string()),
        "sandbox.enabled" => {
            config.sandbox.enabled = value
                .parse()
                .with_context(|| format!("Invalid bool: {value}"))?;
        }
        "sandbox.mode" => config.sandbox.mode = value.to_string(),
        "compaction.model" => config.compaction.model = Some(value.to_string()),
        "tts.enabled" => {
            config.tts.enabled = value
                .parse()
                .with_context(|| format!("Invalid bool: {value}"))?;
        }
        "tts.default_voice" => config.tts.default_voice = value.to_string(),
        _ => anyhow::bail!("Unknown config key: {key}"),
    }
    Ok(())
}

fn apply_credentials(
    plan: &MigrationPlan,
    data: &SourceData,
    borg_data_dir: &Path,
    result: &mut MigrationResult,
) -> Result<()> {
    if plan.credentials_to_add.is_empty() {
        return Ok(());
    }

    let env_path = borg_data_dir.join(".env");
    let mut env_content = std::fs::read_to_string(&env_path).unwrap_or_default();

    for key_name in &plan.credentials_to_add {
        if let Some((_, value)) = data.credentials.iter().find(|(k, _)| k == key_name) {
            if !env_content.is_empty() && !env_content.ends_with('\n') {
                env_content.push('\n');
            }
            env_content.push_str(&format!(
                "{key_name}=\"{}\"\n",
                value.replace('\\', "\\\\").replace('"', "\\\"")
            ));
            result.credentials_added += 1;
        }
    }

    std::fs::write(&env_path, &env_content)
        .with_context(|| format!("Failed to write {}", env_path.display()))?;

    // Set restrictive permissions on .env
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

fn apply_memory_files(
    plan: &MigrationPlan,
    borg_data_dir: &Path,
    result: &mut MigrationResult,
) -> Result<()> {
    if plan.memory_files.is_empty() {
        return Ok(());
    }

    let memory_dir = borg_data_dir.join("memory");
    std::fs::create_dir_all(&memory_dir)?;

    for (src, dest_name) in &plan.memory_files {
        let dest = memory_dir.join(dest_name);
        match std::fs::copy(src, &dest) {
            Ok(_) => result.memory_files_copied += 1,
            Err(e) => result
                .warnings
                .push(format!("Failed to copy {}: {e}", src.display())),
        }
    }

    Ok(())
}

fn apply_persona(
    plan: &MigrationPlan,
    borg_data_dir: &Path,
    result: &mut MigrationResult,
) -> Result<()> {
    if let Some(src) = &plan.persona_file {
        let dest = borg_data_dir.join("IDENTITY.md");
        match std::fs::copy(src, &dest) {
            Ok(_) => result.persona_copied = true,
            Err(e) => result.warnings.push(format!("Failed to copy persona: {e}")),
        }
    }
    Ok(())
}

fn apply_skills(
    plan: &MigrationPlan,
    borg_data_dir: &Path,
    result: &mut MigrationResult,
) -> Result<()> {
    if plan.skill_dirs.is_empty() {
        return Ok(());
    }

    let skills_dir = borg_data_dir.join("skills");
    std::fs::create_dir_all(&skills_dir)?;

    for (src_dir, name) in &plan.skill_dirs {
        let dest_dir = skills_dir.join(name);
        match copy_dir_recursive(src_dir, &dest_dir) {
            Ok(()) => result.skills_copied += 1,
            Err(e) => result
                .warnings
                .push(format!("Failed to copy skill {name}: {e}")),
        }
    }

    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::{MigrationSource, PlanChange};
    use tempfile::TempDir;

    fn make_borg_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();
        dir
    }

    #[test]
    fn test_apply_config_changes() {
        let borg = make_borg_dir();
        // Write a minimal valid config
        std::fs::write(borg.path().join("config.toml"), "").unwrap();

        let plan = MigrationPlan {
            source: MigrationSource::Hermes,
            config_changes: vec![
                PlanChange {
                    key: "llm.model".into(),
                    source_key: "model.default".into(),
                    current_value: None,
                    new_value: "gpt-4o".into(),
                    skipped: false,
                },
                PlanChange {
                    key: "llm.provider".into(),
                    source_key: "model.provider".into(),
                    current_value: None,
                    new_value: "openai".into(),
                    skipped: false,
                },
                PlanChange {
                    key: "llm.model".into(),
                    source_key: "other".into(),
                    current_value: Some("existing".into()),
                    new_value: "should-skip".into(),
                    skipped: true,
                },
            ],
            credentials_to_add: vec![],
            memory_files: vec![],
            persona_file: None,
            skill_dirs: vec![],
        };

        let data = SourceData::default();
        let result = apply_plan(&plan, &data, borg.path()).unwrap();

        assert_eq!(result.config_changes_applied, 2);

        // Verify config was written
        let config = Config::load_from(&borg.path().join("config.toml")).unwrap();
        assert_eq!(config.llm.model, "gpt-4o");
        assert_eq!(config.llm.provider.as_deref(), Some("openai"));
    }

    #[test]
    fn test_apply_credentials_to_env() {
        let borg = make_borg_dir();

        let plan = MigrationPlan {
            source: MigrationSource::Hermes,
            config_changes: vec![],
            credentials_to_add: vec!["OPENAI_API_KEY".into(), "TELEGRAM_BOT_TOKEN".into()],
            memory_files: vec![],
            persona_file: None,
            skill_dirs: vec![],
        };

        let data = SourceData {
            credentials: vec![
                ("OPENAI_API_KEY".into(), "sk-test123".into()),
                ("TELEGRAM_BOT_TOKEN".into(), "bot-token".into()),
            ],
            ..Default::default()
        };

        let result = apply_plan(&plan, &data, borg.path()).unwrap();

        assert_eq!(result.credentials_added, 2);

        let env_content = std::fs::read_to_string(borg.path().join(".env")).unwrap();
        assert!(env_content.contains("OPENAI_API_KEY=\"sk-test123\""));
        assert!(env_content.contains("TELEGRAM_BOT_TOKEN=\"bot-token\""));
    }

    #[test]
    fn test_apply_copies_memory_files() {
        let borg = make_borg_dir();
        let src = TempDir::new().unwrap();
        std::fs::write(src.path().join("MEMORY.md"), "# My Memories").unwrap();

        let plan = MigrationPlan {
            source: MigrationSource::Hermes,
            config_changes: vec![],
            credentials_to_add: vec![],
            memory_files: vec![(src.path().join("MEMORY.md"), "hermes-MEMORY.md".into())],
            persona_file: None,
            skill_dirs: vec![],
        };

        let data = SourceData::default();
        let result = apply_plan(&plan, &data, borg.path()).unwrap();

        assert_eq!(result.memory_files_copied, 1);
        let content = std::fs::read_to_string(borg.path().join("memory/hermes-MEMORY.md")).unwrap();
        assert_eq!(content, "# My Memories");
    }

    #[test]
    fn test_apply_copies_persona() {
        let borg = make_borg_dir();
        let src = TempDir::new().unwrap();
        std::fs::write(src.path().join("SOUL.md"), "# My Persona").unwrap();

        let plan = MigrationPlan {
            source: MigrationSource::Hermes,
            config_changes: vec![],
            credentials_to_add: vec![],
            memory_files: vec![],
            persona_file: Some(src.path().join("SOUL.md")),
            skill_dirs: vec![],
        };

        let data = SourceData::default();
        let result = apply_plan(&plan, &data, borg.path()).unwrap();

        assert!(result.persona_copied);
        let content = std::fs::read_to_string(borg.path().join("IDENTITY.md")).unwrap();
        assert_eq!(content, "# My Persona");
    }

    #[test]
    fn test_apply_copies_skills() {
        let borg = make_borg_dir();
        let src = TempDir::new().unwrap();
        std::fs::create_dir_all(src.path().join("my-skill")).unwrap();
        std::fs::write(
            src.path().join("my-skill/SKILL.md"),
            "---\nname: my-skill\n---",
        )
        .unwrap();

        let plan = MigrationPlan {
            source: MigrationSource::Hermes,
            config_changes: vec![],
            credentials_to_add: vec![],
            memory_files: vec![],
            persona_file: None,
            skill_dirs: vec![(src.path().join("my-skill"), "my-skill".into())],
        };

        let data = SourceData::default();
        let result = apply_plan(&plan, &data, borg.path()).unwrap();

        assert_eq!(result.skills_copied, 1);
        assert!(borg.path().join("skills/my-skill/SKILL.md").exists());
    }

    #[test]
    fn test_apply_no_overwrite_existing_env() {
        let borg = make_borg_dir();
        std::fs::write(borg.path().join(".env"), "EXISTING_KEY=value\n").unwrap();

        let plan = MigrationPlan {
            source: MigrationSource::Hermes,
            config_changes: vec![],
            credentials_to_add: vec!["NEW_KEY".into()],
            memory_files: vec![],
            persona_file: None,
            skill_dirs: vec![],
        };

        let data = SourceData {
            credentials: vec![("NEW_KEY".into(), "new-value".into())],
            ..Default::default()
        };

        let result = apply_plan(&plan, &data, borg.path()).unwrap();

        assert_eq!(result.credentials_added, 1);
        let content = std::fs::read_to_string(borg.path().join(".env")).unwrap();
        assert!(content.contains("EXISTING_KEY=value"));
        assert!(content.contains("NEW_KEY=\"new-value\""));
    }
}
