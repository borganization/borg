use anyhow::Result;
use chrono::{Local, NaiveTime};
use cron::Schedule;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use tokio::sync::mpsc;
use tracing::{debug, info, instrument, warn};

use borg_core::config::HeartbeatConfig;
use borg_core::identity::load_identity;
use borg_core::llm::LlmClient;
use borg_core::memory::load_memory_context;
use borg_core::types::Message;

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
        if let Some(ref cron_expr) = self.config.cron {
            self.run_cron(cron_expr.clone(), tx).await;
        } else {
            self.run_interval(tx).await;
        }
    }

    async fn run_interval(&mut self, tx: mpsc::Sender<HeartbeatEvent>) {
        let interval =
            parse_interval(&self.config.interval).unwrap_or(std::time::Duration::from_secs(1800));
        info!("Heartbeat scheduler started (interval: {interval:?})");

        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // Skip immediate first tick

        loop {
            ticker.tick().await;
            self.tick(&tx).await;
        }
    }

    async fn run_cron(&mut self, cron_expr: String, tx: mpsc::Sender<HeartbeatEvent>) {
        let schedule = match Schedule::from_str(&cron_expr) {
            Ok(s) => s,
            Err(e) => {
                warn!("Invalid cron expression '{cron_expr}': {e}. Falling back to interval.");
                self.run_interval(tx).await;
                return;
            }
        };

        info!("Heartbeat scheduler started (cron: {cron_expr})");

        loop {
            let now = Local::now();
            let next = match schedule.upcoming(Local).next() {
                Some(t) => t,
                None => {
                    warn!("Cron schedule exhausted");
                    return;
                }
            };

            let wait = (next - now)
                .to_std()
                .unwrap_or(std::time::Duration::from_secs(60));
            tokio::time::sleep(wait).await;
            self.tick(&tx).await;
        }
    }

    #[instrument(skip_all)]
    async fn tick(&mut self, tx: &mpsc::Sender<HeartbeatEvent>) {
        if self.is_quiet_hours() {
            debug!("Heartbeat: in quiet hours, skipping");
            return;
        }

        match self.fire_heartbeat().await {
            Ok(Some(message)) => {
                let hash = hash_string(&message);
                if self.last_hash == Some(hash) {
                    debug!("Heartbeat: duplicate response, suppressing");
                    return;
                }
                self.last_hash = Some(hash);

                if message.trim().is_empty() {
                    debug!("Heartbeat: empty response, suppressing");
                    return;
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

    async fn fire_heartbeat(&self) -> Result<Option<String>> {
        let identity = load_identity().unwrap_or_default();
        let memory = load_memory_context(4000).unwrap_or_default();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S %Z");

        let system = format!(
            "{identity}\n\n# Current Time\n{now}\n\n{memory}\n\n\
            # Heartbeat Instructions\n\
            You are checking in on your owner proactively. \
            If you have something useful, timely, or caring to say, say it briefly. \
            If you have nothing meaningful to contribute, respond with an empty message. \
            Keep it short — one or two sentences max."
        );

        let messages = vec![Message::system(system), Message::user("*heartbeat tick*")];

        let response = self.llm.chat(&messages, None).await?;
        Ok(response.text_content().map(String::from))
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
        assert_eq!(d, std::time::Duration::from_secs(30 * 60));
    }

    #[test]
    fn parse_interval_hours() {
        let d = parse_interval("2h").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(2 * 3600));
    }

    #[test]
    fn parse_interval_seconds() {
        let d = parse_interval("45s").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(45));
    }

    #[test]
    fn parse_interval_bare_number() {
        let d = parse_interval("120").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(120));
    }

    #[test]
    fn parse_interval_with_whitespace() {
        let d = parse_interval("  10m  ").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(600));
    }

    #[test]
    fn parse_interval_invalid() {
        assert!(parse_interval("abc").is_none());
        assert!(parse_interval("").is_none());
    }

    #[test]
    fn hash_string_deterministic() {
        let h1 = hash_string("hello");
        let h2 = hash_string("hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_string_different_for_different_inputs() {
        let h1 = hash_string("hello");
        let h2 = hash_string("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn quiet_hours_no_config() {
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: None,
            quiet_hours_end: None,
            cron: None,
        };
        let llm = test_scheduler(config);
        assert!(!llm.is_quiet_hours());
    }

    #[test]
    fn quiet_hours_invalid_format() {
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: Some("not-a-time".to_string()),
            quiet_hours_end: Some("also-bad".to_string()),
            cron: None,
        };
        let sched = test_scheduler(config);
        // Invalid times should not be treated as quiet hours
        assert!(!sched.is_quiet_hours());
    }

    fn test_scheduler(config: HeartbeatConfig) -> HeartbeatScheduler {
        HeartbeatScheduler {
            config,
            llm: make_test_llm(),
            last_hash: None,
        }
    }

    fn make_test_llm() -> LlmClient {
        use borg_core::config::Config;
        // Set the env var so LlmClient::new doesn't fail
        std::env::set_var("OPENROUTER_API_KEY", "test-key");
        let config = Config::default();
        LlmClient::new(config).expect("should create LlmClient for testing")
    }
}
