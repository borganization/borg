use std::fmt;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};

/// Supported LLM providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenRouter,
    OpenAi,
    Anthropic,
    Gemini,
}

/// Priority order for auto-detection.
const DETECT_ORDER: &[Provider] = &[
    Provider::OpenRouter,
    Provider::OpenAi,
    Provider::Anthropic,
    Provider::Gemini,
];

impl Provider {
    /// API base URL for this provider.
    pub fn base_url(&self) -> &'static str {
        match self {
            Provider::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Provider::OpenAi => "https://api.openai.com/v1/chat/completions",
            Provider::Anthropic => "https://api.anthropic.com/v1/messages",
            Provider::Gemini => {
                "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
            }
        }
    }

    /// Default environment variable name for this provider's API key.
    pub fn default_env_var(&self) -> &'static str {
        match self {
            Provider::OpenRouter => "OPENROUTER_API_KEY",
            Provider::OpenAi => "OPENAI_API_KEY",
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::Gemini => "GEMINI_API_KEY",
        }
    }

    /// Whether this provider uses the OpenAI-compatible chat completions format.
    pub fn is_openai_compatible(&self) -> bool {
        !matches!(self, Provider::Anthropic)
    }

    /// Build provider-specific HTTP headers.
    pub fn build_headers(&self, api_key: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));

        let bearer = format!("Bearer {api_key}");

        match self {
            Provider::OpenRouter => {
                let val = HeaderValue::from_str(&bearer)
                    .context("API key contains invalid characters for HTTP headers")?;
                headers.insert("Authorization", val);
                headers.insert(
                    "HTTP-Referer",
                    HeaderValue::from_static("https://github.com/tamagotchi-ai"),
                );
                headers.insert(
                    "X-Title",
                    HeaderValue::from_static("Tamagotchi AI Assistant"),
                );
            }
            Provider::Anthropic => {
                let val = HeaderValue::from_str(api_key)
                    .context("API key contains invalid characters for HTTP headers")?;
                headers.insert("x-api-key", val);
                headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            }
            Provider::OpenAi | Provider::Gemini => {
                let val = HeaderValue::from_str(&bearer)
                    .context("API key contains invalid characters for HTTP headers")?;
                headers.insert("Authorization", val);
            }
        }

        Ok(headers)
    }

    /// Auto-detect provider from environment variables. Checks in priority order.
    pub fn detect_from_env() -> Result<(Provider, String)> {
        for provider in DETECT_ORDER {
            if let Ok(key) = std::env::var(provider.default_env_var()) {
                if !key.is_empty() {
                    return Ok((*provider, key));
                }
            }
        }
        bail!(
            "No API key found. Set one of: {}",
            DETECT_ORDER
                .iter()
                .map(Provider::default_env_var)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// Strip vendor prefixes for direct providers (e.g., "anthropic/claude-sonnet-4" → "claude-sonnet-4").
    pub fn normalize_model(&self, model: &str) -> String {
        match self {
            Provider::OpenRouter => model.to_string(),
            Provider::OpenAi => model.strip_prefix("openai/").unwrap_or(model).to_string(),
            Provider::Anthropic => model
                .strip_prefix("anthropic/")
                .unwrap_or(model)
                .to_string(),
            Provider::Gemini => model.strip_prefix("google/").unwrap_or(model).to_string(),
        }
    }

    /// String identifier for config serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::OpenRouter => "openrouter",
            Provider::OpenAi => "openai",
            Provider::Anthropic => "anthropic",
            Provider::Gemini => "gemini",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Provider {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "openrouter" => Ok(Provider::OpenRouter),
            "openai" => Ok(Provider::OpenAi),
            "anthropic" => Ok(Provider::Anthropic),
            "gemini" => Ok(Provider::Gemini),
            _ => {
                bail!("Unknown provider: {s}. Valid options: openrouter, openai, anthropic, gemini")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_round_trip() {
        for provider in DETECT_ORDER {
            let s = provider.as_str();
            let parsed = Provider::from_str(s).unwrap();
            assert_eq!(parsed, *provider);
        }
    }

    #[test]
    fn from_str_case_insensitive() {
        assert_eq!(
            Provider::from_str("OpenRouter").unwrap(),
            Provider::OpenRouter
        );
        assert_eq!(
            Provider::from_str("ANTHROPIC").unwrap(),
            Provider::Anthropic
        );
        assert_eq!(Provider::from_str("Gemini").unwrap(), Provider::Gemini);
    }

    #[test]
    fn from_str_unknown_errors() {
        assert!(Provider::from_str("unknown").is_err());
    }

    // Note: detect_from_env tests are omitted because std::env::set_var/remove_var
    // is not thread-safe and causes flaky failures in parallel test execution.
    // The detection logic is simple enough to verify by inspection, and is covered
    // by integration testing (setting env vars and running the binary).

    #[test]
    fn normalize_model_strips_prefix() {
        assert_eq!(
            Provider::OpenAi.normalize_model("openai/gpt-4.1"),
            "gpt-4.1"
        );
        assert_eq!(
            Provider::Anthropic.normalize_model("anthropic/claude-sonnet-4"),
            "claude-sonnet-4"
        );
        assert_eq!(
            Provider::Gemini.normalize_model("google/gemini-2.5-pro"),
            "gemini-2.5-pro"
        );
        // OpenRouter keeps the full path
        assert_eq!(
            Provider::OpenRouter.normalize_model("anthropic/claude-sonnet-4"),
            "anthropic/claude-sonnet-4"
        );
    }

    #[test]
    fn normalize_model_no_prefix_passthrough() {
        assert_eq!(Provider::OpenAi.normalize_model("gpt-4.1"), "gpt-4.1");
        assert_eq!(
            Provider::Anthropic.normalize_model("claude-sonnet-4"),
            "claude-sonnet-4"
        );
    }

    #[test]
    fn is_openai_compatible() {
        assert!(Provider::OpenRouter.is_openai_compatible());
        assert!(Provider::OpenAi.is_openai_compatible());
        assert!(Provider::Gemini.is_openai_compatible());
        assert!(!Provider::Anthropic.is_openai_compatible());
    }

    #[test]
    fn build_headers_openrouter() {
        let headers = Provider::OpenRouter.build_headers("sk-test").unwrap();
        assert!(headers
            .get("Authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("Bearer"));
        assert!(headers.get("HTTP-Referer").is_some());
        assert!(headers.get("X-Title").is_some());
    }

    #[test]
    fn build_headers_anthropic() {
        let headers = Provider::Anthropic.build_headers("sk-test").unwrap();
        assert_eq!(
            headers.get("x-api-key").unwrap().to_str().unwrap(),
            "sk-test"
        );
        assert!(headers.get("anthropic-version").is_some());
        assert!(headers.get("Authorization").is_none());
    }

    #[test]
    fn build_headers_openai() {
        let headers = Provider::OpenAi.build_headers("sk-test").unwrap();
        assert!(headers
            .get("Authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("Bearer"));
        assert!(headers.get("x-api-key").is_none());
    }
}
