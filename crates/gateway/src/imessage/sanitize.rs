use regex::Regex;
use std::sync::LazyLock;

static THINKING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<thinking>.*?</thinking>").unwrap_or_else(|_| unreachable!())
});

static INTERNAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<internal>.*?</internal>").unwrap_or_else(|_| unreachable!())
});

static MEMORY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<relevant_memories>.*?</relevant_memories>").unwrap_or_else(|_| unreachable!())
});

static SEPARATOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#{5,}[^\n]*").unwrap_or_else(|_| unreachable!()));

static ROLE_MARKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)assistant\s+to\s*=\s*[^\n]*").unwrap_or_else(|_| unreachable!())
});

static EXCESS_BLANK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n{4,}").unwrap_or_else(|_| unreachable!()));

/// Strip internal tags, thinking blocks, and other LLM artifacts from
/// outbound text before sending via iMessage.
pub fn sanitize_outbound(text: &str) -> String {
    let mut result = text.to_string();
    result = THINKING_RE.replace_all(&result, "").to_string();
    result = INTERNAL_RE.replace_all(&result, "").to_string();
    result = MEMORY_RE.replace_all(&result, "").to_string();
    result = SEPARATOR_RE.replace_all(&result, "").to_string();
    result = ROLE_MARKER_RE.replace_all(&result, "").to_string();
    result = EXCESS_BLANK_RE.replace_all(&result, "\n\n\n").to_string();
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_thinking_blocks() {
        let input = "Hello <thinking>internal thought</thinking> world";
        assert_eq!(sanitize_outbound(input), "Hello  world");
    }

    #[test]
    fn strips_internal_blocks() {
        let input = "before <internal>secret</internal> after";
        assert_eq!(sanitize_outbound(input), "before  after");
    }

    #[test]
    fn strips_memory_blocks() {
        let input = "text <relevant_memories>memories here</relevant_memories> more";
        assert_eq!(sanitize_outbound(input), "text  more");
    }

    #[test]
    fn strips_separators() {
        let input = "line 1\n###### Section\nline 2";
        assert_eq!(sanitize_outbound(input), "line 1\n\nline 2");
    }

    #[test]
    fn collapses_blank_lines() {
        let input = "a\n\n\n\n\n\nb";
        assert_eq!(sanitize_outbound(input), "a\n\n\nb");
    }

    #[test]
    fn clean_text_unchanged() {
        assert_eq!(sanitize_outbound("Hello world!"), "Hello world!");
    }
}
