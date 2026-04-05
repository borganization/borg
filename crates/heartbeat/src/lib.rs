//! Proactive heartbeat scheduler with quiet hours and deduplication.
//!
//! Emits `Fire` events on a configurable interval or cron schedule.
//! The consumer (daemon or TUI) runs a full agent turn and delivers to channels.
#![warn(missing_docs)]

/// Pure timer: interval/cron scheduling, quiet hours (timezone-aware), poke signal.
pub mod scheduler;
