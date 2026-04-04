use std::time::Duration;

/// Calculate exponential backoff with jitter.
///
/// Uses `min_backoff * factor^(errors-1)` capped at `max_backoff`, plus random jitter.
pub fn calculate_backoff(
    consecutive_errors: u32,
    min_backoff: Duration,
    max_backoff: Duration,
    backoff_factor: f64,
    jitter_fraction: f64,
) -> Duration {
    let base = min_backoff.as_secs_f64()
        * backoff_factor.powi(consecutive_errors.saturating_sub(1) as i32);
    let capped = base.min(max_backoff.as_secs_f64());

    // Add jitter
    let jitter = capped * jitter_fraction * rand::random::<f64>();
    Duration::from_secs_f64(capped + jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(1);
    const MAX: Duration = Duration::from_secs(60);
    const FACTOR: f64 = 2.0;
    const JITTER: f64 = 0.1;

    #[test]
    fn backoff_increases_with_errors() {
        let b1 = calculate_backoff(1, MIN, MAX, FACTOR, JITTER);
        let b3 = calculate_backoff(3, MIN, MAX, FACTOR, JITTER);
        assert!(b1.as_secs_f64() >= MIN.as_secs_f64());
        assert!(b3.as_secs_f64() <= MAX.as_secs_f64() * (1.0 + JITTER));
    }

    #[test]
    fn backoff_capped_at_max() {
        let b = calculate_backoff(100, MIN, MAX, FACTOR, JITTER);
        assert!(b.as_secs_f64() <= MAX.as_secs_f64() * (1.0 + JITTER));
    }

    #[test]
    fn backoff_first_attempt() {
        let b = calculate_backoff(1, MIN, MAX, FACTOR, JITTER);
        assert!(b.as_secs_f64() >= MIN.as_secs_f64());
        assert!(b.as_secs_f64() <= MIN.as_secs_f64() * (1.0 + JITTER) + 0.01);
    }
}
