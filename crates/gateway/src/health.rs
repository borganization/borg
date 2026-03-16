use std::collections::HashMap;
use std::time::Instant;

use serde::Serialize;

#[derive(Debug)]
pub struct ChannelHealth {
    pub last_inbound_at: Option<Instant>,
    pub last_outbound_at: Option<Instant>,
    pub last_error: Option<String>,
    pub last_error_at: Option<Instant>,
    pub inbound_count: u64,
    pub outbound_count: u64,
    pub error_count: u64,
    pub reconnect_attempts: u64,
    created_at: Instant,
}

impl ChannelHealth {
    fn new() -> Self {
        Self {
            last_inbound_at: None,
            last_outbound_at: None,
            last_error: None,
            last_error_at: None,
            inbound_count: 0,
            outbound_count: 0,
            error_count: 0,
            reconnect_attempts: 0,
            created_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelHealthSnapshot {
    pub name: String,
    pub inbound_count: u64,
    pub outbound_count: u64,
    pub error_count: u64,
    pub reconnect_attempts: u64,
    pub last_error: Option<String>,
    pub uptime_secs: u64,
    pub last_inbound_ago_secs: Option<u64>,
    pub last_outbound_ago_secs: Option<u64>,
    pub last_error_ago_secs: Option<u64>,
}

#[derive(Debug, Default)]
pub struct ChannelHealthRegistry {
    channels: HashMap<String, ChannelHealth>,
}

impl ChannelHealthRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: &str) {
        self.channels
            .entry(name.to_string())
            .or_insert_with(ChannelHealth::new);
    }

    pub fn record_inbound(&mut self, name: &str) {
        let entry = self
            .channels
            .entry(name.to_string())
            .or_insert_with(ChannelHealth::new);
        entry.last_inbound_at = Some(Instant::now());
        entry.inbound_count += 1;
    }

    pub fn record_outbound(&mut self, name: &str) {
        let entry = self
            .channels
            .entry(name.to_string())
            .or_insert_with(ChannelHealth::new);
        entry.last_outbound_at = Some(Instant::now());
        entry.outbound_count += 1;
    }

    pub fn record_error(&mut self, name: &str, msg: &str) {
        let entry = self
            .channels
            .entry(name.to_string())
            .or_insert_with(ChannelHealth::new);
        let truncated = if msg.len() > 512 {
            format!("{}...", &msg[..509])
        } else {
            msg.to_string()
        };
        entry.last_error = Some(truncated);
        entry.last_error_at = Some(Instant::now());
        entry.error_count += 1;
    }

    pub fn record_reconnect(&mut self, name: &str) {
        let entry = self
            .channels
            .entry(name.to_string())
            .or_insert_with(ChannelHealth::new);
        entry.reconnect_attempts += 1;
    }

    pub fn snapshot(&self) -> Vec<ChannelHealthSnapshot> {
        let now = Instant::now();
        self.channels
            .iter()
            .map(|(name, h)| ChannelHealthSnapshot {
                name: name.clone(),
                inbound_count: h.inbound_count,
                outbound_count: h.outbound_count,
                error_count: h.error_count,
                reconnect_attempts: h.reconnect_attempts,
                last_error: h.last_error.clone(),
                uptime_secs: now.duration_since(h.created_at).as_secs(),
                last_inbound_ago_secs: h.last_inbound_at.map(|t| now.duration_since(t).as_secs()),
                last_outbound_ago_secs: h.last_outbound_at.map(|t| now.duration_since(t).as_secs()),
                last_error_ago_secs: h.last_error_at.map(|t| now.duration_since(t).as_secs()),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_increments() {
        let mut reg = ChannelHealthRegistry::new();
        reg.register("slack");

        reg.record_inbound("slack");
        reg.record_inbound("slack");
        reg.record_outbound("slack");
        reg.record_error("slack", "timeout");

        let snap = reg.snapshot();
        let s = snap.iter().find(|s| s.name == "slack").unwrap();
        assert_eq!(s.inbound_count, 2);
        assert_eq!(s.outbound_count, 1);
        assert_eq!(s.error_count, 1);
        assert_eq!(s.last_error.as_deref(), Some("timeout"));
    }

    #[test]
    fn snapshot_format() {
        let mut reg = ChannelHealthRegistry::new();
        reg.register("discord");
        reg.record_inbound("discord");

        let snap = reg.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].name, "discord");
        assert!(snap[0].last_inbound_ago_secs.is_some());
        assert!(snap[0].last_outbound_ago_secs.is_none());
    }

    #[test]
    fn unregistered_channel_auto_created() {
        let mut reg = ChannelHealthRegistry::new();
        reg.record_inbound("new-channel");

        let snap = reg.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].inbound_count, 1);
    }

    #[test]
    fn reconnect_tracking() {
        let mut reg = ChannelHealthRegistry::new();
        reg.register("slack");
        reg.record_reconnect("slack");
        reg.record_reconnect("slack");

        let snap = reg.snapshot();
        let s = snap.iter().find(|s| s.name == "slack").unwrap();
        assert_eq!(s.reconnect_attempts, 2);
    }
}
