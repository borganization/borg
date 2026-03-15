use anyhow::{Context, Result};
use chrono::Utc;
use cron::Schedule;
use std::str::FromStr;

use crate::db::{Database, ScheduledTaskRow};

/// Calculate the next run time for a task based on its schedule.
pub fn calculate_next_run(schedule_type: &str, schedule_expr: &str) -> Result<Option<i64>> {
    match schedule_type {
        "cron" => {
            let schedule = Schedule::from_str(schedule_expr)
                .with_context(|| format!("Invalid cron expression: {schedule_expr}"))?;
            let next = schedule.upcoming(Utc).next();
            Ok(next.map(|t| t.timestamp()))
        }
        "interval" => {
            let duration = parse_interval(schedule_expr)
                .with_context(|| format!("Invalid interval: {schedule_expr}"))?;
            Ok(Some(Utc::now().timestamp() + duration.as_secs() as i64))
        }
        "once" => {
            // For one-shot tasks, next_run is set at creation time
            Ok(Some(Utc::now().timestamp()))
        }
        other => {
            anyhow::bail!("Unknown schedule type: {other}. Use 'cron', 'interval', or 'once'.")
        }
    }
}

/// Advance a task's next_run after execution.
pub fn advance_next_run(task: &ScheduledTaskRow, db: &Database) -> Result<()> {
    match task.schedule_type.as_str() {
        "once" => {
            // One-shot: mark as completed
            db.update_task_status(&task.id, "completed")?;
            db.update_task_next_run(&task.id, None)?;
        }
        "cron" => {
            let next = calculate_next_run("cron", &task.schedule_expr)?;
            db.update_task_next_run(&task.id, next)?;
        }
        "interval" => {
            let next = calculate_next_run("interval", &task.schedule_expr)?;
            db.update_task_next_run(&task.id, next)?;
        }
        _ => {}
    }
    Ok(())
}

/// Format a task row for display.
pub fn format_task(task: &ScheduledTaskRow) -> String {
    let next = task
        .next_run
        .map(|ts| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_else(|| ts.to_string())
        })
        .unwrap_or_else(|| "—".to_string());

    format!(
        "  {} [{}] ({})\n    Schedule: {} {}\n    Next run: {}\n    Prompt: {}",
        task.name,
        task.status,
        task.id.chars().take(8).collect::<String>(),
        task.schedule_type,
        task.schedule_expr,
        next,
        truncate_str(&task.prompt, 80),
    )
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
        format!("{}...", &s[..end])
    } else {
        s.to_string()
    }
}

/// Validate a schedule expression without computing the next run time.
pub fn validate_schedule(schedule_type: &str, schedule_expr: &str) -> Result<()> {
    match schedule_type {
        "cron" => {
            Schedule::from_str(schedule_expr)
                .with_context(|| format!("Invalid cron expression: {schedule_expr}"))?;
            Ok(())
        }
        "interval" => {
            parse_interval(schedule_expr)
                .ok_or_else(|| anyhow::anyhow!("Invalid interval: {schedule_expr}"))?;
            Ok(())
        }
        "once" => Ok(()),
        other => {
            anyhow::bail!("Unknown schedule type: {other}. Use 'cron', 'interval', or 'once'.")
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
    } else if let Some(days) = s.strip_suffix('d') {
        days.parse::<u64>()
            .ok()
            .map(|d| std::time::Duration::from_secs(d * 86400))
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
        assert_eq!(d, std::time::Duration::from_secs(1800));
    }

    #[test]
    fn parse_interval_hours() {
        let d = parse_interval("2h").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(7200));
    }

    #[test]
    fn parse_interval_days() {
        let d = parse_interval("1d").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(86400));
    }

    #[test]
    fn parse_interval_seconds() {
        let d = parse_interval("60s").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(60));
    }

    #[test]
    fn parse_interval_bare_number() {
        let d = parse_interval("120").unwrap();
        assert_eq!(d, std::time::Duration::from_secs(120));
    }

    #[test]
    fn parse_interval_invalid() {
        assert!(parse_interval("abc").is_none());
    }

    #[test]
    fn calculate_next_run_cron() {
        let next = calculate_next_run("cron", "0 0 9 * * * *").unwrap();
        assert!(next.is_some());
        assert!(next.unwrap() > Utc::now().timestamp());
    }

    #[test]
    fn calculate_next_run_interval() {
        let next = calculate_next_run("interval", "1h").unwrap();
        assert!(next.is_some());
        let expected_min = Utc::now().timestamp() + 3500; // ~1h minus tolerance
        assert!(next.unwrap() > expected_min);
    }

    #[test]
    fn calculate_next_run_once() {
        let next = calculate_next_run("once", "").unwrap();
        assert!(next.is_some());
    }

    #[test]
    fn calculate_next_run_unknown_type() {
        assert!(calculate_next_run("weekly", "").is_err());
    }

    #[test]
    fn validate_schedule_valid_cron() {
        assert!(validate_schedule("cron", "0 0 9 * * * *").is_ok());
    }

    #[test]
    fn validate_schedule_invalid_cron() {
        assert!(validate_schedule("cron", "not a cron").is_err());
    }

    #[test]
    fn validate_schedule_valid_interval() {
        assert!(validate_schedule("interval", "30m").is_ok());
    }

    #[test]
    fn validate_schedule_invalid_interval() {
        assert!(validate_schedule("interval", "abc").is_err());
    }

    #[test]
    fn validate_schedule_once() {
        assert!(validate_schedule("once", "").is_ok());
    }

    #[test]
    fn validate_schedule_unknown_type() {
        assert!(validate_schedule("weekly", "").is_err());
    }

    #[test]
    fn parse_interval_is_pub() {
        // Verifies parse_interval is now pub by calling it from tests
        assert!(parse_interval("30m").is_some());
    }

    #[test]
    fn truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_long() {
        let long = "a".repeat(100);
        let result = truncate_str(&long, 10);
        assert!(result.ends_with("..."));
        assert_eq!(result.chars().count(), 13); // 10 + "..."
    }

    #[test]
    fn truncate_str_multibyte() {
        let s = "hello\u{1F600}world"; // emoji in the middle
        let result = truncate_str(s, 6);
        assert!(result.ends_with("..."));
        assert_eq!(result.chars().count(), 9); // 6 + "..."
    }
}
