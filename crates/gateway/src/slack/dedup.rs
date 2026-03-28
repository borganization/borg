use borg_core::constants;

use crate::dedup::BoundedDedup;

const DEFAULT_CAPACITY: usize = constants::SLACK_DEDUP_CAPACITY;

/// Bounded deduplicator for Slack event IDs.
pub struct EventDeduplicator(BoundedDedup<String>);

impl EventDeduplicator {
    pub fn new() -> Self {
        Self(BoundedDedup::new(DEFAULT_CAPACITY))
    }

    #[cfg(test)]
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self(BoundedDedup::new(capacity))
    }

    /// Returns `true` if this event_id has been seen before.
    pub fn is_duplicate(&mut self, event_id: &str) -> bool {
        self.0.is_duplicate(&event_id.to_string())
    }
}

impl Default for EventDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_seen_not_duplicate() {
        let mut dedup = EventDeduplicator::new();
        assert!(!dedup.is_duplicate("Ev001"));
    }

    #[test]
    fn second_seen_is_duplicate() {
        let mut dedup = EventDeduplicator::new();
        assert!(!dedup.is_duplicate("Ev001"));
        assert!(dedup.is_duplicate("Ev001"));
    }

    #[test]
    fn different_ids_not_duplicate() {
        let mut dedup = EventDeduplicator::new();
        assert!(!dedup.is_duplicate("Ev001"));
        assert!(!dedup.is_duplicate("Ev002"));
        assert!(!dedup.is_duplicate("Ev003"));
    }

    #[test]
    fn eviction_at_capacity() {
        let mut dedup = EventDeduplicator::with_capacity(3);

        assert!(!dedup.is_duplicate("Ev1"));
        assert!(!dedup.is_duplicate("Ev2"));
        assert!(!dedup.is_duplicate("Ev3"));
        // Capacity full — next insert evicts Ev1
        assert!(!dedup.is_duplicate("Ev4"));
        // Ev1 was evicted, no longer detected as duplicate
        assert!(!dedup.is_duplicate("Ev1"));
        // Ev2 was evicted by Ev1's re-insertion
        assert!(!dedup.is_duplicate("Ev2"));
    }

    #[test]
    fn duplicate_after_many_inserts() {
        let mut dedup = EventDeduplicator::new();
        for i in 1..=500 {
            assert!(!dedup.is_duplicate(&format!("Ev{i}")));
        }
        // All 500 should still be in the buffer (capacity=5000)
        assert!(dedup.is_duplicate("Ev1"));
        assert!(dedup.is_duplicate("Ev500"));
    }
}
