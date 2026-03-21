use anyhow::Result;
use async_trait::async_trait;
use borg_core::config::Config;
use borg_core::db::Database;
use tracing::warn;

/// Metadata for a single gateway slash command.
pub struct CommandDef {
    pub name: &'static str,
    pub description: &'static str,
    pub accepts_args: bool,
    /// If true, the command is not intercepted — it passes through to the agent.
    pub pass_through: bool,
}

/// All gateway slash commands. Used for text parsing, /help output, and native
/// platform menu registration (Telegram setMyCommands, Discord global commands).
pub const GATEWAY_COMMANDS: &[CommandDef] = &[
    CommandDef { name: "help",     description: "Show help and available commands",  accepts_args: false, pass_through: false },
    CommandDef { name: "commands", description: "List all slash commands",           accepts_args: false, pass_through: false },
    CommandDef { name: "status",   description: "Show current session info",         accepts_args: false, pass_through: false },
    CommandDef { name: "new",      description: "Start a new session",               accepts_args: false, pass_through: false },
    CommandDef { name: "reset",    description: "Clear current session messages",    accepts_args: false, pass_through: false },
    CommandDef { name: "usage",    description: "Show token usage and budget",       accepts_args: false, pass_through: false },
    CommandDef { name: "skill",    description: "Run a skill by name",               accepts_args: true,  pass_through: true  },
    CommandDef { name: "skills",   description: "List available skills",             accepts_args: false, pass_through: false },
    CommandDef { name: "tools",    description: "List installed tools",              accepts_args: false, pass_through: false },
    CommandDef { name: "whoami",   description: "Show your sender identity",         accepts_args: false, pass_through: false },
    CommandDef { name: "memory",   description: "Show memory files",                accepts_args: false, pass_through: false },
    CommandDef { name: "doctor",   description: "Run system diagnostics",           accepts_args: false, pass_through: false },
    CommandDef { name: "pairing",  description: "Show pairing status",              accepts_args: false, pass_through: false },
];

/// Trait for platforms that support registering slash commands in native menus.
#[async_trait]
pub trait NativeCommandRegistration {
    async fn register_commands(&self, commands: &[CommandDef]) -> Result<()>;
}

enum Command {
    Help,
    Commands,
    Status,
    New,
    Reset,
    Usage,
    Skill,
    Skills,
    Tools,
    WhoAmI,
    Memory,
    Doctor,
    Pairing,
}

impl Command {
    /// Parse a slash command from message text. Strips `@botname` suffix
    /// (Telegram group format: `/command@BotName`). Returns the command and
    /// any trailing arguments.
    fn parse(text: &str) -> Option<(Self, &str)> {
        let first_word = text.split_whitespace().next()?;
        let lowered = first_word.to_ascii_lowercase();
        // Strip @botname suffix
        let cmd_str = lowered.split('@').next().unwrap_or(&lowered);
        let args = text[first_word.len()..].trim_start();
        let cmd = match cmd_str {
            "/help" => Self::Help,
            "/commands" => Self::Commands,
            "/status" => Self::Status,
            "/new" => Self::New,
            "/reset" => Self::Reset,
            "/usage" => Self::Usage,
            "/skill" => Self::Skill,
            "/skills" => Self::Skills,
            "/tools" => Self::Tools,
            "/whoami" => Self::WhoAmI,
            "/memory" => Self::Memory,
            "/doctor" => Self::Doctor,
            "/pairing" => Self::Pairing,
            _ => return None,
        };
        Some((cmd, args))
    }
}

/// Try to handle a slash command. Returns `Some(response)` if the message was
/// a recognised command, `None` to fall through to the agent.
pub fn try_handle_command(
    text: &str,
    db: &Database,
    config: &Config,
    channel_name: &str,
    session_key: &str,
    session_id: &str,
    sender_id: &str,
) -> Option<String> {
    let (cmd, _args) = Command::parse(text)?;
    let response = match cmd {
        Command::Help => handle_help(),
        Command::Commands => handle_commands(),
        Command::Status => handle_status(db, session_id),
        Command::New => handle_new(db, channel_name, session_key),
        Command::Reset => handle_reset(db, session_id),
        Command::Usage => handle_usage(db, config, session_id),
        Command::Skill => return None, // Pass through to agent
        Command::Skills => handle_skills(config),
        Command::Tools => handle_tools(),
        Command::WhoAmI => handle_whoami(sender_id, channel_name, db),
        Command::Memory => handle_memory(),
        Command::Doctor => handle_doctor(config),
        Command::Pairing => handle_pairing(db, channel_name, sender_id),
    };
    Some(response)
}

