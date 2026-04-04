use std::fmt;
use std::time::Duration;

use crate::provider::Provider;

// ── Error classification ──

/// Why a provider failed — used for cooldown duration calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailoverReason {
    Auth,
    Billing,
    RateLimit,
    Overloaded,
    Timeout,
    Format,
    Unknown,
}

impl fmt::Display for FailoverReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth => write!(f, "auth"),
            Self::Billing => write!(f, "billing"),
            Self::RateLimit => write!(f, "rate_limit"),
            Self::Overloaded => write!(f, "overloaded"),
            Self::Timeout => write!(f, "timeout"),
            Self::Format => write!(f, "format"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug)]
pub enum LlmError {
    Retryable {
        source: anyhow::Error,
        retry_after: Option<Duration>,
        reason: FailoverReason,
    },
    Fatal {
        source: anyhow::Error,
        reason: FailoverReason,
    },
    Interrupted,
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable { source, .. } => write!(f, "{source}"),
            Self::Fatal { source, .. } => write!(f, "{source}"),
            Self::Interrupted => write!(f, "request interrupted"),
        }
    }
}

impl std::error::Error for LlmError {}

impl LlmError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    pub fn reason(&self) -> FailoverReason {
        match self {
            Self::Retryable { reason, .. } | Self::Fatal { reason, .. } => *reason,
            Self::Interrupted => FailoverReason::Unknown,
        }
    }
}

pub(crate) fn classify_status(
    status: reqwest::StatusCode,
    body: &str,
    provider: Provider,
) -> LlmError {
    let retry_after = parse_retry_after(body);

    match status.as_u16() {
        429 => LlmError::Retryable {
            source: anyhow::anyhow!("{provider} returned 429 (rate limited): {body}"),
            retry_after,
            reason: FailoverReason::RateLimit,
        },
        500 | 502 | 504 => LlmError::Retryable {
            source: anyhow::anyhow!("{provider} returned {status}: {body}"),
            retry_after: None,
            reason: FailoverReason::Overloaded,
        },
        503 => LlmError::Retryable {
            source: anyhow::anyhow!("{provider} returned {status} (overloaded): {body}"),
            retry_after: None,
            reason: FailoverReason::Overloaded,
        },
        401 | 403 => LlmError::Fatal {
            source: anyhow::anyhow!("{provider} returned {status} (auth error): {body}"),
            reason: FailoverReason::Auth,
        },
        402 => LlmError::Fatal {
            source: anyhow::anyhow!("{provider} returned {status} (billing error): {body}"),
            reason: FailoverReason::Billing,
        },
        400 => LlmError::Fatal {
            source: anyhow::anyhow!("{provider} returned {status} (bad request): {body}"),
            reason: FailoverReason::Format,
        },
        _ => LlmError::Fatal {
            source: anyhow::anyhow!("{provider} returned {status}: {body}"),
            reason: FailoverReason::Unknown,
        },
    }
}

pub(crate) fn classify_network_error(err: anyhow::Error) -> LlmError {
    LlmError::Retryable {
        source: err,
        retry_after: None,
        reason: FailoverReason::Timeout,
    }
}

