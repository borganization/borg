//! Central default values for the entire application.
//!
//! All magic numbers live here. Config `Default` impls and runtime code
//! reference these constants instead of scattering literals.

use std::time::Duration;

// ── Tier A: User-facing (also config keys) ──────────────────────────

/// Max tokens for tool output before truncation (head + tail preserved).
pub const TOOL_OUTPUT_MAX_TOKENS: usize = 4000;

/// Tokens reserved for the compaction summary marker.
pub const COMPACTION_MARKER_TOKENS: usize = 200;

/// Max characters from the transcript sent to the LLM summarizer.
pub const MAX_TRANSCRIPT_CHARS: usize = 4000;

/// Max body size for gateway webhook requests (2 MB).
pub const GATEWAY_MAX_BODY_SIZE: usize = 2 * 1024 * 1024;

/// Telegram long-polling timeout in seconds.
pub const TELEGRAM_POLL_TIMEOUT_SECS: u64 = 30;

/// Consecutive failures before circuit breaker opens.
pub const TELEGRAM_CIRCUIT_FAILURE_THRESHOLD: u32 = 10;

/// Seconds the circuit breaker stays open after tripping.
pub const TELEGRAM_CIRCUIT_SUSPENSION_SECS: u64 = 300;

/// Capacity of the Telegram update deduplicator.
pub const TELEGRAM_DEDUP_CAPACITY: usize = 1000;

/// Capacity of the Slack event deduplicator.
pub const SLACK_DEDUP_CAPACITY: usize = 1000;

/// Max characters shown as tool result preview in REPL.
pub const TOOL_RESULT_PREVIEW_CHARS: usize = 200;

// ── Tier B: Operational internals ───────────────────────────────────

/// Injection score at or above which content is considered high-risk.
pub const INJECTION_HIGH_RISK_THRESHOLD: u8 = 50;

/// Injection score at or above which content is flagged.
pub const INJECTION_FLAGGED_THRESHOLD: u8 = 20;

/// Rough bytes-per-token estimate for truncation math.
pub const APPROX_BYTES_PER_TOKEN: usize = 4;

// Telegram polling backoff parameters
pub const TELEGRAM_MIN_BACKOFF: Duration = Duration::from_secs(2);
pub const TELEGRAM_MAX_BACKOFF: Duration = Duration::from_secs(30);
pub const TELEGRAM_BACKOFF_FACTOR: f64 = 1.8;
pub const TELEGRAM_JITTER_FRACTION: f64 = 0.25;
pub const TELEGRAM_STALL_TIMEOUT: Duration = Duration::from_secs(90);

// Gateway retry defaults
pub const RETRY_MAX_RETRIES: u32 = 5;
pub const RETRY_INITIAL_DELAY_MS: u64 = 5000;
pub const RETRY_MAX_DELAY_MS: u64 = 300_000;
pub const RETRY_BACKOFF_FACTOR: f64 = 2.0;
pub const RETRY_JITTER_FACTOR: f64 = 0.1;

// iMessage echo cache parameters
pub const ECHO_CACHE_TEXT_TTL: Duration = Duration::from_secs(5);
pub const ECHO_CACHE_ID_TTL: Duration = Duration::from_secs(60);

// iMessage self-chat cache parameters
pub const SELF_CHAT_CACHE_MAX_ENTRIES: usize = 512;
pub const SELF_CHAT_CACHE_TTL: Duration = Duration::from_secs(10);

// Rate guard defaults (interactive sessions — generous for long-running tasks)
pub const RATE_TOOL_CALLS_WARN: u32 = 200;
pub const RATE_TOOL_CALLS_BLOCK: u32 = 500;
pub const RATE_SHELL_COMMANDS_WARN: u32 = 100;
pub const RATE_SHELL_COMMANDS_BLOCK: u32 = 250;
pub const RATE_FILE_WRITES_WARN: u32 = 50;
pub const RATE_FILE_WRITES_BLOCK: u32 = 150;
pub const RATE_MEMORY_WRITES_WARN: u32 = 20;
pub const RATE_MEMORY_WRITES_BLOCK: u32 = 50;
pub const RATE_WEB_REQUESTS_WARN: u32 = 50;
pub const RATE_WEB_REQUESTS_BLOCK: u32 = 150;

// Rate guard defaults (gateway sessions — stricter for external senders)
pub const GW_RATE_TOOL_CALLS_WARN: u32 = 30;
pub const GW_RATE_TOOL_CALLS_BLOCK: u32 = 50;
pub const GW_RATE_SHELL_COMMANDS_WARN: u32 = 10;
pub const GW_RATE_SHELL_COMMANDS_BLOCK: u32 = 20;
pub const GW_RATE_FILE_WRITES_WARN: u32 = 10;
pub const GW_RATE_FILE_WRITES_BLOCK: u32 = 20;
pub const GW_RATE_MEMORY_WRITES_WARN: u32 = 5;
pub const GW_RATE_MEMORY_WRITES_BLOCK: u32 = 10;
pub const GW_RATE_WEB_REQUESTS_WARN: u32 = 10;
pub const GW_RATE_WEB_REQUESTS_BLOCK: u32 = 25;
