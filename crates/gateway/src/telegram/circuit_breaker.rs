use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use borg_core::constants;

const FAILURE_THRESHOLD: u32 = constants::TELEGRAM_CIRCUIT_FAILURE_THRESHOLD;
const SUSPENSION_SECS: u64 = constants::TELEGRAM_CIRCUIT_SUSPENSION_SECS;

/// Circuit breaker for Telegram API calls (primarily sendChatAction).
///
/// Prevents infinite 401 loops that can cause bot deletion (ref: OpenClaw #27092).
/// Uses atomics for lock-free operation on the hot path.
pub struct CircuitBreaker {
    consecutive_failures: AtomicU32,
    open: AtomicBool,
    suspended_until: AtomicU64,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
            open: AtomicBool::new(false),
            suspended_until: AtomicU64::new(0),
        }
    }

    /// Returns `true` if the circuit is open (calls should be skipped).
    pub fn is_open(&self) -> bool {
        if !self.open.load(Ordering::Acquire) {
            return false;
        }

        let now = now_secs();
        let suspended_until = self.suspended_until.load(Ordering::Acquire);
        if now >= suspended_until {
            // Suspension period elapsed — half-open: allow a probe
            self.consecutive_failures.store(0, Ordering::Release);
            self.open.store(false, Ordering::Release);
            false
        } else {
            true
        }
    }

    /// Record a successful call — resets the failure counter.
    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Release);
        self.open.store(false, Ordering::Release);
    }

    /// Record a failure with the given HTTP status code.
    /// Only 401 status codes contribute to the circuit breaker threshold.
    pub fn record_failure(&self, status: u16) {
        if status != 401 {
            return;
        }

        let count = self.consecutive_failures.fetch_add(1, Ordering::AcqRel) + 1;
        if count >= FAILURE_THRESHOLD {
            self.suspended_until
                .store(now_secs() + SUSPENSION_SECS, Ordering::Release);
            self.open.store(true, Ordering::Release);
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_circuit_is_closed() {
        let cb = CircuitBreaker::new();
        assert!(!cb.is_open());
    }

    #[test]
    fn non_401_failures_ignored() {
        let cb = CircuitBreaker::new();
        for _ in 0..20 {
            cb.record_failure(500);
        }
        assert!(!cb.is_open());
    }

    #[test]
    fn opens_after_threshold_401s() {
        let cb = CircuitBreaker::new();
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure(401);
        }
        assert!(cb.is_open());
    }

    #[test]
    fn success_resets_counter() {
        let cb = CircuitBreaker::new();
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            cb.record_failure(401);
        }
        assert!(!cb.is_open());
        cb.record_success();
        // One more 401 shouldn't open it since counter was reset
        cb.record_failure(401);
        assert!(!cb.is_open());
    }

    #[test]
    fn reopens_after_suspension_period() {
        let cb = CircuitBreaker::new();
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure(401);
        }
        assert!(cb.is_open());

        // Simulate suspension period elapsed
        cb.suspended_until.store(0, Ordering::Relaxed);
        assert!(!cb.is_open());
    }
}
