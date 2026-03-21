use chrono::NaiveTime;
use chrono_tz::Tz;
use cron::Schedule;
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use borg_core::config::HeartbeatConfig;

/// Minimum allowed heartbeat interval (60 seconds) to prevent API waste.
const MIN_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Clone)]
pub enum HeartbeatEvent {
    /// Timer fired — consumer should run a heartbeat agent turn.
    Fire,
    /// Heartbeat result to display (sent by consumer after agent turn completes).
    Message(String),
}

/// Pure timer that emits `Fire` events on schedule. Does not call LLM directly —
/// the consumer (daemon or TUI) is responsible for running the agent turn.
pub struct HeartbeatScheduler {
    config: HeartbeatConfig,
    timezone: Tz,
    wake_rx: Option<mpsc::Receiver<()>>,
}

impl HeartbeatScheduler {
    pub fn new(config: HeartbeatConfig, timezone: Tz, wake_rx: mpsc::Receiver<()>) -> Self {
        Self {
            config,
            timezone,
            wake_rx: Some(wake_rx),
        }
    }

    pub async fn run(mut self, tx: mpsc::Sender<HeartbeatEvent>, cancel: CancellationToken) {
        if let Some(ref cron_expr) = self.config.cron {
            self.run_cron(cron_expr.clone(), tx, cancel).await;
        } else {
            self.run_interval(tx, cancel).await;
        }
    }

    /// Receive a wake signal, or pend forever if the channel is closed/absent.
    async fn recv_wake(&mut self) {
        match &mut self.wake_rx {
            Some(rx) => match rx.recv().await {
                Some(()) => {}
                None => {
                    // Sender dropped — disable wake to avoid busy-loop
                    debug!("Wake channel closed, disabling wake");
                    self.wake_rx = None;
                    std::future::pending().await
                }
            },
            None => std::future::pending().await,
        }
    }

    async fn run_interval(&mut self, tx: mpsc::Sender<HeartbeatEvent>, cancel: CancellationToken) {
        let mut interval =
            parse_interval(&self.config.interval).unwrap_or(std::time::Duration::from_secs(1800));

        if interval.as_secs() < MIN_INTERVAL_SECS {
            warn!(
                "Heartbeat interval {}s is below minimum {}s, clamping",
                interval.as_secs(),
                MIN_INTERVAL_SECS
            );
            interval = std::time::Duration::from_secs(MIN_INTERVAL_SECS);
        }

        info!("Heartbeat scheduler started (interval: {interval:?})");

        let mut ticker = tokio::time::interval(interval);
        ticker.tick().await; // Skip immediate first tick

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    info!("Heartbeat scheduler shutting down");
                    return;
                }
                _ = self.recv_wake() => {
                    // Wake signal — fire immediately, skip quiet hours (intentional wake)
                    debug!("Heartbeat: wake signal received, firing immediately");
                    let _ = tx.send(HeartbeatEvent::Fire).await;
                }
                _ = ticker.tick() => {
                    if self.is_quiet_hours() {
                        debug!("Heartbeat: in quiet hours, skipping");
                        continue;
                    }
                    let _ = tx.send(HeartbeatEvent::Fire).await;
                }
            }
        }
    }

    async fn run_cron(
        &mut self,
        cron_expr: String,
        tx: mpsc::Sender<HeartbeatEvent>,
        cancel: CancellationToken,
    ) {
        let schedule = match Schedule::from_str(&cron_expr) {
            Ok(s) => s,
            Err(e) => {
                warn!("Invalid cron expression '{cron_expr}': {e}. Falling back to interval.");
                self.run_interval(tx, cancel).await;
                return;
            }
        };

        info!("Heartbeat scheduler started (cron: {cron_expr})");

        loop {
            let now = chrono::Local::now();
            let next = match schedule.upcoming(chrono::Local).next() {
                Some(t) => t,
                None => {
                    warn!("Cron schedule exhausted");
                    return;
                }
            };

            let wait = (next - now)
                .to_std()
                .unwrap_or(std::time::Duration::from_secs(60));

            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    info!("Heartbeat scheduler shutting down");
                    return;
                }
                _ = self.recv_wake() => {
                    debug!("Heartbeat: wake signal received, firing immediately");
                    let _ = tx.send(HeartbeatEvent::Fire).await;
                }
                _ = tokio::time::sleep(wait) => {
                    if self.is_quiet_hours() {
                        debug!("Heartbeat: in quiet hours, skipping");
                        continue;
                    }
                    let _ = tx.send(HeartbeatEvent::Fire).await;
                }
            }
        }
    }

    fn is_quiet_hours(&self) -> bool {
        let (Some(start_str), Some(end_str)) =
            (&self.config.quiet_hours_start, &self.config.quiet_hours_end)
        else {
            return false;
        };

        let Ok(start) = NaiveTime::parse_from_str(start_str, "%H:%M") else {
            warn!("Invalid quiet_hours_start format '{start_str}', expected HH:MM — quiet hours disabled");
            return false;
        };
        let Ok(end) = NaiveTime::parse_from_str(end_str, "%H:%M") else {
            warn!(
                "Invalid quiet_hours_end format '{end_str}', expected HH:MM — quiet hours disabled"
            );
            return false;
        };

        let now = chrono::Utc::now().with_timezone(&self.timezone).time();

        if start <= end {
            now >= start && now < end
        } else {
            now >= start || now < end
        }
    }
}

