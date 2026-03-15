use anyhow::Result;
use chrono::Local;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::config::Config;
use crate::types::Message;

#[derive(Debug, Serialize)]
struct LogEntry {
    timestamp: String,
    #[serde(flatten)]
    kind: LogEntryKind,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum LogEntryKind {
    #[serde(rename = "user_message")]
    UserMessage { content: String },
    #[serde(rename = "assistant_message")]
    AssistantMessage {
        content: Option<String>,
        tool_calls: Option<Vec<LogToolCall>>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct LogToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn log_dir() -> Result<PathBuf> {
    let dir = Config::data_dir()?.join("logs");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn log_path() -> Result<PathBuf> {
    let date = Local::now().format("%Y-%m-%d");
    Ok(log_dir()?.join(format!("{date}.jsonl")))
}

fn append_entry(entry: &LogEntry) -> Result<()> {
    let path = log_path()?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    let line = serde_json::to_string(entry)?;
    writeln!(file, "{line}")?;
    Ok(())
}

pub fn log_message(message: &Message) {
    let timestamp = Local::now().to_rfc3339();

    let kind = match message.role {
        crate::types::Role::User => {
            let content = message.content.clone().unwrap_or_default();
            LogEntryKind::UserMessage { content }
        }
        crate::types::Role::Assistant => LogEntryKind::AssistantMessage {
            content: message.content.clone(),
            tool_calls: message.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| LogToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    })
                    .collect()
            }),
        },
        crate::types::Role::Tool => LogEntryKind::ToolResult {
            tool_call_id: message.tool_call_id.clone().unwrap_or_default(),
            content: message.content.clone().unwrap_or_default(),
        },
        crate::types::Role::System => return,
    };

    let entry = LogEntry { timestamp, kind };
    if let Err(e) = append_entry(&entry) {
        tracing::warn!("Failed to write conversation log: {e}");
    }
}

#[derive(Debug, Default)]
pub struct UsageStats {
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub tool_calls: usize,
}

pub fn count_messages_for_period(days: i64) -> Result<UsageStats> {
    let dir = log_dir()?;
    let today = Local::now().date_naive();
    count_messages_in_dir(&dir, today, days)
}

fn count_messages_in_dir(
    dir: &std::path::Path,
    today: chrono::NaiveDate,
    days: i64,
) -> Result<UsageStats> {
    let mut stats = UsageStats::default();

    for d in 0..days {
        let date = today - chrono::Duration::days(d);
        let path = dir.join(format!("{}.jsonl", date.format("%Y-%m-%d")));
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        for line in content.lines() {
            if line.contains("\"user_message\"") {
                stats.user_messages += 1;
            } else if line.contains("\"assistant_message\"") {
                stats.assistant_messages += 1;
            } else if line.contains("\"tool_result\"") {
                stats.tool_calls += 1;
            }
        }
    }

    Ok(stats)
}

pub fn read_history(lines: usize) -> Result<Vec<String>> {
    let path = log_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)?;
    let all_lines: Vec<String> = content.lines().map(String::from).collect();
    let start = all_lines.len().saturating_sub(lines);
    Ok(all_lines[start..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn count_messages_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let today = Local::now().date_naive();
        let stats = count_messages_in_dir(dir.path(), today, 7).unwrap();
        assert_eq!(stats.user_messages, 0);
        assert_eq!(stats.assistant_messages, 0);
        assert_eq!(stats.tool_calls, 0);
    }

    #[test]
    fn count_messages_single_day() {
        let dir = tempfile::tempdir().unwrap();
        let today = Local::now().date_naive();
        let filename = format!("{}.jsonl", today.format("%Y-%m-%d"));
        let path = dir.path().join(filename);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"user_message","content":"hello"}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant_message","content":"hi"}}"#).unwrap();
        writeln!(f, r#"{{"type":"user_message","content":"do stuff"}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"tool_result","tool_call_id":"c1","content":"ok"}}"#
        )
        .unwrap();

        let stats = count_messages_in_dir(dir.path(), today, 1).unwrap();
        assert_eq!(stats.user_messages, 2);
        assert_eq!(stats.assistant_messages, 1);
        assert_eq!(stats.tool_calls, 1);
    }

    #[test]
    fn count_messages_multi_day() {
        let dir = tempfile::tempdir().unwrap();
        let today = Local::now().date_naive();

        // Today
        let path = dir
            .path()
            .join(format!("{}.jsonl", today.format("%Y-%m-%d")));
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"user_message","content":"today"}}"#).unwrap();

        // Yesterday
        let yesterday = today - chrono::Duration::days(1);
        let path = dir
            .path()
            .join(format!("{}.jsonl", yesterday.format("%Y-%m-%d")));
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"user_message","content":"yesterday"}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant_message","content":"resp"}}"#).unwrap();

        let stats = count_messages_in_dir(dir.path(), today, 2).unwrap();
        assert_eq!(stats.user_messages, 2);
        assert_eq!(stats.assistant_messages, 1);

        // Only 1 day should exclude yesterday
        let stats = count_messages_in_dir(dir.path(), today, 1).unwrap();
        assert_eq!(stats.user_messages, 1);
        assert_eq!(stats.assistant_messages, 0);
    }

    #[test]
    fn count_messages_zero_days() {
        let dir = tempfile::tempdir().unwrap();
        let today = Local::now().date_naive();
        let stats = count_messages_in_dir(dir.path(), today, 0).unwrap();
        assert_eq!(stats.user_messages, 0);
    }

    #[test]
    fn usage_stats_default() {
        let stats = UsageStats::default();
        assert_eq!(stats.user_messages, 0);
        assert_eq!(stats.assistant_messages, 0);
        assert_eq!(stats.tool_calls, 0);
    }
}
