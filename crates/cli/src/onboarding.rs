use anyhow::{bail, Result};
use crossterm::{
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    ExecutableCommand,
};
use inquire::{Password, Select, Text};
use std::io;

use tamagotchi_core::config::Config;
use tamagotchi_core::provider::Provider;

/// Provider choices with display labels.
const PROVIDERS: &[(&str, &str, &str)] = &[
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
const OPENROUTER_MODELS: &[(&str, &str)] = &[
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

const OPENAI_MODELS: &[(&str, &str)] = &[
    ("gpt-4.1", "GPT-4.1 (recommended)"),
    ("gpt-4.1-mini", "GPT-4.1 Mini (fast, cheap)"),
    ("gpt-4.1-nano", "GPT-4.1 Nano (fastest)"),
    ("o3-mini", "o3-mini (reasoning)"),
];

const ANTHROPIC_MODELS: &[(&str, &str)] = &[
    ("claude-sonnet-4", "Claude Sonnet 4 (recommended)"),
    ("claude-haiku-4", "Claude Haiku 4 (fast, cheap)"),
    ("claude-opus-4", "Claude Opus 4 (most capable)"),
];

const GEMINI_MODELS: &[(&str, &str)] = &[
    ("gemini-2.5-pro", "Gemini 2.5 Pro (recommended)"),
    ("gemini-2.5-flash", "Gemini 2.5 Flash (fast)"),
];

/// Personality style presets.
struct PersonalityStyle {
    name: &'static str,
    description: &'static str,
    soul_snippet: &'static str,
}

const STYLES: &[PersonalityStyle] = &[
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

/// Assembled choices from the onboarding wizard.
pub struct OnboardingResult {
    pub agent_name: String,
    pub style_index: usize,
    pub model_id: String,
    pub api_key: Option<String>,
    pub provider: String,
}

/// Helper: prompt the user for input, returning `None` on cancel/interrupt.
macro_rules! prompt_or_cancel {
    ($prompt:expr) => {
        match $prompt {
            Ok(val) => val,
            Err(
                inquire::InquireError::OperationCanceled
                | inquire::InquireError::OperationInterrupted,
            ) => {
                println!("\nSetup cancelled.");
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        }
    };
}

fn print_checkmark(stdout: &mut io::Stdout, label: &str, value: &str) -> Result<()> {
    stdout.execute(SetForegroundColor(Color::Green))?;
    stdout.execute(SetAttribute(Attribute::Bold))?;
    stdout.execute(Print("  ✓ "))?;
    stdout.execute(ResetColor)?;
    stdout.execute(SetAttribute(Attribute::Reset))?;
    stdout.execute(Print(format!("{label}  {value}\n")))?;
    Ok(())
}

/// Get the model list for a given provider.
fn models_for_provider(provider_id: &str) -> &'static [(&'static str, &'static str)] {
    match provider_id {
        "openai" => OPENAI_MODELS,
        "anthropic" => ANTHROPIC_MODELS,
        "gemini" => GEMINI_MODELS,
        _ => OPENROUTER_MODELS,
    }
}

fn provider_id_to_display(id: &str) -> &str {
    PROVIDERS
        .iter()
        .find(|(pid, _, _)| *pid == id)
        .map(|(_, name, _)| *name)
        .unwrap_or(id)
}

fn provider_key_url(provider_id: &str) -> &'static str {
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
    let mut stdout = io::stdout();

    // ── Banner ──
    crate::logo::print_logo()?;

    println!("  Let's set up your personal AI assistant.\n");

    // ── Step 1: Agent name ──
    let agent_name = prompt_or_cancel!(Text::new("What should your agent be called?")
        .with_default("Tamagotchi")
        .prompt());

    // ── Step 2: Personality style ──
    let style_options: Vec<String> = STYLES
        .iter()
        .map(|s| format!("{} — {}", s.name, s.description))
        .collect();

    let chosen_style = prompt_or_cancel!(Select::new("Pick a personality style:", style_options)
        .with_help_message("Defines the tone of your agent's responses")
        .prompt());

    let style_index = STYLES
        .iter()
        .position(|s| chosen_style.starts_with(s.name))
        .unwrap_or(0);

    // ── Step 3: Model selection ──
    let model_options: Vec<String> = MODELS
        .iter()
        .map(|(_, label)| (*label).to_string())
        .collect();

    let chosen_model = prompt_or_cancel!(Select::new("Choose your default model:", model_options)
        .with_help_message(
            "All models served via OpenRouter — you can change this later in config.toml"
        )
        .prompt());

    let model_id = MODELS
        .iter()
        .find(|(_, label)| *label == chosen_model.as_str())
        .map(|(id, _)| (*id).to_string())
        .unwrap_or_else(|| "anthropic/claude-sonnet-4".to_string());

    // ── Step 4: API key ──
    let data_dir = Config::data_dir()?;
    let env_path = data_dir.join(".env");
    let existing_key = if env_path.exists() {
        std::fs::read_to_string(&env_path)
            .ok()
            .and_then(|contents| {
                contents.lines().find_map(|line| {
                    line.strip_prefix("OPENROUTER_API_KEY=")
                        .map(|v| v.trim().trim_matches('"').to_string())
                })
            })
            .filter(|k| !k.is_empty())
    } else {
        None
    };

    let api_key = if existing_key.is_some() {
        println!("  API key already configured in {}", env_path.display());
        None
    } else {
        let key = prompt_or_cancel!(Password::new("OpenRouter API key:")
            .with_help_message("Get yours at https://openrouter.ai/keys — leave empty to skip")
            .without_confirmation()
            .prompt());
        if key.trim().is_empty() {
            None
        } else {
            Some(key.trim().to_string())
        }
    };

    // ── Summary ──
    println!();
    print_checkmark(&mut stdout, "Agent name:", &agent_name)?;
    print_checkmark(&mut stdout, "Style:     ", STYLES[style_index].name)?;
    print_checkmark(&mut stdout, "Model:     ", &model_id)?;
    if existing_key.is_some() {
        print_checkmark(&mut stdout, "API key:   ", "(already set)")?;
    } else if let Some(ref key) = api_key {
        let masked = if key.len() > 8 {
            format!("{}...{}", &key[..5], &key[key.len() - 4..])
        } else {
            "****".to_string()
        };
        print_checkmark(&mut stdout, "API key:   ", &masked)?;
    } else {
        print_checkmark(&mut stdout, "API key:   ", "(skipped)")?;
    }
    println!();

    Ok(Some(OnboardingResult {
        agent_name,
        style_index,
        model_id,
        api_key,
    }))
}

/// Generate SOUL.md content from onboarding choices.
pub fn generate_soul(name: &str, style_index: usize) -> Result<String> {
    let style = STYLES
        .get(style_index)
        .ok_or_else(|| anyhow::anyhow!("invalid style index {style_index}"))?;

    Ok(format!(
        r#"# {name} — Your AI Personal Assistant

You are {name}, a helpful AI personal assistant. You live on your owner's computer and help them with tasks, remember things for them, and occasionally check in to see how they're doing.

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
pub fn generate_config(model_id: &str) -> Result<String> {
    validate_model_id(model_id)?;

    Ok(format!(
        r#"[llm]
api_key_env = "OPENROUTER_API_KEY"
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

    // Write config.toml (skip if already exists to avoid clobbering manual edits)
    let config_path = data_dir.join("config.toml");
    if config_path.exists() {
        println!("  Skipped {} (already exists)", config_path.display());
    } else {
        let config_content = generate_config(&result.model_id)?;
        std::fs::write(&config_path, &config_content)?;
        println!("  Created {}", config_path.display());
    }

    // Write SOUL.md (skip if already exists)
    let soul_path = data_dir.join("SOUL.md");
    if soul_path.exists() {
        println!("  Skipped {} (already exists)", soul_path.display());
    } else {
        let soul_content = generate_soul(&result.agent_name, result.style_index)?;
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

    // Write .env with API key (skip if not provided)
    if let Some(ref api_key) = result.api_key {
        let env_path = data_dir.join(".env");
        std::fs::write(&env_path, format!("OPENROUTER_API_KEY={api_key}\n"))?;
        println!("  Created {}", env_path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_soul_contains_name_and_style() {
        let soul = generate_soul("Buddy", 0).expect("valid style index");
        assert!(soul.contains("# Buddy — Your AI Personal Assistant"));
        assert!(soul.contains("You are Buddy"));
        assert!(soul.contains("## Communication Style"));
        assert!(soul.contains("Professional and direct"));
    }

    #[test]
    fn generate_soul_all_styles_valid() {
        for i in 0..STYLES.len() {
            let soul = generate_soul("Test", i).expect("valid style index");
            assert!(soul.contains(STYLES[i].name) || soul.contains("Communication Style"));
        }
    }

    #[test]
    fn generate_soul_invalid_index_errors() {
        assert!(generate_soul("Test", 999).is_err());
    }

    #[test]
    fn generate_config_default_model() {
        let config = generate_config("anthropic/claude-sonnet-4").expect("valid model");
        assert!(config.contains("model = \"anthropic/claude-sonnet-4\""));
        assert!(config.contains("[llm]"));
        assert!(config.contains("[sandbox]"));
    }

    #[test]
    fn generate_config_rejects_empty_model() {
        assert!(generate_config("").is_err());
    }

    #[test]
    fn generate_config_rejects_injection() {
        assert!(generate_config("model\"\nmalicious = true").is_err());
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
}