pub fn parse_retry_after(body: &str) -> Option<Duration> {
    // Try to extract retry_after from JSON error body
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(secs) = v["error"]["retry_after"].as_f64() {
            return Some(Duration::from_secs_f64(secs));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FailoverReason Display ──

    #[test]
    fn failover_reason_display() {
        assert_eq!(FailoverReason::Auth.to_string(), "auth");
        assert_eq!(FailoverReason::Billing.to_string(), "billing");
        assert_eq!(FailoverReason::RateLimit.to_string(), "rate_limit");
        assert_eq!(FailoverReason::Overloaded.to_string(), "overloaded");
        assert_eq!(FailoverReason::Timeout.to_string(), "timeout");
        assert_eq!(FailoverReason::Format.to_string(), "format");
        assert_eq!(FailoverReason::Unknown.to_string(), "unknown");
    }

    // ── LlmError methods ──

    #[test]
    fn retryable_error_is_retryable() {
        let err = LlmError::Retryable {
            source: anyhow::anyhow!("rate limited"),
            retry_after: Some(Duration::from_secs(5)),
            reason: FailoverReason::RateLimit,
        };
        assert!(err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::RateLimit);
    }

    #[test]
    fn fatal_error_is_not_retryable() {
        let err = LlmError::Fatal {
            source: anyhow::anyhow!("auth failed"),
            reason: FailoverReason::Auth,
        };
        assert!(!err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Auth);
    }

    #[test]
    fn interrupted_error_is_not_retryable() {
        let err = LlmError::Interrupted;
        assert!(!err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Unknown);
    }

    #[test]
    fn llm_error_display() {
        let retryable = LlmError::Retryable {
            source: anyhow::anyhow!("overloaded"),
            retry_after: None,
            reason: FailoverReason::Overloaded,
        };
        assert_eq!(retryable.to_string(), "overloaded");

        let fatal = LlmError::Fatal {
            source: anyhow::anyhow!("bad request"),
            reason: FailoverReason::Format,
        };
        assert_eq!(fatal.to_string(), "bad request");

        assert_eq!(LlmError::Interrupted.to_string(), "request interrupted");
    }

    // ── classify_status ──

    #[test]
    fn classify_429_is_retryable_rate_limit() {
        let err = classify_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "{}",
            Provider::OpenRouter,
        );
        assert!(err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::RateLimit);
    }

    #[test]
    fn classify_500_is_retryable_overloaded() {
        let err = classify_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "error",
            Provider::OpenAi,
        );
        assert!(err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Overloaded);
    }

    #[test]
    fn classify_502_is_retryable() {
        let err = classify_status(reqwest::StatusCode::BAD_GATEWAY, "", Provider::Anthropic);
        assert!(err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Overloaded);
    }

    #[test]
    fn classify_503_is_retryable_overloaded() {
        let err = classify_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            "",
            Provider::Gemini,
        );
        assert!(err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Overloaded);
    }

    #[test]
    fn classify_504_is_retryable() {
        let err = classify_status(
            reqwest::StatusCode::GATEWAY_TIMEOUT,
            "",
            Provider::OpenRouter,
        );
        assert!(err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Overloaded);
    }

    #[test]
    fn classify_401_is_fatal_auth() {
        let err = classify_status(
            reqwest::StatusCode::UNAUTHORIZED,
            "invalid key",
            Provider::OpenAi,
        );
        assert!(!err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Auth);
    }

    #[test]
    fn classify_403_is_fatal_auth() {
        let err = classify_status(
            reqwest::StatusCode::FORBIDDEN,
            "forbidden",
            Provider::Anthropic,
        );
        assert!(!err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Auth);
    }

    #[test]
    fn classify_402_is_fatal_billing() {
        let err = classify_status(
            reqwest::StatusCode::PAYMENT_REQUIRED,
            "insufficient credits",
            Provider::OpenRouter,
        );
        assert!(!err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Billing);
    }

    #[test]
    fn classify_400_is_fatal_format() {
        let err = classify_status(
            reqwest::StatusCode::BAD_REQUEST,
            "malformed",
            Provider::OpenAi,
        );
        assert!(!err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Format);
    }

    #[test]
    fn classify_unknown_status_is_fatal() {
        let err = classify_status(reqwest::StatusCode::IM_A_TEAPOT, "", Provider::OpenRouter);
        assert!(!err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Unknown);
    }

    // ── classify_network_error ──

    #[test]
    fn network_error_is_retryable_timeout() {
        let err = classify_network_error(anyhow::anyhow!("connection reset"));
        assert!(err.is_retryable());
        assert_eq!(err.reason(), FailoverReason::Timeout);
    }

    // ── parse_retry_after ──

    #[test]
    fn parse_retry_after_from_json() {
        let body = r#"{"error":{"retry_after":2.5}}"#;
        let dur = parse_retry_after(body);
        assert_eq!(dur, Some(Duration::from_secs_f64(2.5)));
    }

    #[test]
    fn parse_retry_after_missing_field() {
        let body = r#"{"error":{"message":"too many requests"}}"#;
        assert_eq!(parse_retry_after(body), None);
    }

    #[test]
    fn parse_retry_after_invalid_json() {
        assert_eq!(parse_retry_after("not json"), None);
    }

    #[test]
    fn parse_retry_after_empty_body() {
        assert_eq!(parse_retry_after(""), None);
    }

    // ── classify_status with retry_after in body ──

    #[test]
    fn classify_429_extracts_retry_after() {
        let body = r#"{"error":{"retry_after":10.0}}"#;
        let err = classify_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            body,
            Provider::OpenRouter,
        );
        assert!(err.is_retryable());
        if let LlmError::Retryable { retry_after, .. } = err {
            assert_eq!(retry_after, Some(Duration::from_secs(10)));
        } else {
            panic!("expected Retryable");
        }
    }
}
