use anyhow::{bail, Result};

use std::str::FromStr;

use borg_core::config::Config;
use borg_core::provider::Provider;

/// Auto-detect the system's IANA timezone (e.g. "America/New_York").
#[allow(dead_code)]
pub(crate) fn detect_system_timezone() -> Option<String> {
    iana_time_zone::get_timezone().ok()
}

/// Provider choices with display labels.
pub(crate) const PROVIDERS: &[(&str, &str, &str)] = &[
    (
        "openrouter",
        "OpenRouter",
        "Access all models via one API key",
    ),
    ("openai", "OpenAI", "GPT models directly"),
    ("anthropic", "Anthropic", "Claude models directly"),
    ("gemini", "Gemini", "Google Gemini models directly"),
    ("ollama", "Ollama", "Run models locally (no API key)"),
];

/// Model choices per provider.
pub(crate) const OPENROUTER_MODELS: &[(&str, &str)] = &[
    ("anthropic/claude-sonnet-4", "Claude Sonnet 4 (recommended)"),
    ("anthropic/claude-haiku-4", "Claude Haiku 4 (fast, cheap)"),
    ("openai/gpt-4.1", "GPT-4.1"),
    ("openai/gpt-4.1-mini", "GPT-4.1 Mini (fast, cheap)"),
    ("google/gemini-2.5-pro", "Gemini 2.5 Pro"),
    ("deepseek/deepseek-r1", "DeepSeek R1 (reasoning)"),
];

pub(crate) const OPENAI_MODELS: &[(&str, &str)] = &[
    ("gpt-4.1", "GPT-4.1 (recommended)"),
    ("gpt-4.1-mini", "GPT-4.1 Mini (fast, cheap)"),
    ("gpt-4.1-nano", "GPT-4.1 Nano (fastest)"),
    ("o3-mini", "o3-mini (reasoning)"),
];

pub(crate) const ANTHROPIC_MODELS: &[(&str, &str)] = &[
    ("claude-sonnet-4", "Claude Sonnet 4 (recommended)"),
    ("claude-haiku-4", "Claude Haiku 4 (fast, cheap)"),
    ("claude-opus-4", "Claude Opus 4 (most capable)"),
];

pub(crate) const GEMINI_MODELS: &[(&str, &str)] = &[
    ("gemini-2.5-pro", "Gemini 2.5 Pro (recommended)"),
    ("gemini-2.5-flash", "Gemini 2.5 Flash (fast)"),
];

pub(crate) const OLLAMA_MODELS: &[(&str, &str)] = &[
    ("llama3.3", "Llama 3.3 70B (recommended)"),
    ("qwen2.5", "Qwen 2.5 (versatile)"),
    ("deepseek-r1", "DeepSeek R1 (reasoning)"),
    ("mistral", "Mistral 7B (fast, lightweight)"),
    ("codellama", "Code Llama (coding)"),
];

/// Assembled choices from the onboarding wizard.
pub struct OnboardingResult {
    pub user_name: String,
    pub agent_name: String,
    pub model_id: String,
    pub api_key: Option<String>,
    pub provider: String,
}

/// Get the model list for a given provider.
pub(crate) fn models_for_provider(provider_id: &str) -> &'static [(&'static str, &'static str)] {
    match provider_id {
        "openai" => OPENAI_MODELS,
        "anthropic" => ANTHROPIC_MODELS,
        "gemini" => GEMINI_MODELS,
        "ollama" => OLLAMA_MODELS,
        _ => OPENROUTER_MODELS,
    }
}

/// Run the interactive onboarding wizard. Returns `None` if the user cancels.
pub fn run_onboarding() -> Result<Option<OnboardingResult>> {
    crate::onboarding_tui::run()
}

/// Generate IDENTITY.md content from onboarding choices.
pub fn generate_identity(name: &str, owner_name: &str) -> String {
    format!(
        r#"# {name} — Your AI Personal Assistant

You are {name}, a helpful AI personal assistant. You belong to {owner_name} and live on their computer. You help them with tasks, remember things for them, and occasionally check in to see how they're doing.

## Personality
- Proactive when you notice something useful
- Honest about your limitations
- You remember context from past conversations

## Communication Style
- Professional and direct — no fluff, no filler
- Use structured responses with headers and bullet points when helpful
- Maintain a polished, business-appropriate tone
- Lead with the answer, then provide supporting detail
- Be decisive in recommendations while noting trade-offs

## Capabilities
- You can create and use tools (scripts) to extend your abilities
- You can remember information across sessions
- You can check in proactively via the heartbeat system
- You can read and modify files in your tools directory

## Guidelines
- When creating tools, prefer simple implementations
- Always explain what you're doing before executing tools
- Respect the user's time and attention
"#,
    )
}

