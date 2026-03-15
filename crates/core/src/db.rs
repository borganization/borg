use anyhow::{Context, Result};
use chrono::Datelike;
use rusqlite::{params, Connection};
use std::path::PathBuf;

use crate::config::Config;

/// SQLite database for structured data (session metadata, scheduled tasks, task runs).
pub struct Database {
    conn: Connection,
}

/// Session metadata row from SQLite.
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub total_tokens: i64,
    pub model: String,
    pub title: String,
}

/// Scheduled task row from SQLite.
#[derive(Debug, Clone)]
pub struct ScheduledTaskRow {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub schedule_type: String,
    pub schedule_expr: String,
    pub timezone: String,
    pub status: String,
    pub next_run: Option<i64>,
    pub created_at: i64,
}

/// Task run log row from SQLite.
#[derive(Debug, Clone)]
pub struct TaskRunRow {
    pub id: i64,
    pub task_id: String,
    pub started_at: i64,
    pub duration_ms: i64,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// Persisted message row from SQLite.
#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    pub tool_calls_json: Option<String>,
    pub tool_call_id: Option<String>,
    pub timestamp: Option<String>,
    pub created_at: i64,
}

/// Customization row from SQLite.
#[derive(Debug, Clone)]
pub struct CustomizationRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub category: String,
    pub status: String,
    pub version: String,
    pub installed_at: i64,
    pub verified_at: Option<i64>,
}

/// Parameters for creating a new scheduled task.
pub struct NewTask<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub prompt: &'a str,
    pub schedule_type: &'a str,
    pub schedule_expr: &'a str,
    pub timezone: &'a str,
    pub next_run: Option<i64>,
}

