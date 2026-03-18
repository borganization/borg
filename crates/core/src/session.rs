use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::config::Config;
use crate::db::Database;
use crate::types::{Message, MessageContent, Role, ToolCall};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub meta: SessionMeta,
    pub messages: Vec<Message>,
}

fn sessions_dir() -> Result<PathBuf> {
    let dir = Config::sessions_dir()?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn validate_session_id(id: &str) -> Result<()> {
    if id.is_empty() {
        anyhow::bail!("Session ID must not be empty");
    }
    if id.contains("..") || id.contains('/') || id.contains('\\') {
        anyhow::bail!("Invalid session ID: must not contain path separators or '..'");
    }
    Ok(())
}

fn session_path(id: &str) -> Result<PathBuf> {
    validate_session_id(id)?;
    Ok(sessions_dir()?.join(format!("{id}.json")))
}

fn last_session_path() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("last_session"))
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Local::now().to_rfc3339();
        Self {
            meta: SessionMeta {
                id,
                title: "New conversation".to_string(),
                created_at: now.clone(),
                updated_at: now,
                message_count: 0,
            },
            messages: Vec::new(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = session_path(&self.meta.id)?;
        let tmp_path = path.with_extension("json.tmp");
        let json = serde_json::to_string(self)?;
        // Write to temp file first, then rename for atomicity
        fs::write(&tmp_path, &json)?;
        fs::rename(&tmp_path, &path)?;

        // Track as last session
        let last = last_session_path()?;
        fs::write(&last, &self.meta.id)?;
        Ok(())
    }

    pub fn load(id: &str) -> Result<Self> {
        let path = session_path(id)?;
        let json =
            fs::read_to_string(&path).with_context(|| format!("Session '{id}' not found"))?;
        let session: Session = serde_json::from_str(&json)?;
        Ok(session)
    }

    pub fn update_from_history(&mut self, history: &[Message]) {
        self.messages = history.to_vec();
        self.meta.message_count = history.len();
        self.meta.updated_at = Local::now().to_rfc3339();

        // Auto-title from first user message
        if self.meta.title == "New conversation" {
            if let Some(msg) = history.iter().find(|m| m.role == Role::User) {
                if let Some(content) = msg.text_content() {
                    let title: String = content.chars().take(60).collect();
                    self.meta.title = if content.chars().count() > 60 {
                        format!("{title}...")
                    } else {
                        title
                    };
                }
            }
        }
    }
}

pub fn load_last_session() -> Result<Option<Session>> {
    let last_path = last_session_path()?;
    if !last_path.exists() {
        return Ok(None);
    }
    let id = fs::read_to_string(&last_path)?.trim().to_string();
    if id.is_empty() {
        return Ok(None);
    }
    match Session::load(&id) {
        Ok(session) => Ok(Some(session)),
        Err(_) => Ok(None),
    }
}

/// Lightweight struct for deserializing only the `meta` field from session files,
/// avoiding full deserialization of potentially large message arrays.
#[derive(Deserialize)]
struct SessionMetaOnly {
    meta: SessionMeta,
}

/// Attempt to recover a session's messages from SQLite if the JSON file is stale or missing.
pub fn recover_session_from_db(session_id: &str) -> Result<Option<Vec<Message>>> {
    let db = Database::open()?;
    let rows = db.load_session_messages(session_id)?;
    if rows.is_empty() {
        return Ok(None);
    }

    let mut messages = Vec::with_capacity(rows.len());
    for row in rows {
        let role = match row.role.as_str() {
            "system" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            _ => continue,
        };
        let tool_calls: Option<Vec<ToolCall>> = row
            .tool_calls_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok());
        let content = if let Some(parts_json) = &row.content_parts_json {
            match serde_json::from_str(parts_json) {
                Ok(parts) => Some(parts),
                Err(e) => {
                    tracing::warn!("Failed to deserialize content_parts_json: {e}");
                    row.content.map(MessageContent::Text)
                }
            }
        } else {
            row.content.map(MessageContent::Text)
        };
        messages.push(Message {
            role,
            content,
            tool_calls,
            tool_call_id: row.tool_call_id,
            timestamp: row.timestamp,
        });
    }

    Ok(Some(messages))
}

pub fn list_sessions() -> Result<Vec<SessionMeta>> {
    let dir = sessions_dir()?;
    let mut sessions = Vec::new();

    if !dir.exists() {
        return Ok(sessions);
    }

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(wrapper) = serde_json::from_str::<SessionMetaOnly>(&content) {
                    sessions.push(wrapper.meta);
                }
            }
        }
    }

    // Sort by updated_at descending
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(sessions)
}
