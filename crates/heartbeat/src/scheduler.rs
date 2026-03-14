use anyhow::Result;
use chrono::{Local, NaiveTime};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use tamagotchi_core::config::HeartbeatConfig;
use tamagotchi_core::llm::LlmClient;
use tamagotchi_core::memory::load_memory_context;
use tamagotchi_core::soul::load_soul;
use tamagotchi_core::types::Message;

#[derive(Debug, Clone)]
pub enum HeartbeatEvent {
    Message(String),
}

pub struct HeartbeatScheduler {
    config: HeartbeatConfig,
    llm: LlmClient,
    last_hash: Option<u64>,
}

impl HeartbeatScheduler {
    pub fn new(config: HeartbeatConfig, llm: LlmClient) -> Self {
        Self {
            config,
            llm,
            last_hash: None,
        }
    }

    pub async fn run(mut self, tx: mpsc::Sender<HeartbeatEvent>) {
        let interval =
            parse_interval(&self.config.interval).unwrap_or(std::time::Duration::from_secs(1800));
        info!("Heartbeat scheduler started (interval: {:?})", interval);

        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // Skip immediate first tick

        loop {
            ticker.tick().await;

            if self.is_quiet_hours() {
                debug!("Heartbeat: in quiet hours, skipping");
                continue;
            }

            match self.fire_heartbeat().await {
                Ok(Some(message)) => {
                    let hash = hash_string(&message);
                    if self.last_hash == Some(hash) {
                        debug!("Heartbeat: duplicate response, suppressing");
                        continue;
                    }
                    self.last_hash = Some(hash);

                    if message.trim().is_empty() {
                        debug!("Heartbeat: empty response, suppressing");
                        continue;
                    }

                    let _ = tx.send(HeartbeatEvent::Message(message)).await;
                }
                Ok(None) => {
                    debug!("Heartbeat: no response");
                }
                Err(e) => {
                    warn!("Heartbeat error: {e}");
                }
            }
        }
    }

    async fn fire_heartbeat(&self) -> Result<Option<String>> {
        let soul = load_soul().unwrap_or_default();
        let memory = load_memory_context(4000).unwrap_or_default();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S %Z");

        let system = format!(
            "{soul}\n\n# Current Time\n{now}\n\n{memory}\n\n\
            # Heartbeat Instructions\n\
            You are checking in on your owner proactively. \
            If you have something useful, timely, or caring to say, say it briefly. \
            If you have nothing meaningful to contribute, respond with an empty message. \
            Keep it short — one or two sentences max."
        );

        let messages = vec![Message::system(system), Message::user("*heartbeat tick*")];

        let response = self.llm.chat(&messages, None).await?;
        Ok(response.content)
    }

    fn is_quiet_hours(&self) -> bool {
        let (Some(start_str), Some(end_str)) =
            (&self.config.quiet_hours_start, &self.config.quiet_hours_end)
        else {
            return false;
        };

        let Ok(start) = NaiveTime::parse_from_str(start_str, "%H:%M") else {
            return false;
        };
        let Ok(end) = NaiveTime::parse_from_str(end_str, "%H:%M") else {
            return false;
        };

        let now = Local::now().time();

        if start <= end {
            now >= start && now < end
        } else {
            // Spans midnight (e.g., 23:00 to 07:00)
            now >= start || now < end
        }
    }
}

fn parse_interval(s: &str) -> Option<std::time::Duration> {
    let s = s.trim();
    if let Some(mins) = s.strip_suffix('m') {
        mins.parse::<u64>()
            .ok()
            .map(|m| std::time::Duration::from_secs(m * 60))
    } else if let Some(hours) = s.strip_suffix('h') {
        hours
            .parse::<u64>()
            .ok()
            .map(|h| std::time::Duration::from_secs(h * 3600))
    } else if let Some(secs) = s.strip_suffix('s') {
        secs.parse::<u64>().ok().map(std::time::Duration::from_secs)
    } else {
        s.parse::<u64>().ok().map(std::time::Duration::from_secs)
    }
}

fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval_minutes() {
        let d = parse_interval("30m").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(1800));
    }

    #[test]
    fn parse_interval_hours() {
        let d = parse_interval("2h").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(7200));
    }

    #[test]
    fn parse_interval_seconds_suffix() {
        let d = parse_interval("60s").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(60));
    }

    #[test]
    fn parse_interval_raw_number() {
        let d = parse_interval("120").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(120));
    }

    #[test]
    fn parse_interval_with_whitespace() {
        let d = parse_interval("  15m  ").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(900));
    }

    #[test]
    fn parse_interval_invalid_returns_none() {
        assert!(parse_interval("invalid").is_none());
        assert!(parse_interval("abc").is_none());
    }

    #[test]
    fn parse_interval_empty_returns_none() {
        assert!(parse_interval("").is_none());
    }

    #[test]
    fn parse_interval_one_minute() {
        let d = parse_interval("1m").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(60));
    }

    #[test]
    fn parse_interval_one_hour() {
        let d = parse_interval("1h").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(3600));
    }

    #[test]
    fn hash_string_consistent() {
        let h1 = hash_string("hello");
        let h2 = hash_string("hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_string_different_inputs() {
        let h1 = hash_string("hello");
        let h2 = hash_string("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_string_empty() {
        // Should not panic
        let _h = hash_string("");
    }

    #[test]
    fn quiet_hours_disabled_when_not_configured() {
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: None,
            quiet_hours_end: None,
            cron: None,
        };
        // is_quiet_hours is a method on HeartbeatScheduler, but we can't construct one
        // without LlmClient. Instead, test the logic inline:
        let (start, end) = (&config.quiet_hours_start, &config.quiet_hours_end);
        assert!(start.is_none());
        assert!(end.is_none());
        // The is_quiet_hours method returns false when either is None
    }

    #[test]
    fn quiet_hours_time_parsing() {
        // Test that the time format used by is_quiet_hours parses correctly
        let start = NaiveTime::parse_from_str("23:00", "%H:%M").unwrap();
        let end = NaiveTime::parse_from_str("07:00", "%H:%M").unwrap();
        assert!(start > end); // spans midnight

        let start2 = NaiveTime::parse_from_str("09:00", "%H:%M").unwrap();
        let end2 = NaiveTime::parse_from_str("17:00", "%H:%M").unwrap();
        assert!(start2 < end2); // same day
    }

    #[test]
    fn quiet_hours_midnight_spanning_logic() {
        // Test the quiet hours logic for midnight-spanning ranges
        let start = NaiveTime::parse_from_str("23:00", "%H:%M").unwrap();
        let end = NaiveTime::parse_from_str("07:00", "%H:%M").unwrap();

        // 02:00 should be in quiet hours (after midnight, before end)
        let time_2am = NaiveTime::parse_from_str("02:00", "%H:%M").unwrap();
        let in_quiet = time_2am >= start || time_2am < end;
        assert!(in_quiet);

        // 23:30 should be in quiet hours (after start, before midnight)
        let time_2330 = NaiveTime::parse_from_str("23:30", "%H:%M").unwrap();
        let in_quiet = time_2330 >= start || time_2330 < end;
        assert!(in_quiet);

        // 12:00 should NOT be in quiet hours
        let time_noon = NaiveTime::parse_from_str("12:00", "%H:%M").unwrap();
        let in_quiet = time_noon >= start || time_noon < end;
        assert!(!in_quiet);
    }

    #[test]
    fn quiet_hours_same_day_logic() {
        // Test same-day quiet hours (start < end)
        let start = NaiveTime::parse_from_str("09:00", "%H:%M").unwrap();
        let end = NaiveTime::parse_from_str("17:00", "%H:%M").unwrap();

        let time_noon = NaiveTime::parse_from_str("12:00", "%H:%M").unwrap();
        let in_quiet = time_noon >= start && time_noon < end;
        assert!(in_quiet);

        let time_8am = NaiveTime::parse_from_str("08:00", "%H:%M").unwrap();
        let in_quiet = time_8am >= start && time_8am < end;
        assert!(!in_quiet);

        let time_6pm = NaiveTime::parse_from_str("18:00", "%H:%M").unwrap();
        let in_quiet = time_6pm >= start && time_6pm < end;
        assert!(!in_quiet);
    }
}