fn handle_help() -> String {
    let mut lines = vec!["Available commands:".to_string()];
    for cmd in GATEWAY_COMMANDS {
        let prefix = if cmd.accepts_args {
            format!("  /{} <args>", cmd.name)
        } else {
            format!("  /{}", cmd.name)
        };
        lines.push(format!("{prefix}  — {}", cmd.description));
    }
    lines.join("\n")
}

fn handle_commands() -> String {
    let mut lines = vec!["Slash commands:".to_string()];
    for cmd in GATEWAY_COMMANDS {
        if cmd.pass_through {
            lines.push(format!("  /{}  — {} (sent to agent)", cmd.name, cmd.description));
        } else {
            lines.push(format!("  /{}  — {}", cmd.name, cmd.description));
        }
    }
    lines.join("\n")
}

fn handle_status(db: &Database, session_id: &str) -> String {
    let msg_count = db.count_session_messages(session_id).unwrap_or(0);
    let short_id = session_id.get(..8).unwrap_or(session_id);
    format!("Session: {short_id}...\nMessages: {msg_count}")
}

fn handle_new(db: &Database, channel_name: &str, session_key: &str) -> String {
    let new_id = uuid::Uuid::new_v4().to_string();
    match db.update_channel_session_id(channel_name, session_key, &new_id) {
        Ok(true) => {
            let short_id = &new_id[..8];
            format!("New session started ({short_id}...)")
        }
        Ok(false) => "No existing session found to replace.".to_string(),
        Err(e) => {
            warn!("Failed to create new session: {e}");
            "Failed to create new session.".to_string()
        }
    }
}

fn handle_reset(db: &Database, session_id: &str) -> String {
    match db.delete_session_messages(session_id) {
        Ok(count) => format!("Cleared {count} messages from current session."),
        Err(e) => {
            warn!("Failed to reset session: {e}");
            "Failed to reset session.".to_string()
        }
    }
}

fn handle_usage(db: &Database, config: &Config, session_id: &str) -> String {
    let msg_count = db.count_session_messages(session_id).unwrap_or(0);
    let monthly_tokens = db.monthly_token_total().unwrap_or(0);

    let mut lines = vec![
        format!("Session messages: {msg_count}"),
        format!("Monthly tokens: {monthly_tokens}"),
    ];

    if config.budget.monthly_token_limit > 0 {
        let pct = (monthly_tokens as f64 / config.budget.monthly_token_limit as f64) * 100.0;
        lines.push(format!(
            "Budget: {:.1}% of {} used",
            pct, config.budget.monthly_token_limit
        ));
    }

    lines.join("\n")
}

fn handle_skills(config: &Config) -> String {
    let resolved_creds = config.resolve_credentials();
    match borg_core::skills::load_all_skills(&resolved_creds) {
        Ok(skills) if skills.is_empty() => "No skills installed.".to_string(),
        Ok(skills) => {
            let mut lines = vec![format!("Skills ({}):", skills.len())];
            for skill in &skills {
                let status = if skill.available { "✓" } else { "✗" };
                lines.push(format!(
                    "  {status} {}  — {}",
                    skill.manifest.name, skill.manifest.description
                ));
            }
            lines.join("\n")
        }
        Err(e) => {
            warn!("Failed to load skills: {e}");
            "Failed to list skills.".to_string()
        }
    }
}

fn handle_tools() -> String {
    match borg_tools::registry::ToolRegistry::new() {
        Ok(mut registry) => {
            if let Err(e) = registry.scan() {
                warn!("Failed to scan tools: {e}");
                return "Failed to scan tools.".to_string();
            }
            let tools = registry.list_tools();
            if tools.is_empty() {
                "No tools installed.".to_string()
            } else {
                let mut lines = vec![format!("Tools ({}):", tools.len())];
                for tool in &tools {
                    lines.push(format!("  {tool}"));
                }
                lines.join("\n")
            }
        }
        Err(e) => {
            warn!("Failed to initialize tool registry: {e}");
            "Failed to list tools.".to_string()
        }
    }
}

fn handle_whoami(sender_id: &str, channel_name: &str, db: &Database) -> String {
    let approved = db.is_sender_approved(channel_name, sender_id).unwrap_or(false);
    let status = if approved { "Approved" } else { "Not approved" };
    format!("Sender: {sender_id}\nChannel: {channel_name}\nPairing: {status}")
}

fn handle_memory() -> String {
    match borg_core::memory::list_memory_files() {
        Ok(files) if files.is_empty() => "No memory files.".to_string(),
        Ok(files) => {
            let mut lines = vec![format!("Memory files ({}):", files.len())];
            for f in &files {
                let size_kb = f.size_bytes as f64 / 1024.0;
                lines.push(format!("  {}  ({:.1} KB)", f.filename, size_kb));
            }
            lines.join("\n")
        }
        Err(e) => {
            warn!("Failed to list memory files: {e}");
            "Failed to list memory files.".to_string()
        }
    }
}

