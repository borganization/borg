use regex::Regex;
use std::sync::LazyLock;

struct SecretPattern {
    regex: Regex,
    label: &'static str,
}

static SECRET_PATTERNS: LazyLock<Vec<SecretPattern>> = LazyLock::new(|| {
    let patterns: Vec<(&str, &str)> = vec![
        (r"AKIA[0-9A-Z]{16}", "AWS Access Key"),
        (r"(?:ghp|ghs|gho|ghu|ghm)_[A-Za-z0-9_]{36,}", "GitHub Token"),
        (r"sk[-_]ant[-_][A-Za-z0-9_-]{32,}", "Anthropic API Key"),
        (r"sk[-_]proj[-_][A-Za-z0-9_-]{32,}", "OpenAI Project Key"),
        (r"sk-[A-Za-z0-9]{48,}", "API Key"),
        (
            r"eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            "JWT",
        ),
        (
            r"-----BEGIN (?:RSA |EC |DSA )?PRIVATE KEY-----",
            "Private Key",
        ),
        (r"xox[bpsar]-[A-Za-z0-9-]{24,}", "Slack Token"),
        (
            r#"(?:password|passwd|pwd)\s*[=:]\s*"[^"]{8,}""#,
            "Quoted Password",
        ),
        (
            r"(?:password|passwd|pwd)\s*[=:]\s*(\S{8,64})",
            "Password Assignment",
        ),
        (
            r"(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp|mssql)://[^\s]{8,}",
            "Database Connection String",
        ),
    ];

    patterns
        .into_iter()
        .filter_map(|(pattern, label)| match Regex::new(pattern) {
            Ok(regex) => Some(SecretPattern { regex, label }),
            Err(e) => {
                tracing::error!("Failed to compile secret pattern '{label}': {e} — skipping");
                None
            }
        })
        .collect()
});

/// Redact detected secrets in text, replacing matches with `[REDACTED <label>]`.
pub fn redact_secrets(text: &str) -> String {
    let mut result = text.to_string();
    for pattern in SECRET_PATTERNS.iter() {
        result = pattern
            .regex
            .replace_all(&result, format!("[REDACTED {}]", pattern.label))
            .into_owned();
    }
    result
}

/// Check whether text contains any detectable secrets.
pub fn contains_secrets(text: &str) -> bool {
    SECRET_PATTERNS.iter().any(|p| p.regex.is_match(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_aws_key() {
        let input = "key=AKIAIOSFODNN7EXAMPLE";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED AWS Access Key]"));
        assert!(!result.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn redacts_github_token() {
        let input = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijk";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED GitHub Token]"));
    }

    #[test]
    fn redacts_anthropic_key() {
        let input = "ANTHROPIC_API_KEY=sk-ant-api03-aBcDeFgHiJkLmNoPqRsTuVwXyZ012345678";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED Anthropic API Key]"));
    }

    #[test]
    fn redacts_jwt() {
        let input = "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED JWT]"));
    }

    #[test]
    fn redacts_private_key() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAK...";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED Private Key]"));
    }

    #[test]
    fn no_false_positive_on_normal_text() {
        let input = "Hello world, this is a normal message with no secrets.";
        assert!(!contains_secrets(input));
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn contains_secrets_detects() {
        assert!(contains_secrets("my key AKIAIOSFODNN7EXAMPLE is here"));
        assert!(!contains_secrets("just a normal string"));
    }

    #[test]
    fn redacts_slack_token() {
        let input = "SLACK_TOKEN=xoxb-1234567890-abcdefghijklmnopqrstuvwx";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED Slack Token]"));
    }

    #[test]
    fn redacts_database_connection_string() {
        let input = "DATABASE_URL=postgresql://user:pass@localhost:5432/mydb";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED Database Connection String]"));
        assert!(!result.contains("pass@localhost"));
    }

    #[test]
    fn redacts_mongodb_connection_string() {
        let input = "MONGO=mongodb+srv://admin:secret@cluster0.example.net/db";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED Database Connection String]"));
    }

    #[test]
    fn redacts_underscore_variant_api_keys() {
        let input = "key=sk_ant_api03_aBcDeFgHiJkLmNoPqRsTuVwXyZ012345678";
        let result = redact_secrets(input);
        assert!(result.contains("[REDACTED Anthropic API Key]"));
    }
}
