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
