use std::time::Duration;

use rand::Rng;

/// Compute the delay for an exponential backoff with jitter.
///
/// `attempt` is 0-indexed: attempt 0 → `initial`, attempt 1 → `initial * factor`, etc.
/// Jitter adds ±10% randomness to prevent thundering-herd.
pub fn backoff_delay(attempt: u32, initial: Duration, factor: f64) -> Duration {
    let base = initial.as_millis() as f64 * factor.powi(attempt as i32);
    let jitter = rand::rng().random_range(0.9..=1.1);
    Duration::from_millis((base * jitter) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_attempt_zero_is_near_initial() {
        let delay = backoff_delay(0, Duration::from_millis(200), 2.0);
        // Should be within ±10% of 200ms
        assert!(delay.as_millis() >= 180);
        assert!(delay.as_millis() <= 220);
    }

    #[test]
    fn backoff_increases_with_attempts() {
        let d0 = backoff_delay(0, Duration::from_millis(200), 2.0);
        let d1 = backoff_delay(1, Duration::from_millis(200), 2.0);
        let d2 = backoff_delay(2, Duration::from_millis(200), 2.0);
        // Each should roughly double (within jitter tolerance)
        assert!(d1.as_millis() > d0.as_millis());
        assert!(d2.as_millis() > d1.as_millis());
    }

    #[test]
    fn backoff_stays_reasonable() {
        let delay = backoff_delay(5, Duration::from_millis(200), 2.0);
        // 200 * 2^5 = 6400ms, with jitter ≤ ~7040ms
        assert!(delay.as_millis() <= 8000);
    }
}
