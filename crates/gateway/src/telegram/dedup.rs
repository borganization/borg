use std::collections::{HashSet, VecDeque};

const DEFAULT_CAPACITY: usize = 1000;

/// Bounded deduplicator for Telegram update IDs.
/// Uses a HashSet for O(1) lookups with a VecDeque for eviction order.
pub struct UpdateDeduplicator {
    order: VecDeque<i64>,
    set: HashSet<i64>,
    capacity: usize,
}

impl UpdateDeduplicator {
    pub fn new() -> Self {
        Self {
            order: VecDeque::with_capacity(DEFAULT_CAPACITY),
            set: HashSet::with_capacity(DEFAULT_CAPACITY),
            capacity: DEFAULT_CAPACITY,
        }
    }

    /// Returns `true` if this update_id has been seen before.
    pub fn is_duplicate(&mut self, update_id: i64) -> bool {
        if self.set.contains(&update_id) {
            return true;
        }

        if self.order.len() >= self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.set.remove(&evicted);
            }
        }
        self.order.push_back(update_id);
        self.set.insert(update_id);
        false
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
        let mut dedup = UpdateDeduplicator {
            order: VecDeque::with_capacity(3),
            set: HashSet::with_capacity(3),
            capacity: 3,
        };

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
