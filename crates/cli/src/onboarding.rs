use anyhow::Result;
use crossterm::{
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    ExecutableCommand,
};
use inquire::{Select, Text};
use std::io::{self, Write};

use tamagotchi_core::config::Config;

const BANNER: &str = r#"
  ╔══════════════════════════════════════╗
  ║      🐣  Welcome to Tamagotchi  🐣   ║
  ║     Your AI Personal Assistant       ║
  ╚══════════════════════════════════════╝
"#;

/// Model choices available via OpenRouter.
const MODELS: &[(&str, &str)] = &[
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
}

/// Run the interactive onboarding wizard. Returns `None` if the user cancels.
pub fn run_onboarding() -> Result<Option<OnboardingResult>> {
    let mut stdout = io::stdout();

    // ── Banner ──
    stdout.execute(SetForegroundColor(Color::Magenta))?;
    stdout.execute(SetAttribute(Attribute::Bold))?;
    stdout.execute(Print(BANNER))?;
    stdout.execute(ResetColor)?;
    stdout.execute(SetAttribute(Attribute::Reset))?;
    stdout.flush()?;

    println!("  Let's set up your personal AI assistant.\n");

    // ── Step 1: Agent name ──
    let agent_name = match Text::new("What should your agent be called?")
        .with_default("Tamagotchi")
        .with_help_message("This name appears in the SOUL.md personality file")
        .prompt()
    {
        Ok(name) => name,
        Err(
            inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted,
        ) => {
            println!("\nSetup cancelled.");
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };

    // ── Step 2: Personality style ──
    let style_options: Vec<String> = STYLES
        .iter()
        .map(|s| format!("{} — {}", s.name, s.description))
        .collect();

    let style_index = match Select::new("Pick a personality style:", style_options)
        .with_help_message("Defines the tone of your agent's responses")
        .prompt()
    {
        Ok(chosen) => STYLES
            .iter()
            .position(|s| chosen.starts_with(s.name))
            .unwrap_or(0),
        Err(
            inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted,
        ) => {
            println!("\nSetup cancelled.");
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };

    // ── Step 3: Model selection ──
    let model_options: Vec<String> = MODELS
        .iter()
        .map(|(_, label)| (*label).to_string())
        .collect();

    let model_id = match Select::new("Choose your default model:", model_options)
        .with_help_message(
            "All models served via OpenRouter — you can change this later in config.toml",
        )
        .prompt()
    {
        Ok(chosen) => MODELS
            .iter()
            .find(|(_, label)| *label == chosen.as_str())
            .map(|(id, _)| (*id).to_string())
            .unwrap_or_else(|| "anthropic/claude-sonnet-4".to_string()),
        Err(
            inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted,
        ) => {
            println!("\nSetup cancelled.");
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };

    // ── Summary ──
    println!();
    stdout.execute(SetForegroundColor(Color::Green))?;
    stdout.execute(SetAttribute(Attribute::Bold))?;
    print!("  ✓ ");
    stdout.execute(ResetColor)?;
    stdout.execute(SetAttribute(Attribute::Reset))?;
    println!("Agent name:  {agent_name}");

    stdout.execute(SetForegroundColor(Color::Green))?;
    stdout.execute(SetAttribute(Attribute::Bold))?;
    print!("  ✓ ");
    stdout.execute(ResetColor)?;
    stdout.execute(SetAttribute(Attribute::Reset))?;
    println!("Style:       {}", STYLES[style_index].name);

    stdout.execute(SetForegroundColor(Color::Green))?;
    stdout.execute(SetAttribute(Attribute::Bold))?;
    print!("  ✓ ");
    stdout.execute(ResetColor)?;
    stdout.execute(SetAttribute(Attribute::Reset))?;
    println!("Model:       {model_id}");
    println!();

    Ok(Some(OnboardingResult {
        agent_name,
        style_index,
        model_id,
    }))
}

/// Generate SOUL.md content from onboarding choices.
pub fn generate_soul(name: &str, style_index: usize) -> String {
    let style = &STYLES[style_index];
    format!(
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
    )
}

/// Generate config.toml content from onboarding choices.
pub fn generate_config(model_id: &str) -> String {
    format!(
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
    )
}

/// Apply onboarding results: create directories, write config and soul files.
pub fn apply_onboarding(result: &OnboardingResult) -> Result<()> {
    let data_dir = Config::data_dir()?;

    // Create directory structure
    for sub in &["memory", "tools", "skills", "logs", "cache"] {
        std::fs::create_dir_all(data_dir.join(sub))?;
    }

    // Write config.toml
    let config_path = data_dir.join("config.toml");
    let config_content = generate_config(&result.model_id);
    std::fs::write(&config_path, &config_content)?;
    println!("  Created {}", config_path.display());

    // Write SOUL.md
    let soul_path = data_dir.join("SOUL.md");
    let soul_content = generate_soul(&result.agent_name, result.style_index);
    std::fs::write(&soul_path, &soul_content)?;
    println!("  Created {}", soul_path.display());

    // Write MEMORY.md
    let memory_path = data_dir.join("MEMORY.md");
    if !memory_path.exists() {
        std::fs::write(&memory_path, "# Memory Index\n\nNo memories yet.\n")?;
        println!("  Created {}", memory_path.display());
    }

    Ok(())
}