/// Generate SETUP.md content for the agent's first conversation.
pub fn generate_setup(agent_name: &str, owner_name: &str) -> String {
    format!(
        r#"# First Conversation

This is your very first conversation. You have just been set up by {owner_name}.
You are {agent_name} — a brand new AI personal assistant. Fresh start, no memories yet.

Introduce yourself naturally. Don't be another "Hello! How may I assist you today?" bot.

Things to figure out together:
- Get to know {owner_name} — what should you call them? What's their timezone?
- What's your vibe? Be genuine, not corporate. Show some personality.
- What does {owner_name} need help with? What matters to them?
- Set the tone for your working relationship.
- Ask if they'd like to connect a messaging channel (Telegram, Slack, Discord, etc.) — they can run `borg add <channel>` anytime.

Be yourself. Be curious. Make this first conversation count.
"#,
    )
}

/// Format a number with comma separators (e.g. 1000000 → "1,000,000").
#[cfg(test)]
pub(crate) fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Check if a system keychain is available for secret storage.
pub(crate) fn keychain_available() -> bool {
    borg_plugins::keychain::available()
}

/// Store an API key in the OS keychain.
fn store_in_keychain(provider_id: &str, api_key: &str) -> Result<()> {
    let service_name = format!("borg-{provider_id}");
    borg_plugins::keychain::store(&service_name, "borg", api_key)
}

/// Validate that a model ID is safe for TOML interpolation (alphanumeric, slashes, hyphens, dots).
fn validate_model_id(model_id: &str) -> Result<()> {
    if model_id.is_empty() {
        bail!("model ID cannot be empty");
    }
    if model_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '.' | '_'))
    {
        Ok(())
    } else {
        bail!("model ID contains invalid characters: {model_id}")
    }
}

/// Escape a string value for safe embedding in TOML double-quoted strings.
fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
        .replace('\r', "")
}

