use anyhow::{bail, Result};

use std::str::FromStr;

use borg_core::config::Config;
use borg_core::provider::Provider;

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
];

/// Model choices per provider.
pub(crate) const OPENROUTER_MODELS: &[(&str, &str)] = &[
    ("anthropic/claude-sonnet-4", "Claude Sonnet 4 (recommended)"),
    ("anthropic/claude-haiku-4", "Claude Haiku 4 (fast, cheap)"),
    ("openai/gpt-4.1", "GPT-4.1"),
    ("openai/gpt-4.1-mini", "GPT-4.1 Mini (fast, cheap)"),
    ("openai/gpt-4.1-nano", "GPT-4.1 Nano (fastest)"),
    ("google/gemini-2.5-pro", "Gemini 2.5 Pro"),
    ("google/gemini-2.5-flash", "Gemini 2.5 Flash (fast)"),
    ("meta-llama/llama-4-maverick", "Llama 4 Maverick"),
    ("meta-llama/llama-4-scout", "Llama 4 Scout (fast)"),
    ("deepseek/deepseek-r1", "DeepSeek R1"),
    ("mistralai/mistral-large", "Mistral Large"),
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

/// Personality style presets.
pub(crate) struct PersonalityStyle {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) soul_snippet: &'static str,
}

pub(crate) const STYLES: &[PersonalityStyle] = &[
    PersonalityStyle {
        name: "Professional",
        description: "Clear, direct, business-appropriate tone",
        soul_snippet: r#"## Communication Style
- Professional and direct — no fluff, no filler
- Use structured responses with headers and bullet points when helpful
- Maintain a polished, business-appropriate tone
- Lead with the answer, then provide supporting detail
- Be decisive in recommendations while noting trade-offs"#,
    },
    PersonalityStyle {
        name: "Casual",
        description: "Friendly, relaxed, conversational",
        soul_snippet: r#"## Communication Style
- Casual and conversational — talk like a helpful friend
- Use natural language, contractions, and a relaxed tone
- Keep things light and approachable
- It's fine to use humor when it fits naturally
- Be warm and encouraging without being over-the-top"#,
    },
    PersonalityStyle {
        name: "Snarky",
        description: "Witty, dry humor, still helpful",
        soul_snippet: r#"## Communication Style
- Witty and dry — deliver help with a side of personality
- Use sarcasm sparingly but effectively (never mean-spirited)
- Keep a slightly irreverent tone while still being genuinely useful
- Playful roasts are fine, but always prioritize actually helping
- Think helpful friend who happens to be funny, not comedian who happens to help"#,
    },
    PersonalityStyle {
        name: "Nurturing",
        description: "Encouraging, patient, supportive mentor",
        soul_snippet: r#"## Communication Style
- Warm, patient, and encouraging — like a great mentor
- Celebrate progress and acknowledge effort
- Break down complex topics gently, never condescend
- Offer reassurance when tasks feel overwhelming
- Guide with questions when it helps the user learn"#,
    },
    PersonalityStyle {
        name: "Minimal",
        description: "Terse, to the point, no frills",
        soul_snippet: r#"## Communication Style
- Maximum brevity — say it in as few words as possible
- Skip pleasantries, greetings, and filler
- Use code and commands over prose when possible
- One-line answers are preferred when they suffice
- Only elaborate when explicitly asked"#,
    },
];

/// How the API key should be stored.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyStorage {
    /// Store in ~/.borg/.env (legacy plaintext)
    EnvFile,
    /// Store in OS keychain (macOS Keychain / Linux secret-tool)
    Keychain,
}

/// Assembled choices from the onboarding wizard.
pub struct OnboardingResult {
    pub user_name: String,
    pub agent_name: String,
    pub style_index: usize,
    pub model_id: String,
    pub api_key: Option<String>,
    pub key_storage: KeyStorage,
    pub provider: String,
    pub monthly_token_limit: u64,
}

/// Get the model list for a given provider.
pub(crate) fn models_for_provider(provider_id: &str) -> &'static [(&'static str, &'static str)] {
    match provider_id {
        "openai" => OPENAI_MODELS,
        "anthropic" => ANTHROPIC_MODELS,
        "gemini" => GEMINI_MODELS,
        _ => OPENROUTER_MODELS,
    }
}

pub(crate) fn provider_id_to_display(id: &str) -> &str {
    PROVIDERS
        .iter()
        .find(|(pid, _, _)| *pid == id)
        .map(|(_, name, _)| *name)
        .unwrap_or(id)
}

pub(crate) fn provider_key_url(provider_id: &str) -> &'static str {
    match provider_id {
        "openrouter" => "https://openrouter.ai/keys",
        "openai" => "https://platform.openai.com/api-keys",
        "anthropic" => "https://console.anthropic.com/settings/keys",
        "gemini" => "https://aistudio.google.com/apikey",
        _ => "your provider's website",
    }
}