impl Database {
    /// Open (or create) the database at `~/.tamagotchi/tamagotchi.db`.
    pub fn open() -> Result<Self> {
        let path = Self::db_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn =
            Connection::open(&path).with_context(|| format!("Failed to open DB at {path:?}"))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    /// Create a Database from an existing connection. Runs migrations.
    /// Useful for testing with in-memory databases.
    pub fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    fn db_path() -> Result<PathBuf> {
        Config::db_path()
    }

    /// Current schema version. Bump this when adding new migrations.
    const CURRENT_VERSION: u32 = 5;

    fn run_migrations(&self) -> Result<()> {
        // Ensure meta table exists for version tracking
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;

        let current = self.schema_version()?;

        if current < 1 {
            self.migrate_v1()?;
        }
        if current < 2 {
            self.migrate_v2()?;
        }
        if current < 3 {
            self.migrate_v3()?;
        }
        if current < 4 {
            self.migrate_v4()?;
        }
        if current < 5 {
            self.migrate_v5()?;
        }

        self.set_meta("schema_version", &Self::CURRENT_VERSION.to_string())?;
        Ok(())
    }

    fn schema_version(&self) -> Result<u32> {
        match self.get_meta("schema_version")? {
            Some(v) => Ok(v.parse().unwrap_or(0)),
            None => {
                // Check if tables already exist (pre-versioning database)
                let mut stmt = self.conn.prepare(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='sessions'",
                )?;
                let count: i64 = stmt.query_row([], |row| row.get(0))?;
                if count > 0 {
                    Ok(1) // Legacy DB with original tables
                } else {
                    Ok(0) // Fresh database
                }
            }
        }
    }

    /// V1: Original schema — sessions, scheduled_tasks, task_runs, meta, token_usage
    fn migrate_v1(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                model TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS scheduled_tasks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                prompt TEXT NOT NULL,
                schedule_type TEXT NOT NULL,
                schedule_expr TEXT NOT NULL,
                timezone TEXT NOT NULL DEFAULT 'local',
                status TEXT NOT NULL DEFAULT 'active',
                next_run INTEGER,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL REFERENCES scheduled_tasks(id),
                started_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                result TEXT,
                error TEXT
            );

            CREATE TABLE IF NOT EXISTS token_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_token_usage_ts ON token_usage(timestamp);
            ",
        )?;
        Ok(())
    }

    /// V2: Add messages table for message persistence + retry_count for tasks
    fn migrate_v2(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                tool_calls_json TEXT,
                tool_call_id TEXT,
                timestamp TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, id);

            -- Add retry_count to scheduled_tasks if not present
            ALTER TABLE scheduled_tasks ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
            ",
            )
            .or_else(|e| {
                // ALTER TABLE fails if column already exists — that's OK
                let msg = e.to_string();
                if msg.contains("duplicate column") || msg.contains("already exists") {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;
        Ok(())
    }

    /// V3: Add channel_sessions and channel_messages tables for gateway
    fn migrate_v3(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS channel_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_name TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_active INTEGER NOT NULL,
                UNIQUE(channel_name, sender_id)
            );

            CREATE TABLE IF NOT EXISTS channel_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_name TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                direction TEXT NOT NULL,
                content TEXT,
                metadata_json TEXT,
                session_id TEXT,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_channel_messages_session
                ON channel_messages(channel_name, sender_id, created_at);
            ",
        )?;
        Ok(())
    }

    /// V4: Add customizations, installed_tools, installed_channels, customization_credentials
    fn migrate_v4(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS customizations (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                category TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'installed',
                version TEXT NOT NULL DEFAULT '1.0.0',
                installed_at INTEGER NOT NULL,
                verified_at INTEGER,
                config_json TEXT
            );

            CREATE TABLE IF NOT EXISTS installed_tools (
                name TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                runtime TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'user',
                customization_id TEXT,
                installed_at INTEGER NOT NULL,
                FOREIGN KEY(customization_id) REFERENCES customizations(id) ON DELETE SET NULL
            );

            CREATE TABLE IF NOT EXISTS installed_channels (
                name TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                runtime TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'user',
                customization_id TEXT,
                webhook_path TEXT NOT NULL,
                installed_at INTEGER NOT NULL,
                FOREIGN KEY(customization_id) REFERENCES customizations(id) ON DELETE SET NULL
            );

            CREATE TABLE IF NOT EXISTS customization_credentials (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                customization_id TEXT NOT NULL,
                credential_key TEXT NOT NULL,
                storage_type TEXT NOT NULL,
                keychain_service TEXT,
                env_var TEXT,
                FOREIGN KEY(customization_id) REFERENCES customizations(id) ON DELETE CASCADE,
                UNIQUE(customization_id, credential_key)
            );
            ",
        )?;
        Ok(())
    }

    /// V5: Add file_hashes table for integrity verification
    fn migrate_v5(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS file_hashes (
                customization_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                sha256 TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(customization_id) REFERENCES customizations(id) ON DELETE CASCADE,
                UNIQUE(customization_id, file_path)
            );
            ",
        )?;
        Ok(())
    }

    // ── Customizations ──

    pub fn insert_customization(
        &self,
        id: &str,
        name: &str,
        kind: &str,
        category: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO customizations (id, name, kind, category, status, version, installed_at)
             VALUES (?1, ?2, ?3, ?4, 'installed', '1.0.0', ?5)",
            params![id, name, kind, category, now],
        )?;
        Ok(())
    }

    pub fn delete_customization(&self, id: &str) -> Result<bool> {
        let deleted = self
            .conn
            .execute("DELETE FROM customizations WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    pub fn list_customizations(&self) -> Result<Vec<CustomizationRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind, category, status, version, installed_at, verified_at
             FROM customizations ORDER BY category, name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CustomizationRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    category: row.get(3)?,
                    status: row.get(4)?,
                    version: row.get(5)?,
                    installed_at: row.get(6)?,
                    verified_at: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_customization_verified(&self, id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "UPDATE customizations SET verified_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn insert_credential(
        &self,
        customization_id: &str,
        credential_key: &str,
        storage_type: &str,
        keychain_service: Option<&str>,
        env_var: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO customization_credentials
             (customization_id, credential_key, storage_type, keychain_service, env_var)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                customization_id,
                credential_key,
                storage_type,
                keychain_service,
                env_var
            ],
        )?;
        Ok(())
    }

    pub fn delete_credentials_for(&self, customization_id: &str) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM customization_credentials WHERE customization_id = ?1",
            params![customization_id],
        )?;
        Ok(count)
    }

    // ── File hashes (integrity) ──

    pub fn insert_file_hash(
        &self,
        customization_id: &str,
        file_path: &str,
        sha256: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO file_hashes (customization_id, file_path, sha256, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![customization_id, file_path, sha256, now],
        )?;
        Ok(())
    }

    pub fn get_file_hashes(&self, customization_id: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path, sha256 FROM file_hashes WHERE customization_id = ?1")?;
        let rows = stmt
            .query_map(params![customization_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_file_hashes(&self, customization_id: &str) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM file_hashes WHERE customization_id = ?1",
            params![customization_id],
        )?;
        Ok(count)
    }

    pub fn get_tool_customization_id(&self, tool_name: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT customization_id FROM installed_tools WHERE name = ?1")?;
        let mut rows = stmt.query_map(params![tool_name], |row| row.get::<_, Option<String>>(0))?;
        match rows.next() {
            Some(Ok(val)) => Ok(val),
            _ => Ok(None),
        }
    }

    pub fn get_channel_customization_id(&self, channel_name: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT customization_id FROM installed_channels WHERE name = ?1")?;
        let mut rows =
            stmt.query_map(params![channel_name], |row| row.get::<_, Option<String>>(0))?;
        match rows.next() {
            Some(Ok(val)) => Ok(val),
            _ => Ok(None),
        }
    }

    pub fn insert_installed_tool(
        &self,
        name: &str,
        description: &str,
        runtime: &str,
        customization_id: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO installed_tools (name, description, runtime, source, customization_id, installed_at)
             VALUES (?1, ?2, ?3, 'customization', ?4, ?5)",
            params![name, description, runtime, customization_id, now],
        )?;
        Ok(())
    }

    pub fn insert_installed_channel(
        &self,
        name: &str,
        description: &str,
        runtime: &str,
        customization_id: &str,
        webhook_path: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO installed_channels (name, description, runtime, source, customization_id, webhook_path, installed_at)
             VALUES (?1, ?2, ?3, 'customization', ?4, ?5, ?6)",
            params![name, description, runtime, customization_id, webhook_path, now],
        )?;
        Ok(())
    }

    // ── Session metadata ──

    pub fn upsert_session(
        &self,
        id: &str,
        created_at: i64,
        updated_at: i64,
        total_tokens: i64,
        model: &str,
        title: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, created_at, updated_at, total_tokens, model, title)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                updated_at = ?3, total_tokens = ?4, model = ?5, title = ?6",
            params![id, created_at, updated_at, total_tokens, model, title],
        )?;
        Ok(())
    }

    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, created_at, updated_at, total_tokens, model, title
             FROM sessions ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(SessionRow {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    updated_at: row.get(2)?,
                    total_tokens: row.get(3)?,
                    model: row.get(4)?,
                    title: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Message persistence ──

    pub fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content: Option<&str>,
        tool_calls_json: Option<&str>,
        tool_call_id: Option<&str>,
        timestamp: Option<&str>,
    ) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_calls_json, tool_call_id, timestamp, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![session_id, role, content, tool_calls_json, tool_call_id, timestamp, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn load_session_messages(&self, session_id: &str) -> Result<Vec<MessageRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, tool_calls_json, tool_call_id, timestamp, created_at
             FROM messages WHERE session_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(MessageRow {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    tool_calls_json: row.get(4)?,
                    tool_call_id: row.get(5)?,
                    timestamp: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_session_messages(&self, session_id: &str) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(count)
    }

    // ── Scheduled tasks ──

    pub fn create_task(&self, task: &NewTask<'_>) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO scheduled_tasks (id, name, prompt, schedule_type, schedule_expr, timezone, status, next_run, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8)",
            params![task.id, task.name, task.prompt, task.schedule_type, task.schedule_expr, task.timezone, task.next_run, now],
        )?;
        Ok(())
    }

    pub fn list_tasks(&self) -> Result<Vec<ScheduledTaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, prompt, schedule_type, schedule_expr, timezone, status, next_run, created_at
             FROM scheduled_tasks ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ScheduledTaskRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    prompt: row.get(2)?,
                    schedule_type: row.get(3)?,
                    schedule_expr: row.get(4)?,
                    timezone: row.get(5)?,
                    status: row.get(6)?,
                    next_run: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_due_tasks(&self, now: i64) -> Result<Vec<ScheduledTaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, prompt, schedule_type, schedule_expr, timezone, status, next_run, created_at
             FROM scheduled_tasks
             WHERE status = 'active' AND next_run IS NOT NULL AND next_run <= ?1
             ORDER BY next_run ASC",
        )?;
        let rows = stmt
            .query_map(params![now], |row| {
                Ok(ScheduledTaskRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    prompt: row.get(2)?,
                    schedule_type: row.get(3)?,
                    schedule_expr: row.get(4)?,
                    timezone: row.get(5)?,
                    status: row.get(6)?,
                    next_run: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn update_task_status(&self, id: &str, status: &str) -> Result<bool> {
        let updated = self.conn.execute(
            "UPDATE scheduled_tasks SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        Ok(updated > 0)
    }

    pub fn update_task_next_run(&self, id: &str, next_run: Option<i64>) -> Result<()> {
        self.conn.execute(
            "UPDATE scheduled_tasks SET next_run = ?1 WHERE id = ?2",
            params![next_run, id],
        )?;
        Ok(())
    }

    pub fn record_task_run(
        &self,
        task_id: &str,
        started_at: i64,
        duration_ms: i64,
        result: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO task_runs (task_id, started_at, duration_ms, result, error)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task_id, started_at, duration_ms, result, error],
        )?;
        Ok(())
    }

    pub fn task_run_history(&self, task_id: &str, limit: usize) -> Result<Vec<TaskRunRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, started_at, duration_ms, result, error
             FROM task_runs WHERE task_id = ?1 ORDER BY started_at DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![task_id, limit as i64], |row| {
                Ok(TaskRunRow {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    started_at: row.get(2)?,
                    duration_ms: row.get(3)?,
                    result: row.get(4)?,
                    error: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Channel sessions ──

    pub fn resolve_channel_session(&self, channel_name: &str, sender_id: &str) -> Result<String> {
        let now = chrono::Utc::now().timestamp();
        let mut stmt = self.conn.prepare(
            "SELECT session_id FROM channel_sessions WHERE channel_name = ?1 AND sender_id = ?2",
        )?;
        let existing: Option<String> = stmt
            .query_map(params![channel_name, sender_id], |row| row.get(0))?
            .next()
            .and_then(std::result::Result::ok);

        match existing {
            Some(session_id) => {
                self.conn.execute(
                    "UPDATE channel_sessions SET last_active = ?1 WHERE channel_name = ?2 AND sender_id = ?3",
                    params![now, channel_name, sender_id],
                )?;
                Ok(session_id)
            }
            None => {
                let session_id = uuid::Uuid::new_v4().to_string();
                self.conn.execute(
                    "INSERT INTO channel_sessions (channel_name, sender_id, session_id, created_at, last_active)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![channel_name, sender_id, session_id, now, now],
                )?;
                Ok(session_id)
            }
        }
    }

    pub fn log_channel_message(
        &self,
        channel_name: &str,
        sender_id: &str,
        direction: &str,
        content: Option<&str>,
        metadata_json: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO channel_messages (channel_name, sender_id, direction, content, metadata_json, session_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![channel_name, sender_id, direction, content, metadata_json, session_id, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    // ── Token usage ──

    pub fn log_token_usage(&self, prompt: u64, completion: u64, total: u64) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO token_usage (timestamp, prompt_tokens, completion_tokens, total_tokens)
             VALUES (?1, ?2, ?3, ?4)",
            params![now, prompt as i64, completion as i64, total as i64],
        )?;
        Ok(())
    }

    pub fn monthly_token_total(&self) -> Result<u64> {
        let now = chrono::Utc::now();
        let first_of_month = now.date_naive().with_day(1).unwrap_or(now.date_naive());
        let midnight = first_of_month
            .and_hms_opt(0, 0, 0)
            .context("failed to construct midnight timestamp")?;
        let start_ts = midnight.and_utc().timestamp();
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM token_usage WHERE timestamp >= ?1",
        )?;
        let total: i64 = stmt.query_row(params![start_ts], |row| row.get(0))?;
        Ok(total as u64)
    }

    // ── Meta key-value ──

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
        match rows.next() {
            Some(Ok(val)) => Ok(Some(val)),
            _ => Ok(None),
        }
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .expect("pragmas");
        let db = Database { conn };
        db.run_migrations().expect("migrations");
        db
    }

    #[test]
    fn create_and_list_tasks() {
        let db = test_db();
        db.create_task(&NewTask {
            id: "t1",
            name: "morning summary",
            prompt: "summarize",
            schedule_type: "cron",
            schedule_expr: "0 9 * * *",
            timezone: "local",
            next_run: Some(100),
        })
        .expect("create task");
        db.create_task(&NewTask {
            id: "t2",
            name: "stock check",
            prompt: "check stocks",
            schedule_type: "interval",
            schedule_expr: "1h",
            timezone: "local",
            next_run: Some(200),
        })
        .expect("create task 2");

        let tasks = db.list_tasks().expect("list");
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn get_due_tasks_filters_correctly() {
        let db = test_db();
        db.create_task(&NewTask {
            id: "t1",
            name: "due",
            prompt: "prompt",
            schedule_type: "cron",
            schedule_expr: "expr",
            timezone: "local",
            next_run: Some(50),
        })
        .expect("create");
        db.create_task(&NewTask {
            id: "t2",
            name: "not due",
            prompt: "prompt",
            schedule_type: "cron",
            schedule_expr: "expr",
            timezone: "local",
            next_run: Some(200),
        })
        .expect("create");
        db.create_task(&NewTask {
            id: "t3",
            name: "paused",
            prompt: "prompt",
            schedule_type: "cron",
            schedule_expr: "expr",
            timezone: "local",
            next_run: Some(50),
        })
        .expect("create");
        db.update_task_status("t3", "paused").expect("pause");

        let due = db.get_due_tasks(100).expect("due");
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "t1");
    }

    #[test]
    fn update_task_status_and_next_run() {
        let db = test_db();
        db.create_task(&NewTask {
            id: "t1",
            name: "test",
            prompt: "prompt",
            schedule_type: "cron",
            schedule_expr: "expr",
            timezone: "local",
            next_run: Some(100),
        })
        .expect("create");

        assert!(db.update_task_status("t1", "paused").expect("update"));
        let tasks = db.list_tasks().expect("list");
        assert_eq!(tasks[0].status, "paused");

        db.update_task_next_run("t1", Some(999))
            .expect("update next_run");
        let tasks = db.list_tasks().expect("list");
        assert_eq!(tasks[0].next_run, Some(999));
    }

    #[test]
    fn record_and_query_task_runs() {
        let db = test_db();
        db.create_task(&NewTask {
            id: "t1",
            name: "test",
            prompt: "prompt",
            schedule_type: "interval",
            schedule_expr: "30m",
            timezone: "local",
            next_run: Some(100),
        })
        .expect("create");
        db.record_task_run("t1", 1000, 500, Some("done"), None)
            .expect("record");
        db.record_task_run("t1", 2000, 300, None, Some("failed"))
            .expect("record");

        let runs = db.task_run_history("t1", 10).expect("history");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].started_at, 2000); // most recent first
    }

    #[test]
    fn upsert_session_metadata() {
        let db = test_db();
        db.upsert_session("s1", 100, 100, 500, "gpt-4", "Hello chat")
            .expect("upsert");
        db.upsert_session("s1", 100, 200, 1000, "gpt-4", "Hello chat updated")
            .expect("upsert again");

        let sessions = db.list_sessions(10).expect("list");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].total_tokens, 1000);
        assert_eq!(sessions[0].title, "Hello chat updated");
    }

    #[test]
    fn meta_get_set() {
        let db = test_db();
        assert!(db.get_meta("version").expect("get").is_none());

        db.set_meta("version", "1").expect("set");
        assert_eq!(db.get_meta("version").expect("get").as_deref(), Some("1"));

        db.set_meta("version", "2").expect("set again");
        assert_eq!(db.get_meta("version").expect("get").as_deref(), Some("2"));
    }

    #[test]
    fn update_nonexistent_task_returns_false() {
        let db = test_db();
        assert!(!db
            .update_task_status("nonexistent", "paused")
            .expect("update"));
    }

    #[test]
    fn log_and_query_token_usage() {
        let db = test_db();
        db.log_token_usage(100, 50, 150).expect("log usage");
        db.log_token_usage(200, 100, 300).expect("log usage 2");
        let total = db.monthly_token_total().expect("query");
        assert_eq!(total, 450);
    }

    #[test]
    fn monthly_token_total_empty_returns_zero() {
        let db = test_db();
        let total = db.monthly_token_total().expect("query");
        assert_eq!(total, 0);
    }

    #[test]
    fn monthly_token_total_excludes_old_entries() {
        let db = test_db();
        // Insert a row with a very old timestamp (year 2020)
        db.conn
            .execute(
                "INSERT INTO token_usage (timestamp, prompt_tokens, completion_tokens, total_tokens)
                 VALUES (?1, 500, 500, 1000)",
                params![1577836800_i64], // 2020-01-01
            )
            .expect("insert old");
        // Insert a current row
        db.log_token_usage(100, 50, 150).expect("log current");
        let total = db.monthly_token_total().expect("query");
        // Old entry should be excluded, only current entry counts
        assert_eq!(total, 150);
    }

    #[test]
    fn schema_version_tracking() {
        let db = test_db();
        let version = db.schema_version().expect("get version");
        assert_eq!(version, Database::CURRENT_VERSION);
    }

    #[test]
    fn insert_and_load_messages() {
        let db = test_db();
        db.insert_message(
            "s1",
            "user",
            Some("Hello"),
            None,
            None,
            Some("2026-01-01T00:00:00Z"),
        )
        .expect("insert user msg");
        db.insert_message(
            "s1",
            "assistant",
            Some("Hi there"),
            None,
            None,
            Some("2026-01-01T00:00:01Z"),
        )
        .expect("insert assistant msg");
        db.insert_message("s2", "user", Some("Other session"), None, None, None)
            .expect("insert other session msg");

        let msgs = db.load_session_messages("s1").expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content.as_deref(), Some("Hello"));
        assert_eq!(msgs[1].role, "assistant");
    }

    #[test]
    fn delete_session_messages() {
        let db = test_db();
        db.insert_message("s1", "user", Some("msg1"), None, None, None)
            .expect("insert");
        db.insert_message("s1", "user", Some("msg2"), None, None, None)
            .expect("insert");
        let deleted = db.delete_session_messages("s1").expect("delete");
        assert_eq!(deleted, 2);
        let msgs = db.load_session_messages("s1").expect("load");
        assert!(msgs.is_empty());
    }

    #[test]
    fn messages_with_tool_calls() {
        let db = test_db();
        let tc_json =
            r#"[{"id":"c1","type":"function","function":{"name":"test","arguments":"{}"}}]"#;
        db.insert_message("s1", "assistant", None, Some(tc_json), None, None)
            .expect("insert");
        let msgs = db.load_session_messages("s1").expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].tool_calls_json.as_deref(), Some(tc_json));
    }

    #[test]
    fn resolve_channel_session_creates_new() {
        let db = test_db();
        let session_id = db
            .resolve_channel_session("slack", "user1")
            .expect("resolve");
        assert!(!session_id.is_empty());
        // UUID v4 format
        assert_eq!(session_id.len(), 36);
    }

    #[test]
    fn resolve_channel_session_returns_existing() {
        let db = test_db();
        let first = db.resolve_channel_session("slack", "user1").expect("first");
        let second = db
            .resolve_channel_session("slack", "user1")
            .expect("second");
        assert_eq!(first, second);
    }

    #[test]
    fn resolve_channel_session_different_senders() {
        let db = test_db();
        let s1 = db.resolve_channel_session("slack", "alice").expect("alice");
        let s2 = db.resolve_channel_session("slack", "bob").expect("bob");
        assert_ne!(s1, s2);
    }

    #[test]
    fn log_channel_message_and_count() {
        let db = test_db();
        let id1 = db
            .log_channel_message("slack", "user1", "inbound", Some("hello"), None, None)
            .expect("log 1");
        let id2 = db
            .log_channel_message("slack", "user1", "outbound", Some("hi back"), None, None)
            .expect("log 2");
        assert!(id1 > 0);
        assert!(id2 > id1);
    }

    #[test]
    fn insert_message_with_tool_call_id() {
        let db = test_db();
        db.insert_message(
            "s1",
            "tool",
            Some("result data"),
            None,
            Some("call_abc123"),
            None,
        )
        .expect("insert");
        let msgs = db.load_session_messages("s1").expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].tool_call_id.as_deref(), Some("call_abc123"));
        assert_eq!(msgs[0].content.as_deref(), Some("result data"));
    }

    #[test]
    fn delete_messages_nonexistent_session() {
        let db = test_db();
        let deleted = db
            .delete_session_messages("no-such-session")
            .expect("delete");
        assert_eq!(deleted, 0);
    }

    #[test]
    fn insert_and_list_customizations() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        db.insert_customization("email/gmail", "Gmail", "tool", "email")
            .expect("insert");
        let list = db.list_customizations().expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "email/gmail"); // ordered by category, name
        assert_eq!(list[1].id, "messaging/telegram");
    }

    #[test]
    fn delete_customization() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        assert!(db
            .delete_customization("messaging/telegram")
            .expect("delete"));
        assert!(!db
            .delete_customization("nonexistent")
            .expect("delete missing"));
        let list = db.list_customizations().expect("list");
        assert!(list.is_empty());
    }

    #[test]
    fn set_customization_verified() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        let list = db.list_customizations().expect("list");
        assert!(list[0].verified_at.is_none());

        db.set_customization_verified("messaging/telegram")
            .expect("verify");
        let list = db.list_customizations().expect("list");
        assert!(list[0].verified_at.is_some());
    }

    #[test]
    fn insert_and_delete_credentials() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        db.insert_credential(
            "messaging/telegram",
            "TELEGRAM_BOT_TOKEN",
            "keychain",
            Some("tamagotchi-telegram"),
            None,
        )
        .expect("insert cred");
        let deleted = db
            .delete_credentials_for("messaging/telegram")
            .expect("delete");
        assert_eq!(deleted, 1);
    }

    #[test]
    fn credential_cascade_on_customization_delete() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        db.insert_credential(
            "messaging/telegram",
            "TELEGRAM_BOT_TOKEN",
            "keychain",
            Some("tamagotchi-telegram"),
            None,
        )
        .expect("insert cred");

        db.delete_customization("messaging/telegram")
            .expect("delete");
        // Credential should be cascade-deleted
        let deleted = db
            .delete_credentials_for("messaging/telegram")
            .expect("delete");
        assert_eq!(deleted, 0);
    }

    #[test]
    fn insert_customization_replaces_existing() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        db.insert_customization("messaging/telegram", "Telegram v2", "channel", "messaging")
            .expect("replace");
        let list = db.list_customizations().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Telegram v2");
    }

    #[test]
    fn migrate_v5_creates_file_hashes_table() {
        let db = test_db();
        let mut stmt = db
            .conn
            .prepare("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='file_hashes'")
            .expect("prepare");
        let count: i64 = stmt.query_row([], |row| row.get(0)).expect("query");
        assert_eq!(count, 1);
    }

    #[test]
    fn insert_and_get_file_hashes() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert cust");
        db.insert_file_hash("messaging/telegram", "telegram/channel.toml", "abc123")
            .expect("insert hash 1");
        db.insert_file_hash("messaging/telegram", "telegram/parse_inbound.py", "def456")
            .expect("insert hash 2");
        db.insert_file_hash("messaging/telegram", "telegram/send_outbound.py", "ghi789")
            .expect("insert hash 3");
        let hashes = db.get_file_hashes("messaging/telegram").expect("get");
        assert_eq!(hashes.len(), 3);
    }

    #[test]
    fn file_hashes_cascade_delete() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert cust");
        db.insert_file_hash("messaging/telegram", "telegram/channel.toml", "abc123")
            .expect("insert hash");
        db.delete_customization("messaging/telegram")
            .expect("delete cust");
        let hashes = db.get_file_hashes("messaging/telegram").expect("get");
        assert!(hashes.is_empty());
    }

    #[test]
    fn delete_file_hashes_by_customization() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert cust 1");
        db.insert_customization("email/gmail", "Gmail", "tool", "email")
            .expect("insert cust 2");
        db.insert_file_hash("messaging/telegram", "telegram/channel.toml", "abc")
            .expect("insert hash 1");
        db.insert_file_hash("email/gmail", "gmail/tool.toml", "def")
            .expect("insert hash 2");
        db.delete_file_hashes("messaging/telegram").expect("delete");
        let t_hashes = db.get_file_hashes("messaging/telegram").expect("get");
        let g_hashes = db.get_file_hashes("email/gmail").expect("get");
        assert!(t_hashes.is_empty());
        assert_eq!(g_hashes.len(), 1);
    }

    #[test]
    fn insert_installed_tool_and_get_customization_id() {
        let db = test_db();
        db.insert_customization("email/gmail", "Gmail", "tool", "email")
            .expect("insert cust");
        db.insert_installed_tool("gmail", "Gmail integration", "python", "email/gmail")
            .expect("insert tool");
        let cust_id = db.get_tool_customization_id("gmail").expect("get");
        assert_eq!(cust_id.as_deref(), Some("email/gmail"));
    }

    #[test]
    fn get_tool_customization_id_returns_none_for_unknown() {
        let db = test_db();
        let cust_id = db.get_tool_customization_id("nonexistent").expect("get");
        assert!(cust_id.is_none());
    }

    #[test]
    fn insert_installed_channel_and_get_customization_id() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert cust");
        db.insert_installed_channel(
            "telegram",
            "Telegram bot",
            "python",
            "messaging/telegram",
            "/webhook/telegram",
        )
        .expect("insert channel");
        let cust_id = db.get_channel_customization_id("telegram").expect("get");
        assert_eq!(cust_id.as_deref(), Some("messaging/telegram"));
    }

    #[test]
    fn get_channel_customization_id_returns_none_for_unknown() {
        let db = test_db();
        let cust_id = db.get_channel_customization_id("nonexistent").expect("get");
        assert!(cust_id.is_none());
    }

    #[test]
    fn file_hash_upsert_on_reinstall() {
        let db = test_db();
        db.insert_customization("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert cust");
        db.insert_file_hash("messaging/telegram", "telegram/channel.toml", "old_hash")
            .expect("insert hash");
        db.insert_file_hash("messaging/telegram", "telegram/channel.toml", "new_hash")
            .expect("upsert hash");
        let hashes = db.get_file_hashes("messaging/telegram").expect("get");
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].1, "new_hash");
    }
}