/// Generate config.toml content from onboarding choices.
pub fn generate_config(
    model_id: &str,
    provider_id: &str,
    user_name: &str,
    agent_name: &str,
    use_keychain: bool,
) -> Result<String> {
    validate_model_id(model_id)?;
    let user_name = escape_toml_string(user_name);
    let agent_name = escape_toml_string(agent_name);

    let provider = Provider::from_str(provider_id)?;

    let api_key_line = if !provider.requires_api_key() {
        // Keyless providers (e.g., Ollama) — no API key config needed
        "# No API key required for local provider\n# base_url = \"http://localhost:11434/v1/chat/completions\"  # uncomment to override".to_string()
    } else if use_keychain {
        let service_name = format!("borg-{provider_id}");
        if cfg!(target_os = "macos") {
            format!(
                r#"api_key = {{ source = "exec", command = "security", args = ["find-generic-password", "-s", "{service_name}", "-a", "borg", "-w"] }}"#,
            )
        } else {
            format!(
                r#"api_key = {{ source = "exec", command = "secret-tool", args = ["lookup", "service", "borg", "provider", "{provider_id}"] }}"#,
            )
        }
    } else {
        let env_var = provider.default_env_var();
        format!(r#"api_key_env = "{env_var}""#)
    };

    Ok(format!(
        r#"[user]
name = "{user_name}"
agent_name = "{agent_name}"

[llm]
provider = "{provider_id}"
{api_key_line}
model = "{model_id}"
temperature = 0.7
max_tokens = 4096
"#,
    ))
}

/// Apply onboarding results: create directories, write config and identity files.
pub fn apply_onboarding(result: &OnboardingResult) -> Result<()> {
    let data_dir = Config::data_dir()?;

    // Create directory structure
    for sub in &["memory", "tools", "skills", "logs", "cache"] {
        std::fs::create_dir_all(data_dir.join(sub))?;
    }

    // Install bundled skills to filesystem
    match borg_core::skills::install_default_skills(&data_dir) {
        Ok(0) => {}
        Ok(n) => println!("  Installed {n} default skill(s)"),
        Err(e) => eprintln!("  Warning: failed to install default skills: {e}"),
    }

    // Determine key storage automatically
    let use_keychain = result.api_key.is_some() && keychain_available();

    // Write config.toml (skip if already exists to avoid clobbering manual edits)
    let config_path = data_dir.join("config.toml");
    if config_path.exists() {
        println!("  Skipped {} (already exists)", config_path.display());
    } else {
        let config_content = generate_config(
            &result.model_id,
            &result.provider,
            &result.user_name,
            &result.agent_name,
            use_keychain,
        )?;
        std::fs::write(&config_path, &config_content)?;
        println!("  Created {}", config_path.display());
    }

    // Write IDENTITY.md (skip if already exists)
    let identity_path = data_dir.join("IDENTITY.md");
    if identity_path.exists() {
        println!("  Skipped {} (already exists)", identity_path.display());
    } else {
        let identity_content = generate_identity(&result.agent_name, &result.user_name);
        std::fs::write(&identity_path, &identity_content)?;
        println!("  Created {}", identity_path.display());
    }

    // Write MEMORY.md (skip if already exists)
    let memory_path = data_dir.join("MEMORY.md");
    if memory_path.exists() {
        println!("  Skipped {} (already exists)", memory_path.display());
    } else {
        std::fs::write(&memory_path, "# Memory Index\n\nNo memories yet.\n")?;
        println!("  Created {}", memory_path.display());
    }

    // Write SETUP.md for first conversation instructions
    let setup_path = data_dir.join("SETUP.md");
    if !setup_path.exists() {
        let setup_content = generate_setup(&result.agent_name, &result.user_name);
        std::fs::write(&setup_path, &setup_content)?;
        println!("  Created {}", setup_path.display());
    }

    // Store API key based on chosen storage method
    if let Some(ref api_key) = result.api_key {
        if use_keychain {
            match store_in_keychain(&result.provider, api_key) {
                Ok(()) => {
                    println!(
                        "  Stored API key in OS keychain (service: borg-{})",
                        result.provider
                    );
                }
                Err(e) => {
                    eprintln!("  Warning: Failed to store in keychain: {e}");
                    eprintln!("  Falling back to .env file");
                    let fallback_config = generate_config(
                        &result.model_id,
                        &result.provider,
                        &result.user_name,
                        &result.agent_name,
                        false,
                    )?;
                    std::fs::write(&config_path, &fallback_config)?;
                    let provider = Provider::from_str(&result.provider)?;
                    let env_var = provider.default_env_var();
                    let env_path = data_dir.join(".env");
                    let clean_key = api_key.trim().replace(['\n', '\r'], "");
                    std::fs::write(&env_path, format!("{env_var}={clean_key}\n"))?;
                    println!("  Updated config to use .env file");
                    println!("  Created {}", env_path.display());
                }
            }
        } else {
            let provider = Provider::from_str(&result.provider)?;
            let env_var = provider.default_env_var();
            let env_path = data_dir.join(".env");
            let clean_key = api_key.trim().replace(['\n', '\r'], "");
            std::fs::write(&env_path, format!("{env_var}={clean_key}\n"))?;
            // Restrict permissions on .env file
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600));
            }
            println!("  Created {}", env_path.display());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_identity_contains_name_and_style() {
        let identity = generate_identity("Buddy", "Mike");
        assert!(identity.contains("# Buddy — Your AI Personal Assistant"));
        assert!(identity.contains("You are Buddy"));
        assert!(identity.contains("You belong to Mike"));
        assert!(identity.contains("## Communication Style"));
        assert!(identity.contains("Professional and direct"));
    }

    #[test]
    fn generate_config_default_model() {
        let config = generate_config(
            "anthropic/claude-sonnet-4",
            "openrouter",
            "Mike",
            "Buddy",
            false,
        )
        .expect("valid model");
        assert!(config.contains("model = \"anthropic/claude-sonnet-4\""));
        assert!(config.contains("provider = \"openrouter\""));
        assert!(config.contains("api_key_env = \"OPENROUTER_API_KEY\""));
        assert!(config.contains("[llm]"));
    }

    #[test]
    fn generate_config_includes_user_section() {
        let config = generate_config(
            "anthropic/claude-sonnet-4",
            "openrouter",
            "Mike",
            "Buddy",
            false,
        )
        .expect("valid model");
        assert!(config.contains("[user]"));
        assert!(config.contains("name = \"Mike\""));
        assert!(config.contains("agent_name = \"Buddy\""));
    }

    #[test]
    fn generate_config_anthropic_provider() {
        let config =
            generate_config("claude-sonnet-4", "anthropic", "User", "Agent", false).expect("valid");
        assert!(config.contains("provider = \"anthropic\""));
        assert!(config.contains("api_key_env = \"ANTHROPIC_API_KEY\""));
    }

    #[test]
    fn generate_config_openai_provider() {
        let config = generate_config("gpt-4.1", "openai", "User", "Agent", false).expect("valid");
        assert!(config.contains("provider = \"openai\""));
        assert!(config.contains("api_key_env = \"OPENAI_API_KEY\""));
    }

    #[test]
    fn generate_config_gemini_provider() {
        let config =
            generate_config("gemini-2.5-pro", "gemini", "User", "Agent", false).expect("valid");
        assert!(config.contains("provider = \"gemini\""));
        assert!(config.contains("api_key_env = \"GEMINI_API_KEY\""));
    }

    #[test]
    fn generate_config_rejects_empty_model() {
        assert!(generate_config("", "openrouter", "User", "Agent", false).is_err());
    }

    #[test]
    fn generate_config_rejects_injection() {
        assert!(generate_config(
            "model\"\nmalicious = true",
            "openrouter",
            "User",
            "Agent",
            false,
        )
        .is_err());
    }

    #[test]
    fn models_for_all_providers_non_empty() {
        for (id, _, _) in PROVIDERS {
            let models = models_for_provider(id);
            assert!(!models.is_empty(), "models for {id} should not be empty");
        }
    }

    #[test]
    fn validate_model_id_accepts_valid() {
        assert!(validate_model_id("anthropic/claude-sonnet-4").is_ok());
        assert!(validate_model_id("openai/gpt-4.1-mini").is_ok());
        assert!(validate_model_id("meta-llama/llama-4-scout").is_ok());
        assert!(validate_model_id("llama3.3").is_ok());
    }

    #[test]
    fn validate_model_id_rejects_invalid() {
        assert!(validate_model_id("").is_err());
        assert!(validate_model_id("model\"").is_err());
        assert!(validate_model_id("model\nkey=val").is_err());
        assert!(validate_model_id("model with spaces").is_err());
    }

    #[test]
    fn format_number_basic() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1_000_000), "1,000,000");
        assert_eq!(format_number(500_000), "500,000");
        assert_eq!(format_number(5_000_000), "5,000,000");
    }

    #[test]
    fn generate_config_keychain_storage() {
        let config = generate_config(
            "anthropic/claude-sonnet-4",
            "openrouter",
            "User",
            "Agent",
            true,
        )
        .expect("valid");
        assert!(config.contains("api_key = {"));
        assert!(config.contains("source = \"exec\""));
        // Should NOT contain api_key_env when using keychain
        assert!(!config.contains("api_key_env"));
    }

    #[test]
    fn keychain_available_returns_bool() {
        // Just verify it doesn't panic — actual availability depends on platform
        let _available = keychain_available();
    }

    #[test]
    fn generate_config_ollama_provider() {
        let config =
            generate_config("llama3.3", "ollama", "Mike", "Buddy", false).expect("valid config");
        assert!(config.contains("provider = \"ollama\""));
        assert!(config.contains("model = \"llama3.3\""));
        // Ollama config should NOT contain api_key_env or api_key = { lines
        assert!(!config.contains("api_key_env"));
        assert!(!config.contains("api_key = {"));
    }

    #[test]
    fn generate_config_ollama_keychain_still_no_api_key() {
        // Even with keychain flag, Ollama should skip API key config
        let config =
            generate_config("llama3.3", "ollama", "Mike", "Buddy", true).expect("valid config");
        assert!(config.contains("provider = \"ollama\""));
        assert!(!config.contains("api_key_env"));
        assert!(!config.contains("api_key = {"));
    }

    #[test]
    fn ollama_models_non_empty() {
        assert!(!OLLAMA_MODELS.is_empty());
        for (id, _) in OLLAMA_MODELS {
            assert!(validate_model_id(id).is_ok(), "invalid model id: {id}");
        }
    }

    #[test]
    fn models_for_provider_ollama() {
        let models = models_for_provider("ollama");
        assert_eq!(models.len(), OLLAMA_MODELS.len());
    }

    #[test]
    fn openrouter_models_trimmed() {
        assert!(
            OPENROUTER_MODELS.len() <= 7,
            "OpenRouter models should be trimmed to ~6"
        );
        assert!(
            OPENROUTER_MODELS.len() >= 5,
            "OpenRouter models should have at least 5"
        );
    }
}