/// Run the interactive onboarding wizard. Returns `None` if the user cancels.
pub fn run_onboarding() -> Result<Option<OnboardingResult>> {
    crate::onboarding_tui::run()
}

/// Generate SOUL.md content from onboarding choices.
pub fn generate_soul(name: &str, style_index: usize, owner_name: &str) -> Result<String> {
    let style = STYLES
        .get(style_index)
        .ok_or_else(|| anyhow::anyhow!("invalid style index {style_index}"))?;

    Ok(format!(
        r#"# {name} — Your AI Personal Assistant

You are {name}, a helpful AI personal assistant. You belong to {owner_name} and live on their computer. You help them with tasks, remember things for them, and occasionally check in to see how they're doing.

## Personality
- Proactive when you notice something useful
- Honest about your limitations
- You remember context from past conversations

{style_snippet}

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
        name = name,
        style_snippet = style.soul_snippet,
    ))
}

/// Format a number with comma separators (e.g. 1000000 → "1,000,000").
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
    borg_customizations::keychain::available()
}

/// Store an API key in the OS keychain.
fn store_in_keychain(provider_id: &str, api_key: &str) -> Result<()> {
    let service_name = format!("borg-{provider_id}");
    borg_customizations::keychain::store(&service_name, "borg", api_key)
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

/// Generate config.toml content from onboarding choices.
pub fn generate_config(
    model_id: &str,
    provider_id: &str,
    user_name: &str,
    agent_name: &str,
    monthly_token_limit: u64,
    key_storage: &KeyStorage,
) -> Result<String> {
    validate_model_id(model_id)?;

    let provider = Provider::from_str(provider_id)?;
    let env_var = provider.default_env_var();

    let api_key_line = match key_storage {
        KeyStorage::Keychain => {
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
        }
        KeyStorage::EnvFile => {
            format!(r#"api_key_env = "{env_var}""#)
        }
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

[heartbeat]
enabled = false
interval = "30m"
quiet_hours_start = "23:00"
quiet_hours_end = "07:00"

[tools]
default_timeout_ms = 30000

[sandbox]
enabled = true
mode = "strict"

[memory]
max_context_tokens = 8000

[skills]
enabled = true
max_context_tokens = 4000

[budget]
monthly_token_limit = {monthly_token_limit}
warning_threshold = 0.8
"#,
    ))
}

/// Apply onboarding results: create directories, write config and soul files.
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
            result.monthly_token_limit,
            &result.key_storage,
        )?;
        std::fs::write(&config_path, &config_content)?;
        println!("  Created {}", config_path.display());
    }

    // Write SOUL.md (skip if already exists)
    let soul_path = data_dir.join("SOUL.md");
    if soul_path.exists() {
        println!("  Skipped {} (already exists)", soul_path.display());
    } else {
        let soul_content =
            generate_soul(&result.agent_name, result.style_index, &result.user_name)?;
        std::fs::write(&soul_path, &soul_content)?;
        println!("  Created {}", soul_path.display());
    }

    // Write MEMORY.md (skip if already exists)
    let memory_path = data_dir.join("MEMORY.md");
    if memory_path.exists() {
        println!("  Skipped {} (already exists)", memory_path.display());
    } else {
        std::fs::write(&memory_path, "# Memory Index\n\nNo memories yet.\n")?;
        println!("  Created {}", memory_path.display());
    }

    // Store API key based on chosen storage method
    if let Some(ref api_key) = result.api_key {
        match result.key_storage {
            KeyStorage::Keychain => match store_in_keychain(&result.provider, api_key) {
                Ok(()) => {
                    println!(
                        "  Stored API key in OS keychain (service: borg-{})",
                        result.provider
                    );
                }
                Err(e) => {
                    eprintln!("  Warning: Failed to store in keychain: {e}");
                    eprintln!("  Falling back to .env file");
                    // Rewrite config to use api_key_env instead of SecretRef exec,
                    // so the config matches the actual storage location.
                    let fallback_config = generate_config(
                        &result.model_id,
                        &result.provider,
                        &result.user_name,
                        &result.agent_name,
                        result.monthly_token_limit,
                        &KeyStorage::EnvFile,
                    )?;
                    std::fs::write(&config_path, &fallback_config)?;
                    let provider = Provider::from_str(&result.provider)?;
                    let env_var = provider.default_env_var();
                    let env_path = data_dir.join(".env");
                    std::fs::write(&env_path, format!("{env_var}={api_key}\n"))?;
                    println!("  Updated config to use .env file");
                    println!("  Created {}", env_path.display());
                }
            },
            KeyStorage::EnvFile => {
                let provider = Provider::from_str(&result.provider)?;
                let env_var = provider.default_env_var();
                let env_path = data_dir.join(".env");
                std::fs::write(&env_path, format!("{env_var}={api_key}\n"))?;
                // Restrict permissions on .env file
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600));
                }
                println!("  Created {}", env_path.display());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_soul_contains_name_and_style() {
        let soul = generate_soul("Buddy", 0, "Mike").expect("valid style index");
        assert!(soul.contains("# Buddy — Your AI Personal Assistant"));
        assert!(soul.contains("You are Buddy"));
        assert!(soul.contains("You belong to Mike"));
        assert!(soul.contains("## Communication Style"));
        assert!(soul.contains("Professional and direct"));
    }

    #[test]
    fn generate_soul_all_styles_valid() {
        for i in 0..STYLES.len() {
            let soul = generate_soul("Test", i, "Owner").expect("valid style index");
            assert!(soul.contains(STYLES[i].name) || soul.contains("Communication Style"));
        }
    }

    #[test]
    fn generate_soul_invalid_index_errors() {
        assert!(generate_soul("Test", 999, "Owner").is_err());
    }

    #[test]
    fn generate_config_default_model() {
        let config = generate_config(
            "anthropic/claude-sonnet-4",
            "openrouter",
            "Mike",
            "Buddy",
            1_000_000,
            &KeyStorage::EnvFile,
        )
        .expect("valid model");
        assert!(config.contains("model = \"anthropic/claude-sonnet-4\""));
        assert!(config.contains("provider = \"openrouter\""));
        assert!(config.contains("api_key_env = \"OPENROUTER_API_KEY\""));
        assert!(config.contains("[llm]"));
        assert!(config.contains("[sandbox]"));
    }

    #[test]
    fn generate_config_includes_user_section() {
        let config = generate_config(
            "anthropic/claude-sonnet-4",
            "openrouter",
            "Mike",
            "Buddy",
            1_000_000,
            &KeyStorage::EnvFile,
        )
        .expect("valid model");
        assert!(config.contains("[user]"));
        assert!(config.contains("name = \"Mike\""));
        assert!(config.contains("agent_name = \"Buddy\""));
    }

    #[test]
    fn generate_config_anthropic_provider() {
        let config = generate_config(
            "claude-sonnet-4",
            "anthropic",
            "User",
            "Agent",
            500_000,
            &KeyStorage::EnvFile,
        )
        .expect("valid");
        assert!(config.contains("provider = \"anthropic\""));
        assert!(config.contains("api_key_env = \"ANTHROPIC_API_KEY\""));
    }

    #[test]
    fn generate_config_openai_provider() {
        let config = generate_config(
            "gpt-4.1",
            "openai",
            "User",
            "Agent",
            0,
            &KeyStorage::EnvFile,
        )
        .expect("valid");
        assert!(config.contains("provider = \"openai\""));
        assert!(config.contains("api_key_env = \"OPENAI_API_KEY\""));
    }

    #[test]
    fn generate_config_gemini_provider() {
        let config = generate_config(
            "gemini-2.5-pro",
            "gemini",
            "User",
            "Agent",
            0,
            &KeyStorage::EnvFile,
        )
        .expect("valid");
        assert!(config.contains("provider = \"gemini\""));
        assert!(config.contains("api_key_env = \"GEMINI_API_KEY\""));
    }

    #[test]
    fn generate_config_rejects_empty_model() {
        assert!(
            generate_config("", "openrouter", "User", "Agent", 0, &KeyStorage::EnvFile).is_err()
        );
    }

    #[test]
    fn generate_config_rejects_injection() {
        assert!(generate_config(
            "model\"\nmalicious = true",
            "openrouter",
            "User",
            "Agent",
            0,
            &KeyStorage::EnvFile,
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
    fn generate_config_includes_budget_section() {
        let config = generate_config(
            "anthropic/claude-sonnet-4",
            "openrouter",
            "Mike",
            "Buddy",
            1_000_000,
            &KeyStorage::EnvFile,
        )
        .expect("valid");
        assert!(config.contains("[budget]"));
        assert!(config.contains("monthly_token_limit = 1000000"));
        assert!(config.contains("warning_threshold = 0.8"));
    }

    #[test]
    fn generate_config_budget_unlimited() {
        let config = generate_config(
            "anthropic/claude-sonnet-4",
            "openrouter",
            "User",
            "Agent",
            0,
            &KeyStorage::EnvFile,
        )
        .expect("valid");
        assert!(config.contains("monthly_token_limit = 0"));
    }

    #[test]
    fn generate_config_keychain_storage() {
        let config = generate_config(
            "anthropic/claude-sonnet-4",
            "openrouter",
            "User",
            "Agent",
            0,
            &KeyStorage::Keychain,
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
}
