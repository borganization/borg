use std::sync::LazyLock;
use tiktoken_rs::{cl100k_base, CoreBPE};

static BPE: LazyLock<Option<CoreBPE>> = LazyLock::new(|| match cl100k_base() {
    Ok(bpe) => Some(bpe),
    Err(e) => {
        tracing::error!("Failed to load cl100k_base tokenizer: {e} — using byte heuristic");
        None
    }
});

/// Estimate token count using the cl100k_base BPE tokenizer.
///
/// cl100k_base is the encoding used by GPT-4, ChatGPT, and text-embedding-ada-002.
/// It provides a reasonable approximation for most LLMs available through OpenRouter,
/// including Claude models.
///
/// Falls back to a byte-count heuristic (~4 bytes per token) if the tokenizer
/// fails to initialize.
pub fn estimate_tokens(text: &str) -> usize {
    match BPE.as_ref() {
        Some(bpe) => bpe.encode_with_special_tokens(text).len(),
        None => text.len().div_ceil(4),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_token_count() {
        let count = estimate_tokens("Hello, world!");
        assert!(count > 0);
        assert!(count < 10);
    }

    #[test]
    fn empty_string() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn longer_text_reasonable_estimate() {
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(10);
        let count = estimate_tokens(&text);
        // Should be significantly more accurate than len/4
        assert!(count > 50);
        assert!(count < 200);
    }
}
