use std::collections::{HashSet, VecDeque};

use borg_core::constants;

const DEFAULT_CAPACITY: usize = constants::SLACK_DEDUP_CAPACITY;

/// Bounded deduplicator for Slack event IDs.
/// Uses a HashSet for O(1) lookups with a VecDeque for eviction order.
pub struct EventDeduplicator {
    order: VecDeque<String>,
    set: HashSet<String>,
    capacity: usize,
}

impl EventDeduplicator {
    pub fn new() -> Self {
        Self {
            order: VecDeque::with_capacity(DEFAULT_CAPACITY),
            set: HashSet::with_capacity(DEFAULT_CAPACITY),
            capacity: DEFAULT_CAPACITY,
        }
    }

    /// Returns `true` if this event_id has been seen before.
    pub fn is_duplicate(&mut self, event_id: &str) -> bool {
        if self.set.contains(event_id) {
            return true;
        }

        if self.order.len() >= self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.set.remove(&evicted);
            }
        }
        let owned = event_id.to_string();
        self.set.insert(owned.clone());
        self.order.push_back(owned);
        false
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
        let mut dedup = EventDeduplicator {
            order: VecDeque::with_capacity(3),
            set: HashSet::with_capacity(3),
            capacity: 3,
        };

        assert!(!dedup.is_duplicate("Ev001"));
        assert!(!dedup.is_duplicate("Ev002"));
        assert!(!dedup.is_duplicate("Ev003"));
        // Capacity full — next insert evicts Ev001
        assert!(!dedup.is_duplicate("Ev004"));
        // Ev001 was evicted, no longer detected as duplicate
        assert!(!dedup.is_duplicate("Ev001"));
        // After adding Ev004, order is [Ev002,Ev003,Ev004]. Adding Ev001 evicts Ev002
        assert!(!dedup.is_duplicate("Ev002"));
    }

    #[test]
    fn duplicate_after_many_inserts() {
        let mut dedup = EventDeduplicator::new();
        for i in 1..=500 {
            assert!(!dedup.is_duplicate(&format!("Ev{i:04}")));
        }
        // All 500 should still be in the buffer (capacity=1000)
        assert!(dedup.is_duplicate("Ev0001"));
        assert!(dedup.is_duplicate("Ev0500"));
    }
}
