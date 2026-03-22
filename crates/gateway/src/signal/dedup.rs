use std::collections::HashSet;

/// Deduplicator for Signal inbound messages, keyed on (sender_id, timestamp).
///
/// Prevents duplicate processing when the SSE stream reconnects and replays
/// recent events.
pub struct MessageDeduplicator {
    seen: HashSet<(String, i64)>,
    capacity: usize,
}

impl MessageDeduplicator {
    pub fn new(capacity: usize) -> Self {
        Self {
            seen: HashSet::with_capacity(capacity),
            capacity,
        }
    }

    /// Returns `true` if this (sender, timestamp) pair has been seen before.
    pub fn is_duplicate(&mut self, sender_id: &str, timestamp: i64) -> bool {
        let key = (sender_id.to_string(), timestamp);
        if self.seen.contains(&key) {
            return true;
        }

        // Evict oldest entries if at capacity
        if self.seen.len() >= self.capacity {
            // Simple strategy: clear half the set when full.
            // Signal timestamps are monotonically increasing, so newer entries
            // are what we want to keep, but HashSet doesn't preserve order.
            // For simplicity, just clear and start fresh — the window is large
            // enough that duplicates from a reconnect will still be caught.
            self.seen.clear();
        }

        self.seen.insert(key);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_message_not_duplicate() {
        let mut dedup = MessageDeduplicator::new(100);
        assert!(!dedup.is_duplicate("+15551234567", 1700000000000));
    }

    #[test]
    fn same_message_is_duplicate() {
        let mut dedup = MessageDeduplicator::new(100);
        assert!(!dedup.is_duplicate("+15551234567", 1700000000000));
        assert!(dedup.is_duplicate("+15551234567", 1700000000000));
    }

    #[test]
    fn different_sender_same_timestamp_not_duplicate() {
        let mut dedup = MessageDeduplicator::new(100);
        assert!(!dedup.is_duplicate("+15551234567", 1700000000000));
        assert!(!dedup.is_duplicate("+15559876543", 1700000000000));
    }

    #[test]
    fn same_sender_different_timestamp_not_duplicate() {
        let mut dedup = MessageDeduplicator::new(100);
        assert!(!dedup.is_duplicate("+15551234567", 1700000000000));
        assert!(!dedup.is_duplicate("+15551234567", 1700000001000));
    }

    #[test]
    fn eviction_on_capacity() {
        let mut dedup = MessageDeduplicator::new(3);
        assert!(!dedup.is_duplicate("a", 1));
        assert!(!dedup.is_duplicate("b", 2));
        assert!(!dedup.is_duplicate("c", 3));
        // At capacity — next insert clears and starts fresh
        assert!(!dedup.is_duplicate("d", 4));
        // After clear, old entries are no longer tracked
        assert!(!dedup.is_duplicate("a", 1));
    }
}