pub fn parse_interval(s: &str) -> Option<std::time::Duration> {
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

    fn test_scheduler(config: HeartbeatConfig, tz: Tz) -> HeartbeatScheduler {
        let (_tx, rx) = mpsc::channel(1);
        HeartbeatScheduler {
            config,
            timezone: tz,
            wake_rx: Some(rx),
        }
    }

    #[test]
    fn quiet_hours_no_config() {
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: None,
            quiet_hours_end: None,
            cron: None,
            channels: Vec::new(),
        };
        let sched = test_scheduler(config, chrono_tz::UTC);
        assert!(!sched.is_quiet_hours());
    }

    #[test]
    fn quiet_hours_invalid_format() {
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: Some("not-a-time".to_string()),
            quiet_hours_end: Some("also-bad".to_string()),
            cron: None,
            channels: Vec::new(),
        };
        let sched = test_scheduler(config, chrono_tz::UTC);
        assert!(!sched.is_quiet_hours());
    }

    #[test]
    fn quiet_hours_uses_timezone() {
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: Some("00:00".to_string()),
            quiet_hours_end: Some("06:00".to_string()),
            cron: None,
            channels: Vec::new(),
        };
        // Verify the timezone field is used and doesn't panic
        let sched = test_scheduler(config, chrono_tz::US::Eastern);
        let _ = sched.is_quiet_hours();
    }

    #[test]
    fn default_config_has_quiet_hours() {
        let config = HeartbeatConfig::default();
        assert_eq!(config.quiet_hours_start, Some("00:00".to_string()));
        assert_eq!(config.quiet_hours_end, Some("06:00".to_string()));
        assert!(config.channels.is_empty());
    }

    #[test]
    fn parse_interval_zero_seconds() {
        let d = parse_interval("0s").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(0));
    }

    #[test]
    fn parse_interval_zero_bare() {
        let d = parse_interval("0").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(0));
    }

    #[test]
    fn parse_interval_large_hours() {
        let d = parse_interval("24h").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(24 * 3600));
    }

    #[test]
    fn quiet_hours_wraparound_midnight() {
        // Quiet hours that cross midnight: 22:00 - 06:00
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: Some("22:00".to_string()),
            quiet_hours_end: Some("06:00".to_string()),
            cron: None,
            channels: Vec::new(),
        };
        let sched = test_scheduler(config, chrono_tz::UTC);
        // This test just ensures the wrap-around path doesn't panic.
        // The actual result depends on current UTC time.
        let _ = sched.is_quiet_hours();
    }

    #[test]
    fn quiet_hours_only_start_configured() {
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: Some("00:00".to_string()),
            quiet_hours_end: None,
            cron: None,
            channels: Vec::new(),
        };
        let sched = test_scheduler(config, chrono_tz::UTC);
        // Missing end => not quiet
        assert!(!sched.is_quiet_hours());
    }

    #[tokio::test]
    async fn scheduler_cancels_cleanly() {
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: None,
            quiet_hours_end: None,
            cron: None,
            channels: Vec::new(),
        };

        let (_wake_tx, wake_rx) = mpsc::channel(1);
        let (fire_tx, _fire_rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();

        let scheduler = HeartbeatScheduler::new(config, chrono_tz::UTC, wake_rx);
        let cancel_clone = cancel.clone();
        let handle =
            tokio::spawn(async move { scheduler.run(fire_tx, cancel_clone).await });

        // Cancel immediately
        cancel.cancel();

        // Should complete without hanging
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("scheduler did not shut down in time")
            .expect("scheduler task panicked");
    }

    #[tokio::test]
    async fn wake_signal_triggers_fire() {
        let config = HeartbeatConfig {
            enabled: true,
            interval: "30m".to_string(),
            quiet_hours_start: None,
            quiet_hours_end: None,
            cron: None,
            channels: Vec::new(),
        };

        let (wake_tx, wake_rx) = mpsc::channel(8);
        let (fire_tx, mut fire_rx) = mpsc::channel(32);
        let cancel = CancellationToken::new();

        let scheduler = HeartbeatScheduler::new(config, chrono_tz::UTC, wake_rx);
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            scheduler.run(fire_tx, cancel_clone).await;
        });

        // Send wake signal
        wake_tx.send(()).await.unwrap();

        // Should receive a Fire event
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), fire_rx.recv())
            .await
            .expect("timed out waiting for Fire event")
            .expect("channel closed");

        assert!(matches!(event, HeartbeatEvent::Fire));

        cancel.cancel();
    }
}
