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
