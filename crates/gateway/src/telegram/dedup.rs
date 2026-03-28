use borg_core::constants;

use crate::dedup::BoundedDedup;

const DEFAULT_CAPACITY: usize = constants::TELEGRAM_DEDUP_CAPACITY;

/// Bounded deduplicator for Telegram update IDs.
pub struct UpdateDeduplicator(BoundedDedup<i64>);

impl UpdateDeduplicator {
    pub fn new() -> Self {
        Self(BoundedDedup::new(DEFAULT_CAPACITY))
    }

    #[cfg(test)]
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self(BoundedDedup::new(capacity))
    }

    /// Returns `true` if this update_id has been seen before.
    pub fn is_duplicate(&mut self, update_id: i64) -> bool {
        self.0.is_duplicate(&update_id)
    }
}

impl Default for UpdateDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_seen_not_duplicate() {
        let mut dedup = UpdateDeduplicator::new();
        assert!(!dedup.is_duplicate(1));
    }

    #[test]
    fn second_seen_is_duplicate() {
        let mut dedup = UpdateDeduplicator::new();
        assert!(!dedup.is_duplicate(1));
        assert!(dedup.is_duplicate(1));
    }

    #[test]
    fn different_ids_not_duplicate() {
        let mut dedup = UpdateDeduplicator::new();
        assert!(!dedup.is_duplicate(1));
        assert!(!dedup.is_duplicate(2));
        assert!(!dedup.is_duplicate(3));
    }

    #[test]
    fn eviction_at_capacity() {
        let mut dedup = UpdateDeduplicator::with_capacity(3);

        assert!(!dedup.is_duplicate(1));
        assert!(!dedup.is_duplicate(2));
        assert!(!dedup.is_duplicate(3));
        // Capacity full — next insert evicts 1
        assert!(!dedup.is_duplicate(4));
        // 1 was evicted, no longer detected as duplicate
        assert!(!dedup.is_duplicate(1));
        // After adding 4, order is [2,3,4]. Adding 1 evicts 2 -> [3,4,1]
        assert!(!dedup.is_duplicate(2));
    }

    #[test]
    fn duplicate_after_many_inserts() {
        let mut dedup = UpdateDeduplicator::new();
        for i in 1..=500 {
            assert!(!dedup.is_duplicate(i));
        }
        // All 500 should still be in the buffer (capacity=1000)
        assert!(dedup.is_duplicate(1));
        assert!(dedup.is_duplicate(500));
    }
}
