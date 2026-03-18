use borg_core::config::Config;
use borg_core::db::Database;
use tracing::warn;

enum Command {
    Status,
    New,
    Reset,
    Usage,
    Help,
}

impl Command {
    fn parse(text: &str) -> Option<Self> {
        let first_word = text.split_whitespace().next()?.to_ascii_lowercase();
        match first_word.as_str() {
            "/status" => Some(Self::Status),
            "/new" => Some(Self::New),
            "/reset" => Some(Self::Reset),
            "/usage" => Some(Self::Usage),
            "/help" => Some(Self::Help),
            _ => None,
        }
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
) -> Option<String> {
    let cmd = Command::parse(text)?;
    let response = match cmd {
        Command::Status => handle_status(db, session_id),
        Command::New => handle_new(db, channel_name, session_key),
        Command::Reset => handle_reset(db, session_id),
        Command::Usage => handle_usage(db, config, session_id),
        Command::Help => handle_help(),
    };
    Some(response)
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

fn handle_help() -> String {
    [
        "Available commands:",
        "  /status  — Show current session info",
        "  /new     — Start a new session",
        "  /reset   — Clear messages in current session",
        "  /usage   — Show token usage and budget",
        "  /help    — Show this help message",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_commands() {
        assert!(matches!(Command::parse("/status"), Some(Command::Status)));
        assert!(matches!(Command::parse("/new"), Some(Command::New)));
        assert!(matches!(Command::parse("/reset"), Some(Command::Reset)));
        assert!(matches!(Command::parse("/usage"), Some(Command::Usage)));
        assert!(matches!(Command::parse("/help"), Some(Command::Help)));
    }

    #[test]
    fn parse_unknown_command_returns_none() {
        assert!(Command::parse("/unknown").is_none());
        assert!(Command::parse("hello").is_none());
        assert!(Command::parse("").is_none());
    }

    #[test]
    fn parse_command_with_trailing_text() {
        assert!(matches!(
            Command::parse("/status extra args"),
            Some(Command::Status)
        ));
    }

    #[test]
    fn parse_case_insensitive() {
        assert!(matches!(Command::parse("/Status"), Some(Command::Status)));
        assert!(matches!(Command::parse("/HELP"), Some(Command::Help)));
        assert!(matches!(Command::parse("/New"), Some(Command::New)));
    }

    #[test]
    fn help_contains_all_commands() {
        let help = handle_help();
        assert!(help.contains("/status"));
        assert!(help.contains("/new"));
        assert!(help.contains("/reset"));
        assert!(help.contains("/usage"));
        assert!(help.contains("/help"));
    }
}