fn handle_doctor(config: &Config) -> String {
    let report = borg_core::doctor::run_diagnostics(config);
    let (pass, warnings, fail) = report.counts();
    let mut lines = vec![format!(
        "Diagnostics: {pass} passed, {warnings} warning(s), {fail} failed"
    )];
    for check in &report.checks {
        match &check.status {
            borg_core::doctor::CheckStatus::Warn(msg) => {
                lines.push(format!("  ⚠ {} — {msg}", check.name));
            }
            borg_core::doctor::CheckStatus::Fail(msg) => {
                lines.push(format!("  ✗ {} — {msg}", check.name));
            }
            _ => {}
        }
    }
    if warnings == 0 && fail == 0 {
        lines.push("  All checks passed.".to_string());
    }
    lines.join("\n")
}

fn handle_pairing(db: &Database, channel_name: &str, sender_id: &str) -> String {
    let approved = db.is_sender_approved(channel_name, sender_id).unwrap_or(false);
    if approved {
        return format!("You are approved on channel '{channel_name}'.");
    }
    match db.find_pending_for_sender(channel_name, sender_id) {
        Ok(Some(_)) => format!(
            "You have a pending pairing request on channel '{channel_name}'. Ask the owner to approve it."
        ),
        Ok(None) => {
            "No pairing request found. Send a message to start the pairing process.".to_string()
        }
        Err(e) => {
            warn!("Failed to check pairing status: {e}");
            "Failed to check pairing status.".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_commands() {
        assert!(matches!(Command::parse("/status"), Some((Command::Status, _))));
        assert!(matches!(Command::parse("/new"), Some((Command::New, _))));
        assert!(matches!(Command::parse("/reset"), Some((Command::Reset, _))));
        assert!(matches!(Command::parse("/usage"), Some((Command::Usage, _))));
        assert!(matches!(Command::parse("/help"), Some((Command::Help, _))));
        assert!(matches!(Command::parse("/commands"), Some((Command::Commands, _))));
        assert!(matches!(Command::parse("/skills"), Some((Command::Skills, _))));
        assert!(matches!(Command::parse("/tools"), Some((Command::Tools, _))));
        assert!(matches!(Command::parse("/whoami"), Some((Command::WhoAmI, _))));
        assert!(matches!(Command::parse("/memory"), Some((Command::Memory, _))));
        assert!(matches!(Command::parse("/doctor"), Some((Command::Doctor, _))));
        assert!(matches!(Command::parse("/pairing"), Some((Command::Pairing, _))));
        assert!(matches!(Command::parse("/skill"), Some((Command::Skill, _))));
    }

    #[test]
    fn parse_unknown_command_returns_none() {
        assert!(Command::parse("/unknown").is_none());
        assert!(Command::parse("hello").is_none());
        assert!(Command::parse("").is_none());
    }

    #[test]
    fn parse_command_with_trailing_text() {
        let (cmd, args) = Command::parse("/status extra args").unwrap();
        assert!(matches!(cmd, Command::Status));
        assert_eq!(args, "extra args");
    }

    #[test]
    fn parse_skill_with_args() {
        let (cmd, args) = Command::parse("/skill weather").unwrap();
        assert!(matches!(cmd, Command::Skill));
        assert_eq!(args, "weather");
    }

    #[test]
    fn parse_case_insensitive() {
        assert!(matches!(Command::parse("/Status"), Some((Command::Status, _))));
        assert!(matches!(Command::parse("/HELP"), Some((Command::Help, _))));
        assert!(matches!(Command::parse("/New"), Some((Command::New, _))));
        assert!(matches!(Command::parse("/SKILLS"), Some((Command::Skills, _))));
    }

    #[test]
    fn parse_strips_botname_suffix() {
        assert!(matches!(Command::parse("/help@MyBorgBot"), Some((Command::Help, _))));
        assert!(matches!(Command::parse("/status@BotName"), Some((Command::Status, _))));
        assert!(matches!(Command::parse("/skills@Bot extra"), Some((Command::Skills, _))));
    }

    #[test]
    fn help_contains_all_commands() {
        let help = handle_help();
        for cmd in GATEWAY_COMMANDS {
            assert!(
                help.contains(&format!("/{}", cmd.name)),
                "help missing /{}",
                cmd.name
            );
        }
    }

    #[test]
    fn commands_contains_all_commands() {
        let output = handle_commands();
        for cmd in GATEWAY_COMMANDS {
            assert!(
                output.contains(&format!("/{}", cmd.name)),
                "commands missing /{}",
                cmd.name
            );
        }
    }

    #[test]
    fn gateway_commands_no_duplicates() {
        let mut names: Vec<&str> = GATEWAY_COMMANDS.iter().map(|c| c.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), GATEWAY_COMMANDS.len());
    }
}
