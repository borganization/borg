use anyhow::{Context, Result};
use chrono::Datelike;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use tracing::instrument;

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
    pub max_retries: i32,
    pub retry_count: i32,
    pub retry_after: Option<i64>,
    pub last_error: Option<String>,
    pub timeout_ms: i64,
    pub delivery_channel: Option<String>,
    pub delivery_target: Option<String>,
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
    pub content_parts_json: Option<String>,
    pub tool_calls_json: Option<String>,
    pub tool_call_id: Option<String>,
    pub timestamp: Option<String>,
    pub created_at: i64,
}

/// Delivery queue row from SQLite.
#[derive(Debug, Clone)]
pub struct DeliveryRow {
    pub id: String,
    pub channel_name: String,
    pub sender_id: String,
    pub channel_id: Option<String>,
    pub session_id: Option<String>,
    pub payload_json: String,
    pub status: String,
    pub retry_count: i32,
    pub max_retries: i32,
    pub next_retry_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub error: Option<String>,
}

/// Plugin row from SQLite.
#[derive(Debug, Clone)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub category: String,
    pub status: String,
    pub version: String,
    pub installed_at: i64,
    pub verified_at: Option<i64>,
}

/// Memory embedding row from SQLite.
#[derive(Debug, Clone)]
pub struct EmbeddingRow {
    pub id: i64,
    pub scope: String,
    pub filename: String,
    pub content_hash: String,
    pub embedding: Vec<u8>,
    pub dimension: usize,
    pub model: String,
    pub created_at: i64,
}

/// Chunk row from SQLite (for chunked/FTS memory search).
#[derive(Debug, Clone)]
pub struct ChunkRow {
    pub id: i64,
    pub scope: String,
    pub filename: String,
    pub chunk_index: i64,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub content: String,
    pub content_hash: String,
    pub embedding: Option<Vec<u8>>,
    pub created_at: i64,
}

/// Input data for upserting a chunk.
#[derive(Debug, Clone)]
pub struct ChunkData {
    pub chunk_index: i64,
    pub content: String,
    pub content_hash: String,
    pub embedding: Option<Vec<u8>>,
    pub dimension: Option<usize>,
    pub model: Option<String>,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
}

/// Pairing request row from SQLite.
#[derive(Debug, Clone)]
pub struct PairingRequestRow {
    pub id: String,
    pub channel_name: String,
    pub sender_id: String,
    pub code: String,
    pub status: String,
    pub display_name: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub approved_at: Option<i64>,
}

/// Approved sender row from SQLite.
#[derive(Debug, Clone)]
pub struct ApprovedSenderRow {
    pub id: i64,
    pub channel_name: String,
    pub sender_id: String,
    pub display_name: Option<String>,
    pub approved_at: i64,
}

/// Agent role row from SQLite.
#[derive(Debug, Clone)]
pub struct AgentRoleRow {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub temperature: Option<f32>,
    pub system_instructions: Option<String>,
    pub tools_allowed: Option<String>,
    pub max_iterations: Option<i64>,
    pub is_builtin: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Sub-agent run row from SQLite.
#[derive(Debug, Clone)]
pub struct SubAgentRunRow {
    pub id: String,
    pub nickname: String,
    pub role: String,
    pub parent_session_id: String,
    pub session_id: String,
    pub depth: u32,
    pub status: String,
    pub result_text: Option<String>,
    pub error_text: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug)]
pub struct ModelUsageRow {
    pub provider: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub total_cost_usd: Option<f64>,
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
    pub max_retries: Option<i32>,
    pub timeout_ms: Option<i64>,
    pub delivery_channel: Option<&'a str>,
    pub delivery_target: Option<&'a str>,
}

/// Parameters for enqueuing a delivery.
pub struct NewDelivery<'a> {
    pub id: &'a str,
    pub channel_name: &'a str,
    pub sender_id: &'a str,
    pub channel_id: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub payload_json: &'a str,
    pub max_retries: i32,
}

/// Parameters for updating an existing scheduled task. `None` fields are left unchanged.
pub struct UpdateTask<'a> {
    pub name: Option<&'a str>,
    pub prompt: Option<&'a str>,
    pub schedule_type: Option<&'a str>,
    pub schedule_expr: Option<&'a str>,
    pub timezone: Option<&'a str>,
}

impl Database {
    /// Open (or create) the database at `~/.borg/borg.db`.
    #[instrument(skip_all)]
    pub fn open() -> Result<Self> {
        let path = Self::db_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn =
            Connection::open(&path).with_context(|| format!("Failed to open DB at {path:?}"))?;
        // WAL pragma returns a result row — use query_row to avoid execute_batch error
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    /// Create a Database from an existing connection. Runs migrations.
    /// Useful for testing with in-memory databases.
    pub fn from_connection(conn: Connection) -> Result<Self> {
        // WAL pragma returns a result row — use query_row to avoid execute_batch error
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    fn db_path() -> Result<PathBuf> {
        Config::db_path()
    }

    /// Current schema version. Bump this when adding new migrations.
    const CURRENT_VERSION: u32 = 14;

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
        if current < 6 {
            self.migrate_v6()?;
        }
        if current < 7 {
            self.migrate_v7()?;
        }
        if current < 8 {
            self.migrate_v8()?;
        }
        if current < 9 {
            self.migrate_v9()?;
        }
        if current < 10 {
            self.migrate_v10()?;
        }
        if current < 11 {
            self.migrate_v11()?;
        }
        if current < 12 {
            self.migrate_v12()?;
        }
        if current < 13 {
            self.migrate_v13()?;
        }
        if current < 14 {
            self.migrate_v14()?;
        }

        self.set_meta("schema_version", &Self::CURRENT_VERSION.to_string())?;
        Ok(())
    }

    fn schema_version(&self) -> Result<u32> {
        match self.get_meta("schema_version")? {
            Some(v) => match v.parse() {
                Ok(n) => Ok(n),
                Err(_) => {
                    tracing::warn!("Corrupted schema_version '{v}', treating as 0");
                    Ok(0)
                }
            },
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

    /// V6: Add delivery_queue table for persistent outbound delivery
    fn migrate_v6(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS delivery_queue (
                id TEXT PRIMARY KEY,
                channel_name TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                channel_id TEXT,
                session_id TEXT,
                payload_json TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                retry_count INTEGER NOT NULL DEFAULT 0,
                max_retries INTEGER NOT NULL DEFAULT 5,
                next_retry_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_delivery_queue_status
                ON delivery_queue(status, next_retry_at);
            ",
        )?;
        Ok(())
    }

    /// V7: Add agent_roles and sub_agent_runs tables for multi-agent system
    fn migrate_v7(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS agent_roles (
                name TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                model TEXT,
                provider TEXT,
                temperature REAL,
                system_instructions TEXT,
                tools_allowed TEXT,
                max_iterations INTEGER,
                is_builtin INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sub_agent_runs (
                id TEXT PRIMARY KEY,
                nickname TEXT NOT NULL,
                role TEXT NOT NULL,
                parent_session_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                depth INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending_init',
                result_text TEXT,
                error_text TEXT,
                created_at INTEGER NOT NULL,
                completed_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_sub_agent_runs_parent
                ON sub_agent_runs(parent_session_id);
            ",
        )?;
        crate::multi_agent::roles::seed_builtin_roles(self)?;
        Ok(())
    }

    /// V8: Add content_parts_json column to messages for multimodal content
    fn migrate_v8(&self) -> Result<()> {
        self.conn
            .execute_batch("ALTER TABLE messages ADD COLUMN content_parts_json TEXT;")
            .or_else(|e| {
                let msg = e.to_string();
                if msg.contains("duplicate column") || msg.contains("already exists") {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;
        Ok(())
    }

    /// V9: Add settings table for runtime configuration overrides
    fn migrate_v9(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            ",
        )?;
        Ok(())
    }

    /// V10: Add memory_embeddings table for semantic memory search
    fn migrate_v10(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS memory_embeddings (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                scope       TEXT NOT NULL DEFAULT 'global',
                filename    TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                embedding   BLOB NOT NULL,
                dimension   INTEGER NOT NULL,
                model       TEXT NOT NULL,
                created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(scope, filename)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_embeddings_scope ON memory_embeddings(scope);
            ",
        )?;
        Ok(())
    }

    fn migrate_v11(&self) -> Result<()> {
        // Add provider, model, cost_usd columns to token_usage.
        // Use try-execute to handle "duplicate column" gracefully on re-runs.
        let alters = [
            "ALTER TABLE token_usage ADD COLUMN provider TEXT",
            "ALTER TABLE token_usage ADD COLUMN model TEXT",
            "ALTER TABLE token_usage ADD COLUMN cost_usd REAL",
        ];
        for sql in &alters {
            if let Err(e) = self.conn.execute_batch(sql) {
                let msg = e.to_string();
                if !msg.contains("duplicate column") {
                    return Err(e.into());
                }
            }
        }
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_token_usage_model ON token_usage(model);",
        )?;
        Ok(())
    }

    /// V12: Add memory_chunks table and FTS index for chunked semantic search.
    fn migrate_v12(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS memory_chunks (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                scope        TEXT NOT NULL DEFAULT 'global',
                filename     TEXT NOT NULL,
                chunk_index  INTEGER NOT NULL,
                start_line   INTEGER,
                end_line     INTEGER,
                content      TEXT NOT NULL,
                content_hash TEXT NOT NULL DEFAULT '',
                embedding    BLOB,
                dimension    INTEGER,
                model        TEXT,
                created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(scope, filename, chunk_index)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_chunks_scope_file ON memory_chunks(scope, filename);

            CREATE VIRTUAL TABLE IF NOT EXISTS memory_chunks_fts USING fts5(
                scope UNINDEXED,
                filename UNINDEXED,
                chunk_index UNINDEXED,
                content,
                content='memory_chunks',
                content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS memory_chunks_ai AFTER INSERT ON memory_chunks BEGIN
                INSERT INTO memory_chunks_fts(rowid, scope, filename, chunk_index, content)
                VALUES (new.id, new.scope, new.filename, new.chunk_index, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memory_chunks_ad AFTER DELETE ON memory_chunks BEGIN
                INSERT INTO memory_chunks_fts(memory_chunks_fts, rowid, scope, filename, chunk_index, content)
                VALUES ('delete', old.id, old.scope, old.filename, old.chunk_index, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memory_chunks_au AFTER UPDATE ON memory_chunks BEGIN
                INSERT INTO memory_chunks_fts(memory_chunks_fts, rowid, scope, filename, chunk_index, content)
                VALUES ('delete', old.id, old.scope, old.filename, old.chunk_index, old.content);
                INSERT INTO memory_chunks_fts(rowid, scope, filename, chunk_index, content)
                VALUES (new.id, new.scope, new.filename, new.chunk_index, new.content);
            END;
            ",
        )?;
        Ok(())
    }

    fn migrate_v13(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS pairing_requests (
                id TEXT PRIMARY KEY,
                channel_name TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                code TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                display_name TEXT,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                approved_at INTEGER,
                UNIQUE(channel_name, code)
            );
            CREATE INDEX IF NOT EXISTS idx_pairing_channel_sender
                ON pairing_requests(channel_name, sender_id, status);

            CREATE TABLE IF NOT EXISTS approved_senders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_name TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                display_name TEXT,
                approved_at INTEGER NOT NULL,
                pairing_request_id TEXT,
                UNIQUE(channel_name, sender_id)
            );
            ",
        )?;
        Ok(())
    }

    /// V14: Add retry, timeout, and delivery columns to scheduled_tasks
    fn migrate_v14(&self) -> Result<()> {
        // retry_count already exists from V2, add remaining columns
        let alters = [
            "ALTER TABLE scheduled_tasks ADD COLUMN max_retries INTEGER NOT NULL DEFAULT 3",
            "ALTER TABLE scheduled_tasks ADD COLUMN retry_after INTEGER",
            "ALTER TABLE scheduled_tasks ADD COLUMN last_error TEXT",
            "ALTER TABLE scheduled_tasks ADD COLUMN timeout_ms INTEGER NOT NULL DEFAULT 300000",
            "ALTER TABLE scheduled_tasks ADD COLUMN delivery_channel TEXT",
            "ALTER TABLE scheduled_tasks ADD COLUMN delivery_target TEXT",
        ];
        for sql in &alters {
            self.conn.execute(sql, []).or_else(|e| {
                let msg = e.to_string();
                if msg.contains("duplicate column") || msg.contains("already exists") {
                    Ok(0)
                } else {
                    Err(e)
                }
            })?;
        }
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_tasks_retry ON scheduled_tasks(status, retry_after);",
        )?;
        Ok(())
    }

    // ── Pairing CRUD ──

    pub fn create_pairing_request(
        &self,
        channel_name: &str,
        sender_id: &str,
        code: &str,
        display_name: Option<&str>,
        ttl_secs: i64,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + ttl_secs;
        self.conn.execute(
            "INSERT INTO pairing_requests (id, channel_name, sender_id, code, status, display_name, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7)",
            params![id, channel_name, sender_id, code, display_name, now, expires_at],
        )?;
        Ok(id)
    }

    fn map_pairing_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PairingRequestRow> {
        Ok(PairingRequestRow {
            id: row.get(0)?,
            channel_name: row.get(1)?,
            sender_id: row.get(2)?,
            code: row.get(3)?,
            status: row.get(4)?,
            display_name: row.get(5)?,
            created_at: row.get(6)?,
            expires_at: row.get(7)?,
            approved_at: row.get(8)?,
        })
    }

    fn map_approved_sender_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApprovedSenderRow> {
        Ok(ApprovedSenderRow {
            id: row.get(0)?,
            channel_name: row.get(1)?,
            sender_id: row.get(2)?,
            display_name: row.get(3)?,
            approved_at: row.get(4)?,
        })
    }

    pub fn find_pending_pairing(
        &self,
        channel_name: &str,
        code: &str,
    ) -> Result<Option<PairingRequestRow>> {
        let now = chrono::Utc::now().timestamp();
        let mut stmt = self.conn.prepare(
            "SELECT id, channel_name, sender_id, code, status, display_name, created_at, expires_at, approved_at
             FROM pairing_requests
             WHERE channel_name = ?1 AND code = ?2 AND status = 'pending' AND expires_at > ?3",
        )?;
        let row = stmt
            .query_row(params![channel_name, code, now], Self::map_pairing_row)
            .optional()?;
        Ok(row)
    }

    pub fn find_pending_for_sender(
        &self,
        channel_name: &str,
        sender_id: &str,
    ) -> Result<Option<PairingRequestRow>> {
        let now = chrono::Utc::now().timestamp();
        let mut stmt = self.conn.prepare(
            "SELECT id, channel_name, sender_id, code, status, display_name, created_at, expires_at, approved_at
             FROM pairing_requests
             WHERE channel_name = ?1 AND sender_id = ?2 AND status = 'pending' AND expires_at > ?3
             ORDER BY created_at DESC LIMIT 1",
        )?;
        let row = stmt
            .query_row(params![channel_name, sender_id, now], Self::map_pairing_row)
            .optional()?;
        Ok(row)
    }

    pub fn approve_pairing(&self, channel_name: &str, code: &str) -> Result<PairingRequestRow> {
        let code = code.to_uppercase();
        let now = chrono::Utc::now().timestamp();

        let tx = self.conn.unchecked_transaction()?;

        // Find the pending request within the transaction
        let request = {
            let mut stmt = tx.prepare(
                "SELECT id, channel_name, sender_id, code, status, display_name, created_at, expires_at, approved_at
                 FROM pairing_requests
                 WHERE channel_name = ?1 AND code = ?2 AND status = 'pending' AND expires_at > ?3",
            )?;
            stmt.query_row(params![channel_name, code, now], Self::map_pairing_row)
                .optional()?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No pending pairing request found for channel '{channel_name}' with code '{code}'"
                    )
                })?
        };

        tx.execute(
            "UPDATE pairing_requests SET status = 'approved', approved_at = ?1 WHERE id = ?2",
            params![now, request.id],
        )?;

        tx.execute(
            "INSERT INTO approved_senders (channel_name, sender_id, display_name, approved_at, pairing_request_id)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(channel_name, sender_id) DO UPDATE SET
                approved_at = ?4, pairing_request_id = ?5",
            params![
                request.channel_name,
                request.sender_id,
                request.display_name,
                now,
                request.id,
            ],
        )?;

        tx.commit()?;

        Ok(PairingRequestRow {
            status: "approved".into(),
            approved_at: Some(now),
            ..request
        })
    }

    /// Remove expired pending pairing requests.
    pub fn cleanup_expired_pairings(&self) -> Result<usize> {
        let now = chrono::Utc::now().timestamp();
        let deleted = self.conn.execute(
            "DELETE FROM pairing_requests WHERE status = 'pending' AND expires_at <= ?1",
            params![now],
        )?;
        Ok(deleted)
    }

    pub fn is_sender_approved(&self, channel_name: &str, sender_id: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM approved_senders WHERE channel_name = ?1 AND sender_id = ?2",
            params![channel_name, sender_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn revoke_sender(&self, channel_name: &str, sender_id: &str) -> Result<bool> {
        let changes = self.conn.execute(
            "DELETE FROM approved_senders WHERE channel_name = ?1 AND sender_id = ?2",
            params![channel_name, sender_id],
        )?;
        Ok(changes > 0)
    }

    pub fn list_pairings(&self, channel_name: Option<&str>) -> Result<Vec<PairingRequestRow>> {
        let now = chrono::Utc::now().timestamp();
        if let Some(ch) = channel_name {
            self.list_pairings_for_channel(ch, now)
        } else {
            self.list_pairings_all(now)
        }
    }

    fn list_pairings_for_channel(&self, ch: &str, now: i64) -> Result<Vec<PairingRequestRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, channel_name, sender_id, code, status, display_name, created_at, expires_at, approved_at
             FROM pairing_requests
             WHERE channel_name = ?1 AND status = 'pending' AND expires_at > ?2
             ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map(params![ch, now], Self::map_pairing_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn list_pairings_all(&self, now: i64) -> Result<Vec<PairingRequestRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, channel_name, sender_id, code, status, display_name, created_at, expires_at, approved_at
             FROM pairing_requests
             WHERE status = 'pending' AND expires_at > ?1
             ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map(params![now], Self::map_pairing_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_approved_senders(
        &self,
        channel_name: Option<&str>,
    ) -> Result<Vec<ApprovedSenderRow>> {
        if let Some(ch) = channel_name {
            self.list_approved_senders_for_channel(ch)
        } else {
            self.list_approved_senders_all()
        }
    }

    fn list_approved_senders_for_channel(&self, ch: &str) -> Result<Vec<ApprovedSenderRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, channel_name, sender_id, display_name, approved_at
             FROM approved_senders WHERE channel_name = ?1 ORDER BY approved_at DESC",
        )?;
        let rows = stmt
            .query_map(params![ch], Self::map_approved_sender_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn list_approved_senders_all(&self) -> Result<Vec<ApprovedSenderRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, channel_name, sender_id, display_name, approved_at
             FROM approved_senders ORDER BY approved_at DESC",
        )?;
        let rows = stmt
            .query_map([], Self::map_approved_sender_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Embedding CRUD ──

    pub fn upsert_embedding(
        &self,
        scope: &str,
        filename: &str,
        content_hash: &str,
        embedding: &[u8],
        dimension: usize,
        model: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO memory_embeddings (scope, filename, content_hash, embedding, dimension, model, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(scope, filename) DO UPDATE SET
                content_hash = ?3, embedding = ?4, dimension = ?5, model = ?6, created_at = ?7",
            params![scope, filename, content_hash, embedding, dimension as i64, model, now],
        )?;
        Ok(())
    }

    pub fn get_embedding(&self, scope: &str, filename: &str) -> Result<Option<EmbeddingRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope, filename, content_hash, embedding, dimension, model, created_at
             FROM memory_embeddings WHERE scope = ?1 AND filename = ?2",
        )?;
        let row = stmt
            .query_row(params![scope, filename], |row| {
                Ok(EmbeddingRow {
                    id: row.get(0)?,
                    scope: row.get(1)?,
                    filename: row.get(2)?,
                    content_hash: row.get(3)?,
                    embedding: row.get(4)?,
                    dimension: row.get::<_, i64>(5)? as usize,
                    model: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn get_all_embeddings(&self, scope: &str) -> Result<Vec<EmbeddingRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope, filename, content_hash, embedding, dimension, model, created_at
             FROM memory_embeddings WHERE scope = ?1",
        )?;
        let rows = stmt
            .query_map(params![scope], |row| {
                Ok(EmbeddingRow {
                    id: row.get(0)?,
                    scope: row.get(1)?,
                    filename: row.get(2)?,
                    content_hash: row.get(3)?,
                    embedding: row.get(4)?,
                    dimension: row.get::<_, i64>(5)? as usize,
                    model: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_embedding(&self, scope: &str, filename: &str) -> Result<bool> {
        let count = self.conn.execute(
            "DELETE FROM memory_embeddings WHERE scope = ?1 AND filename = ?2",
            params![scope, filename],
        )?;
        Ok(count > 0)
    }

    pub fn count_embeddings(&self, scope: &str) -> Result<usize> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM memory_embeddings WHERE scope = ?1")?;
        let count: i64 = stmt.query_row(params![scope], |row| row.get(0))?;
        Ok(count as usize)
    }

    // ── Chunk CRUD ──

    /// Upsert a set of chunks for a given scope+filename, replacing any existing chunks for that file.
    pub fn upsert_chunks(&self, scope: &str, filename: &str, chunks: &[ChunkData]) -> Result<()> {
        // Delete existing chunks for this file
        self.conn.execute(
            "DELETE FROM memory_chunks WHERE scope = ?1 AND filename = ?2",
            params![scope, filename],
        )?;
        let now = chrono::Utc::now().timestamp();
        for chunk in chunks {
            self.conn.execute(
                "INSERT INTO memory_chunks
                    (scope, filename, chunk_index, start_line, end_line, content, content_hash, embedding, dimension, model, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(scope, filename, chunk_index) DO UPDATE SET
                    start_line = ?4, end_line = ?5, content = ?6, content_hash = ?7,
                    embedding = ?8, dimension = ?9, model = ?10, created_at = ?11",
                params![
                    scope,
                    filename,
                    chunk.chunk_index,
                    chunk.start_line,
                    chunk.end_line,
                    chunk.content,
                    chunk.content_hash,
                    chunk.embedding,
                    chunk.dimension.map(|d| d as i64),
                    chunk.model,
                    now
                ],
            )?;
        }
        Ok(())
    }

    /// Retrieve all chunks for a given scope.
    pub fn get_all_chunks(&self, scope: &str) -> Result<Vec<ChunkRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope, filename, chunk_index, start_line, end_line, content, content_hash, embedding, created_at
             FROM memory_chunks WHERE scope = ?1 ORDER BY filename, chunk_index",
        )?;
        let rows = stmt
            .query_map(params![scope], |row| {
                Ok(ChunkRow {
                    id: row.get(0)?,
                    scope: row.get(1)?,
                    filename: row.get(2)?,
                    chunk_index: row.get(3)?,
                    start_line: row.get(4)?,
                    end_line: row.get(5)?,
                    content: row.get(6)?,
                    content_hash: row.get(7)?,
                    embedding: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Delete all chunks for a specific file.
    pub fn delete_chunks_for_file(&self, scope: &str, filename: &str) -> Result<bool> {
        let count = self.conn.execute(
            "DELETE FROM memory_chunks WHERE scope = ?1 AND filename = ?2",
            params![scope, filename],
        )?;
        Ok(count > 0)
    }

    /// Retrieve all chunks for a specific file in a scope.
    pub fn get_chunks_for_file(&self, scope: &str, filename: &str) -> Result<Vec<ChunkRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope, filename, chunk_index, start_line, end_line, content, content_hash, embedding, created_at
             FROM memory_chunks WHERE scope = ?1 AND filename = ?2 ORDER BY chunk_index",
        )?;
        let rows = stmt
            .query_map(params![scope, filename], |row| {
                Ok(ChunkRow {
                    id: row.get(0)?,
                    scope: row.get(1)?,
                    filename: row.get(2)?,
                    chunk_index: row.get(3)?,
                    start_line: row.get(4)?,
                    end_line: row.get(5)?,
                    content: row.get(6)?,
                    content_hash: row.get(7)?,
                    embedding: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Sanitize a query string for FTS5 MATCH syntax.
    /// Wraps each word in double quotes to prevent FTS5 operator injection.
    fn sanitize_fts_query(query: &str) -> String {
        query
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Full-text search over chunk content within a scope.
    /// Returns matching (ChunkRow, bm25_score) pairs sorted by relevance, limited to `limit`.
    pub fn fts_search(
        &self,
        scope: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(ChunkRow, f32)>> {
        let sanitized = Self::sanitize_fts_query(query);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT mc.id, mc.scope, mc.filename, mc.chunk_index, mc.start_line, mc.end_line,
                    mc.content, mc.content_hash, mc.embedding, mc.created_at,
                    -bm25(memory_chunks_fts) AS score
             FROM memory_chunks_fts
             JOIN memory_chunks mc ON mc.id = memory_chunks_fts.rowid
             WHERE memory_chunks_fts MATCH ?1
               AND mc.scope = ?2
             ORDER BY score DESC
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![sanitized, scope, limit as i64], |row| {
                Ok((
                    ChunkRow {
                        id: row.get(0)?,
                        scope: row.get(1)?,
                        filename: row.get(2)?,
                        chunk_index: row.get(3)?,
                        start_line: row.get(4)?,
                        end_line: row.get(5)?,
                        content: row.get(6)?,
                        content_hash: row.get(7)?,
                        embedding: row.get(8)?,
                        created_at: row.get(9)?,
                    },
                    row.get::<_, f64>(10)? as f32,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Settings CRUD ──

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let value = stmt
            .query_row(params![key], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(value)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
            params![key, value, now],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<bool> {
        let count = self
            .conn
            .execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(count > 0)
    }

    pub fn list_settings(&self) -> Result<Vec<(String, String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value, updated_at FROM settings ORDER BY key")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Agent Roles ──

    #[allow(clippy::too_many_arguments)]
    pub fn insert_role(
        &self,
        name: &str,
        description: &str,
        model: Option<&str>,
        provider: Option<&str>,
        temperature: Option<f32>,
        system_instructions: Option<&str>,
        tools_allowed: Option<&str>,
        max_iterations: Option<i64>,
        is_builtin: bool,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO agent_roles (name, description, model, provider, temperature, system_instructions, tools_allowed, max_iterations, is_builtin, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![name, description, model, provider, temperature, system_instructions, tools_allowed, max_iterations, is_builtin as i32, now],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_role(
        &self,
        name: &str,
        description: Option<&str>,
        model: Option<&str>,
        provider: Option<&str>,
        temperature: Option<f32>,
        system_instructions: Option<&str>,
        tools_allowed: Option<&str>,
        max_iterations: Option<i64>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "UPDATE agent_roles SET description = COALESCE(?2, description), model = COALESCE(?3, model), provider = COALESCE(?4, provider), temperature = COALESCE(?5, temperature), system_instructions = COALESCE(?6, system_instructions), tools_allowed = COALESCE(?7, tools_allowed), max_iterations = COALESCE(?8, max_iterations), updated_at = ?1 WHERE name = ?9",
            params![now, description, model, provider, temperature, system_instructions, tools_allowed, max_iterations, name],
        )?;
        Ok(())
    }

    pub fn delete_role(&self, name: &str) -> Result<bool> {
        let count = self
            .conn
            .execute("DELETE FROM agent_roles WHERE name = ?1", params![name])?;
        Ok(count > 0)
    }

    pub fn get_role(&self, name: &str) -> Result<Option<AgentRoleRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, description, model, provider, temperature, system_instructions, tools_allowed, max_iterations, is_builtin, created_at, updated_at FROM agent_roles WHERE name = ?1",
        )?;
        let row = stmt
            .query_row(params![name], |row| {
                Ok(AgentRoleRow {
                    name: row.get(0)?,
                    description: row.get(1)?,
                    model: row.get(2)?,
                    provider: row.get(3)?,
                    temperature: row.get(4)?,
                    system_instructions: row.get(5)?,
                    tools_allowed: row.get(6)?,
                    max_iterations: row.get(7)?,
                    is_builtin: row.get::<_, i32>(8)? != 0,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })
            .optional()?;
        Ok(row)
    }

    pub fn list_roles(&self) -> Result<Vec<AgentRoleRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, description, model, provider, temperature, system_instructions, tools_allowed, max_iterations, is_builtin, created_at, updated_at FROM agent_roles ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(AgentRoleRow {
                    name: row.get(0)?,
                    description: row.get(1)?,
                    model: row.get(2)?,
                    provider: row.get(3)?,
                    temperature: row.get(4)?,
                    system_instructions: row.get(5)?,
                    tools_allowed: row.get(6)?,
                    max_iterations: row.get(7)?,
                    is_builtin: row.get::<_, i32>(8)? != 0,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── Sub-Agent Runs ──

    pub fn insert_sub_agent_run(
        &self,
        id: &str,
        nickname: &str,
        role: &str,
        parent_session_id: &str,
        session_id: &str,
        depth: u32,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO sub_agent_runs (id, nickname, role, parent_session_id, session_id, depth, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending_init', ?7)",
            params![id, nickname, role, parent_session_id, session_id, depth, now],
        )?;
        Ok(())
    }

    pub fn update_sub_agent_status(
        &self,
        id: &str,
        status: &str,
        result_text: Option<&str>,
        error_text: Option<&str>,
    ) -> Result<()> {
        let completed_at = if status == "completed" || status == "errored" || status == "shutdown" {
            Some(chrono::Utc::now().timestamp())
        } else {
            None
        };
        self.conn.execute(
            "UPDATE sub_agent_runs SET status = ?2, result_text = ?3, error_text = ?4, completed_at = ?5 WHERE id = ?1",
            params![id, status, result_text, error_text, completed_at],
        )?;
        Ok(())
    }

    pub fn list_sub_agent_runs(&self, parent_session_id: &str) -> Result<Vec<SubAgentRunRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, nickname, role, parent_session_id, session_id, depth, status, result_text, error_text, created_at, completed_at FROM sub_agent_runs WHERE parent_session_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map(params![parent_session_id], |row| {
                Ok(SubAgentRunRow {
                    id: row.get(0)?,
                    nickname: row.get(1)?,
                    role: row.get(2)?,
                    parent_session_id: row.get(3)?,
                    session_id: row.get(4)?,
                    depth: row.get(5)?,
                    status: row.get(6)?,
                    result_text: row.get(7)?,
                    error_text: row.get(8)?,
                    created_at: row.get(9)?,
                    completed_at: row.get(10)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_sub_agent_run(&self, id: &str) -> Result<Option<SubAgentRunRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, nickname, role, parent_session_id, session_id, depth, status, result_text, error_text, created_at, completed_at FROM sub_agent_runs WHERE id = ?1",
        )?;
        let row = stmt
            .query_row(params![id], |row| {
                Ok(SubAgentRunRow {
                    id: row.get(0)?,
                    nickname: row.get(1)?,
                    role: row.get(2)?,
                    parent_session_id: row.get(3)?,
                    session_id: row.get(4)?,
                    depth: row.get(5)?,
                    status: row.get(6)?,
                    result_text: row.get(7)?,
                    error_text: row.get(8)?,
                    created_at: row.get(9)?,
                    completed_at: row.get(10)?,
                })
            })
            .optional()?;
        Ok(row)
    }

    // ── Delivery Queue ──

    pub fn enqueue_delivery(&self, d: &NewDelivery<'_>) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO delivery_queue (id, channel_name, sender_id, channel_id, session_id, payload_json, status, retry_count, max_retries, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 0, ?7, ?8, ?8)",
            params![d.id, d.channel_name, d.sender_id, d.channel_id, d.session_id, d.payload_json, d.max_retries, now],
        )?;
        Ok(())
    }

    pub fn claim_pending_deliveries(&mut self, limit: u32) -> Result<Vec<DeliveryRow>> {
        let now = chrono::Utc::now().timestamp();
        let tx = self.conn.transaction()?;

        let mut stmt = tx.prepare(
            "SELECT id, channel_name, sender_id, channel_id, session_id, payload_json, status, retry_count, max_retries, next_retry_at, created_at, updated_at, error
             FROM delivery_queue
             WHERE status = 'pending' AND (next_retry_at IS NULL OR next_retry_at <= ?1)
             ORDER BY created_at ASC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![now, limit], |row| {
                Ok(DeliveryRow {
                    id: row.get(0)?,
                    channel_name: row.get(1)?,
                    sender_id: row.get(2)?,
                    channel_id: row.get(3)?,
                    session_id: row.get(4)?,
                    payload_json: row.get(5)?,
                    status: row.get(6)?,
                    retry_count: row.get(7)?,
                    max_retries: row.get(8)?,
                    next_retry_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    error: row.get(12)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);

        // Mark claimed rows as in_progress
        for row in &rows {
            tx.execute(
                "UPDATE delivery_queue SET status = 'in_progress', updated_at = ?1 WHERE id = ?2",
                params![now, row.id],
            )?;
        }

        tx.commit()?;
        Ok(rows)
    }

    pub fn mark_delivered(&self, id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "UPDATE delivery_queue SET status = 'delivered', updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn mark_failed(&self, id: &str, error: &str, next_retry_at: Option<i64>) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "UPDATE delivery_queue SET status = CASE WHEN retry_count + 1 >= max_retries THEN 'exhausted' ELSE 'pending' END, retry_count = retry_count + 1, error = ?1, next_retry_at = ?2, updated_at = ?3 WHERE id = ?4",
            params![error, next_retry_at, now, id],
        )?;
        Ok(())
    }

    pub fn replay_unfinished(&self) -> Result<u32> {
        let now = chrono::Utc::now().timestamp();
        let count = self.conn.execute(
            "UPDATE delivery_queue SET status = 'pending', updated_at = ?1 WHERE status = 'in_progress'",
            params![now],
        )?;
        Ok(count as u32)
    }

    // ── Plugins ──

    pub fn insert_plugin(&self, id: &str, name: &str, kind: &str, category: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO customizations (id, name, kind, category, status, version, installed_at)
             VALUES (?1, ?2, ?3, ?4, 'installed', '1.0.0', ?5)",
            params![id, name, kind, category, now],
        )?;
        Ok(())
    }

    pub fn delete_plugin(&self, id: &str) -> Result<bool> {
        let deleted = self
            .conn
            .execute("DELETE FROM customizations WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    pub fn list_plugins(&self) -> Result<Vec<PluginRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind, category, status, version, installed_at, verified_at
             FROM customizations ORDER BY category, name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PluginRow {
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

    pub fn set_plugin_verified(&self, id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "UPDATE customizations SET verified_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn insert_credential(
        &self,
        plugin_id: &str,
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
                plugin_id,
                credential_key,
                storage_type,
                keychain_service,
                env_var
            ],
        )?;
        Ok(())
    }

    pub fn delete_credentials_for(&self, plugin_id: &str) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM customization_credentials WHERE customization_id = ?1",
            params![plugin_id],
        )?;
        Ok(count)
    }

    // ── File hashes (integrity) ──

    pub fn insert_file_hash(&self, plugin_id: &str, file_path: &str, sha256: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO file_hashes (customization_id, file_path, sha256, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![plugin_id, file_path, sha256, now],
        )?;
        Ok(())
    }

    pub fn get_file_hashes(&self, plugin_id: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path, sha256 FROM file_hashes WHERE customization_id = ?1")?;
        let rows = stmt
            .query_map(params![plugin_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_file_hashes(&self, plugin_id: &str) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM file_hashes WHERE customization_id = ?1",
            params![plugin_id],
        )?;
        Ok(count)
    }

    pub fn get_tool_plugin_id(&self, tool_name: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT customization_id FROM installed_tools WHERE name = ?1")?;
        let mut rows = stmt.query_map(params![tool_name], |row| row.get::<_, Option<String>>(0))?;
        match rows.next() {
            Some(Ok(val)) => Ok(val),
            _ => Ok(None),
        }
    }

    pub fn get_channel_plugin_id(&self, channel_name: &str) -> Result<Option<String>> {
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
        plugin_id: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO installed_tools (name, description, runtime, source, customization_id, installed_at)
             VALUES (?1, ?2, ?3, 'plugin', ?4, ?5)",
            params![name, description, runtime, plugin_id, now],
        )?;
        Ok(())
    }

    pub fn insert_installed_channel(
        &self,
        name: &str,
        description: &str,
        runtime: &str,
        plugin_id: &str,
        webhook_path: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO installed_channels (name, description, runtime, source, customization_id, webhook_path, installed_at)
             VALUES (?1, ?2, ?3, 'plugin', ?4, ?5, ?6)",
            params![name, description, runtime, plugin_id, webhook_path, now],
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

    #[instrument(skip_all)]
    #[allow(clippy::too_many_arguments)]
    pub fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content: Option<&str>,
        tool_calls_json: Option<&str>,
        tool_call_id: Option<&str>,
        timestamp: Option<&str>,
        content_parts_json: Option<&str>,
    ) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_calls_json, tool_call_id, timestamp, created_at, content_parts_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![session_id, role, content, tool_calls_json, tool_call_id, timestamp, now, content_parts_json],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn load_session_messages(&self, session_id: &str) -> Result<Vec<MessageRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, tool_calls_json, tool_call_id, timestamp, created_at, content_parts_json
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
                    content_parts_json: row.get(8)?,
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
        let max_retries = task.max_retries.unwrap_or(3);
        let timeout_ms = task.timeout_ms.unwrap_or(300_000);
        self.conn.execute(
            "INSERT INTO scheduled_tasks (id, name, prompt, schedule_type, schedule_expr, timezone, status, next_run, created_at, max_retries, timeout_ms, delivery_channel, delivery_target)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?9, ?10, ?11, ?12)",
            params![task.id, task.name, task.prompt, task.schedule_type, task.schedule_expr, task.timezone, task.next_run, now, max_retries, timeout_ms, task.delivery_channel, task.delivery_target],
        )?;
        Ok(())
    }

    pub fn list_tasks(&self) -> Result<Vec<ScheduledTaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, prompt, schedule_type, schedule_expr, timezone, status, next_run, created_at,
                    max_retries, retry_count, retry_after, last_error, timeout_ms, delivery_channel, delivery_target
             FROM scheduled_tasks ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], Self::map_task_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_due_tasks(&self, now: i64) -> Result<Vec<ScheduledTaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, prompt, schedule_type, schedule_expr, timezone, status, next_run, created_at,
                    max_retries, retry_count, retry_after, last_error, timeout_ms, delivery_channel, delivery_target
             FROM scheduled_tasks
             WHERE status = 'active' AND next_run IS NOT NULL AND next_run <= ?1
               AND retry_after IS NULL
             ORDER BY next_run ASC",
        )?;
        let rows = stmt
            .query_map(params![now], Self::map_task_row)?
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

    pub fn get_task_by_id(&self, id: &str) -> Result<Option<ScheduledTaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, prompt, schedule_type, schedule_expr, timezone, status, next_run, created_at,
                    max_retries, retry_count, retry_after, last_error, timeout_ms, delivery_channel, delivery_target
             FROM scheduled_tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], Self::map_task_row)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    fn map_task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledTaskRow> {
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
            max_retries: row.get(9)?,
            retry_count: row.get(10)?,
            retry_after: row.get(11)?,
            last_error: row.get(12)?,
            timeout_ms: row.get(13)?,
            delivery_channel: row.get(14)?,
            delivery_target: row.get(15)?,
        })
    }

    pub fn set_task_retry(
        &self,
        task_id: &str,
        retry_count: i32,
        last_error: &str,
        retry_after: i64,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE scheduled_tasks SET retry_count = ?1, last_error = ?2, retry_after = ?3 WHERE id = ?4",
            params![retry_count, last_error, retry_after, task_id],
        )?;
        Ok(())
    }

    pub fn clear_task_retry(&self, task_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE scheduled_tasks SET retry_count = 0, retry_after = NULL, last_error = NULL WHERE id = ?1",
            params![task_id],
        )?;
        Ok(())
    }

    pub fn get_tasks_pending_retry(&self, now: i64) -> Result<Vec<ScheduledTaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, prompt, schedule_type, schedule_expr, timezone, status, next_run, created_at,
                    max_retries, retry_count, retry_after, last_error, timeout_ms, delivery_channel, delivery_target
             FROM scheduled_tasks
             WHERE status = 'active' AND retry_after IS NOT NULL AND retry_after <= ?1
             ORDER BY retry_after ASC",
        )?;
        let rows = stmt
            .query_map(params![now], Self::map_task_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_task(&self, id: &str) -> Result<bool> {
        self.conn
            .execute("DELETE FROM task_runs WHERE task_id = ?1", params![id])?;
        let deleted = self
            .conn
            .execute("DELETE FROM scheduled_tasks WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    pub fn update_task(&self, id: &str, update: &UpdateTask<'_>) -> Result<bool> {
        let existing = match self.get_task_by_id(id)? {
            Some(row) => row,
            None => return Ok(false),
        };

        let name = update.name.unwrap_or(&existing.name);
        let prompt = update.prompt.unwrap_or(&existing.prompt);
        let schedule_type = update.schedule_type.unwrap_or(&existing.schedule_type);
        let schedule_expr = update.schedule_expr.unwrap_or(&existing.schedule_expr);
        let timezone = update.timezone.unwrap_or(&existing.timezone);

        let next_run = if update.schedule_type.is_some() || update.schedule_expr.is_some() {
            crate::tasks::calculate_next_run(schedule_type, schedule_expr)?
        } else {
            existing.next_run
        };

        self.conn.execute(
            "UPDATE scheduled_tasks SET name = ?1, prompt = ?2, schedule_type = ?3, schedule_expr = ?4, timezone = ?5, next_run = ?6 WHERE id = ?7",
            params![name, prompt, schedule_type, schedule_expr, timezone, next_run, id],
        )?;
        Ok(true)
    }

    pub fn last_task_run(&self, task_id: &str) -> Result<Option<TaskRunRow>> {
        let runs = self.task_run_history(task_id, 1)?;
        Ok(runs.into_iter().next())
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

    /// Update session_id for a channel session (used by /new command).
    pub fn update_channel_session_id(
        &self,
        channel_name: &str,
        sender_id: &str,
        new_session_id: &str,
    ) -> Result<bool> {
        let now = chrono::Utc::now().timestamp();
        let updated = self.conn.execute(
            "UPDATE channel_sessions SET session_id = ?1, last_active = ?2 WHERE channel_name = ?3 AND sender_id = ?4",
            params![new_session_id, now, channel_name, sender_id],
        )?;
        Ok(updated > 0)
    }

    /// Count messages in a session.
    pub fn count_session_messages(&self, session_id: &str) -> Result<usize> {
        let mut stmt = self
            .conn
            .prepare("SELECT COUNT(*) FROM messages WHERE session_id = ?1")?;
        let count: i64 = stmt.query_row(params![session_id], |row| row.get(0))?;
        Ok(count as usize)
    }

    // ── Token usage ──

    fn month_start_ts() -> Result<i64> {
        let now = chrono::Utc::now();
        let first_of_month = now.date_naive().with_day(1).unwrap_or(now.date_naive());
        let midnight = first_of_month
            .and_hms_opt(0, 0, 0)
            .context("failed to construct midnight timestamp")?;
        Ok(midnight.and_utc().timestamp())
    }

    pub fn log_token_usage(
        &self,
        prompt: u64,
        completion: u64,
        total: u64,
        provider: &str,
        model: &str,
        cost_usd: Option<f64>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO token_usage (timestamp, prompt_tokens, completion_tokens, total_tokens, provider, model, cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![now, prompt as i64, completion as i64, total as i64, provider, model, cost_usd],
        )?;
        Ok(())
    }

    #[instrument(skip_all)]
    pub fn monthly_token_total(&self) -> Result<u64> {
        let start_ts = Self::month_start_ts()?;
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM token_usage WHERE timestamp >= ?1",
        )?;
        let total: i64 = stmt.query_row(params![start_ts], |row| row.get(0))?;
        Ok(total as u64)
    }

    #[instrument(skip_all)]
    pub fn monthly_total_cost(&self) -> Result<Option<f64>> {
        let start_ts = Self::month_start_ts()?;
        let mut stmt = self
            .conn
            .prepare("SELECT SUM(cost_usd) FROM token_usage WHERE timestamp >= ?1")?;
        let cost: Option<f64> = stmt.query_row(params![start_ts], |row| row.get(0))?;
        Ok(cost)
    }

    #[instrument(skip_all)]
    pub fn monthly_usage_by_model(&self) -> Result<Vec<ModelUsageRow>> {
        let start_ts = Self::month_start_ts()?;
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(provider, '') as provider, COALESCE(model, '') as model,
                    COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0),
                    COALESCE(SUM(total_tokens), 0), SUM(cost_usd)
             FROM token_usage WHERE timestamp >= ?1
             GROUP BY provider, model
             ORDER BY SUM(total_tokens) DESC",
        )?;
        let rows = stmt
            .query_map(params![start_ts], |row| {
                Ok(ModelUsageRow {
                    provider: row.get(0)?,
                    model: row.get(1)?,
                    prompt_tokens: row.get::<_, i64>(2)? as u64,
                    completion_tokens: row.get::<_, i64>(3)? as u64,
                    total_tokens: row.get::<_, i64>(4)? as u64,
                    total_cost_usd: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
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
        // WAL mode returns a result row, so use query_row instead of execute_batch
        let _: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .expect("journal_mode pragma");
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .expect("foreign_keys pragma");
        let db = Database { conn };
        db.run_migrations().expect("migrations");
        db
    }

    fn simple_task<'a>(
        id: &'a str,
        name: &'a str,
        prompt: &'a str,
        schedule_type: &'a str,
        schedule_expr: &'a str,
        next_run: Option<i64>,
    ) -> NewTask<'a> {
        NewTask {
            id,
            name,
            prompt,
            schedule_type,
            schedule_expr,
            timezone: "local",
            next_run,
            max_retries: None,
            timeout_ms: None,
            delivery_channel: None,
            delivery_target: None,
        }
    }

    #[test]
    fn create_and_list_tasks() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "morning summary",
            "summarize",
            "cron",
            "0 9 * * *",
            Some(100),
        ))
        .expect("create task");
        db.create_task(&simple_task(
            "t2",
            "stock check",
            "check stocks",
            "interval",
            "1h",
            Some(200),
        ))
        .expect("create task 2");

        let tasks = db.list_tasks().expect("list");
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn get_due_tasks_filters_correctly() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "due",
            "prompt",
            "cron",
            "expr",
            Some(50),
        ))
        .expect("create");
        db.create_task(&simple_task(
            "t2",
            "not due",
            "prompt",
            "cron",
            "expr",
            Some(200),
        ))
        .expect("create");
        db.create_task(&simple_task(
            "t3",
            "paused",
            "prompt",
            "cron",
            "expr",
            Some(50),
        ))
        .expect("create");
        db.update_task_status("t3", "paused").expect("pause");

        let due = db.get_due_tasks(100).expect("due");
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "t1");
    }

    #[test]
    fn update_task_status_and_next_run() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "test",
            "prompt",
            "cron",
            "expr",
            Some(100),
        ))
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
        db.create_task(&simple_task(
            "t1",
            "test",
            "prompt",
            "interval",
            "30m",
            Some(100),
        ))
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
    fn get_task_by_id_found() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "test",
            "prompt",
            "interval",
            "30m",
            Some(100),
        ))
        .expect("create");
        let task = db.get_task_by_id("t1").expect("get").expect("some");
        assert_eq!(task.name, "test");
        assert_eq!(task.schedule_expr, "30m");
    }

    #[test]
    fn get_task_by_id_not_found() {
        let db = test_db();
        assert!(db.get_task_by_id("nope").expect("get").is_none());
    }

    #[test]
    fn delete_task_removes_task_and_runs() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "test",
            "prompt",
            "interval",
            "30m",
            Some(100),
        ))
        .expect("create");
        db.record_task_run("t1", 1000, 500, Some("done"), None)
            .expect("record");

        assert!(db.delete_task("t1").expect("delete"));
        assert!(db.get_task_by_id("t1").expect("get").is_none());
        assert!(db.task_run_history("t1", 10).expect("history").is_empty());
    }

    #[test]
    fn delete_nonexistent_task_returns_false() {
        let db = test_db();
        assert!(!db.delete_task("nope").expect("delete"));
    }

    #[test]
    fn update_task_fields() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "old name",
            "old prompt",
            "interval",
            "30m",
            Some(100),
        ))
        .expect("create");

        let update = UpdateTask {
            name: Some("new name"),
            prompt: None,
            schedule_type: None,
            schedule_expr: Some("1h"),
            timezone: None,
        };
        assert!(db.update_task("t1", &update).expect("update"));

        let task = db.get_task_by_id("t1").expect("get").expect("some");
        assert_eq!(task.name, "new name");
        assert_eq!(task.prompt, "old prompt");
        assert_eq!(task.schedule_expr, "1h");
    }

    #[test]
    fn update_task_not_found() {
        let db = test_db();
        let update = UpdateTask {
            name: Some("x"),
            prompt: None,
            schedule_type: None,
            schedule_expr: None,
            timezone: None,
        };
        assert!(!db.update_task("nope", &update).expect("update"));
    }

    #[test]
    fn last_task_run_returns_most_recent() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "test",
            "prompt",
            "interval",
            "30m",
            Some(100),
        ))
        .expect("create");
        db.record_task_run("t1", 1000, 500, Some("first"), None)
            .expect("record");
        db.record_task_run("t1", 2000, 300, Some("second"), None)
            .expect("record");

        let run = db.last_task_run("t1").expect("last").expect("some");
        assert_eq!(run.started_at, 2000);
        assert_eq!(run.result.as_deref(), Some("second"));
    }

    #[test]
    fn last_task_run_none_when_no_runs() {
        let db = test_db();
        assert!(db.last_task_run("t1").expect("last").is_none());
    }

    #[test]
    fn log_and_query_token_usage() {
        let db = test_db();
        db.log_token_usage(100, 50, 150, "openai", "gpt-4", None)
            .expect("log usage");
        db.log_token_usage(200, 100, 300, "openai", "gpt-4", None)
            .expect("log usage 2");
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
        db.log_token_usage(100, 50, 150, "openai", "gpt-4", None)
            .expect("log current");
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
            None,
        )
        .expect("insert user msg");
        db.insert_message(
            "s1",
            "assistant",
            Some("Hi there"),
            None,
            None,
            Some("2026-01-01T00:00:01Z"),
            None,
        )
        .expect("insert assistant msg");
        db.insert_message("s2", "user", Some("Other session"), None, None, None, None)
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
        db.insert_message("s1", "user", Some("msg1"), None, None, None, None)
            .expect("insert");
        db.insert_message("s1", "user", Some("msg2"), None, None, None, None)
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
        db.insert_message("s1", "assistant", None, Some(tc_json), None, None, None)
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
    fn update_channel_session_id_works() {
        let db = test_db();
        let old_id = db.resolve_channel_session("tg", "u1").expect("resolve");
        let updated = db
            .update_channel_session_id("tg", "u1", "new-session-id")
            .expect("update");
        assert!(updated);
        let current = db.resolve_channel_session("tg", "u1").expect("resolve2");
        assert_eq!(current, "new-session-id");
        assert_ne!(current, old_id);
    }

    #[test]
    fn update_channel_session_id_no_row() {
        let db = test_db();
        let updated = db
            .update_channel_session_id("tg", "nobody", "new-id")
            .expect("update");
        assert!(!updated);
    }

    #[test]
    fn count_session_messages_works() {
        let db = test_db();
        assert_eq!(db.count_session_messages("s1").expect("count"), 0);
        db.insert_message("s1", "user", Some("hi"), None, None, None, None)
            .expect("insert");
        db.insert_message("s1", "assistant", Some("hello"), None, None, None, None)
            .expect("insert");
        assert_eq!(db.count_session_messages("s1").expect("count"), 2);
        assert_eq!(db.count_session_messages("s2").expect("count"), 0);
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
    fn insert_and_list_plugins() {
        let db = test_db();
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        db.insert_plugin("email/gmail", "Gmail", "tool", "email")
            .expect("insert");
        let list = db.list_plugins().expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "email/gmail"); // ordered by category, name
        assert_eq!(list[1].id, "messaging/telegram");
    }

    #[test]
    fn delete_plugin() {
        let db = test_db();
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        assert!(db.delete_plugin("messaging/telegram").expect("delete"));
        assert!(!db.delete_plugin("nonexistent").expect("delete missing"));
        let list = db.list_plugins().expect("list");
        assert!(list.is_empty());
    }

    #[test]
    fn set_plugin_verified() {
        let db = test_db();
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        let list = db.list_plugins().expect("list");
        assert!(list[0].verified_at.is_none());

        db.set_plugin_verified("messaging/telegram")
            .expect("verify");
        let list = db.list_plugins().expect("list");
        assert!(list[0].verified_at.is_some());
    }

    #[test]
    fn insert_and_delete_credentials() {
        let db = test_db();
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        db.insert_credential(
            "messaging/telegram",
            "TELEGRAM_BOT_TOKEN",
            "keychain",
            Some("borg-telegram"),
            None,
        )
        .expect("insert cred");
        let deleted = db
            .delete_credentials_for("messaging/telegram")
            .expect("delete");
        assert_eq!(deleted, 1);
    }

    #[test]
    fn credential_cascade_on_plugin_delete() {
        let db = test_db();
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        db.insert_credential(
            "messaging/telegram",
            "TELEGRAM_BOT_TOKEN",
            "keychain",
            Some("borg-telegram"),
            None,
        )
        .expect("insert cred");

        db.delete_plugin("messaging/telegram").expect("delete");
        // Credential should be cascade-deleted
        let deleted = db
            .delete_credentials_for("messaging/telegram")
            .expect("delete");
        assert_eq!(deleted, 0);
    }

    #[test]
    fn insert_plugin_replaces_existing() {
        let db = test_db();
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert");
        db.insert_plugin("messaging/telegram", "Telegram v2", "channel", "messaging")
            .expect("replace");
        let list = db.list_plugins().expect("list");
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
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
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
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert cust");
        db.insert_file_hash("messaging/telegram", "telegram/channel.toml", "abc123")
            .expect("insert hash");
        db.delete_plugin("messaging/telegram").expect("delete cust");
        let hashes = db.get_file_hashes("messaging/telegram").expect("get");
        assert!(hashes.is_empty());
    }

    #[test]
    fn delete_file_hashes_by_plugin() {
        let db = test_db();
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert cust 1");
        db.insert_plugin("email/gmail", "Gmail", "tool", "email")
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
    fn insert_installed_tool_and_get_plugin_id() {
        let db = test_db();
        db.insert_plugin("email/gmail", "Gmail", "tool", "email")
            .expect("insert cust");
        db.insert_installed_tool("gmail", "Gmail integration", "python", "email/gmail")
            .expect("insert tool");
        let cust_id = db.get_tool_plugin_id("gmail").expect("get");
        assert_eq!(cust_id.as_deref(), Some("email/gmail"));
    }

    #[test]
    fn get_tool_plugin_id_returns_none_for_unknown() {
        let db = test_db();
        let cust_id = db.get_tool_plugin_id("nonexistent").expect("get");
        assert!(cust_id.is_none());
    }

    #[test]
    fn insert_installed_channel_and_get_plugin_id() {
        let db = test_db();
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert cust");
        db.insert_installed_channel(
            "telegram",
            "Telegram bot",
            "python",
            "messaging/telegram",
            "/webhook/telegram",
        )
        .expect("insert channel");
        let cust_id = db.get_channel_plugin_id("telegram").expect("get");
        assert_eq!(cust_id.as_deref(), Some("messaging/telegram"));
    }

    #[test]
    fn get_channel_plugin_id_returns_none_for_unknown() {
        let db = test_db();
        let cust_id = db.get_channel_plugin_id("nonexistent").expect("get");
        assert!(cust_id.is_none());
    }

    #[test]
    fn file_hash_upsert_on_reinstall() {
        let db = test_db();
        db.insert_plugin("messaging/telegram", "Telegram", "channel", "messaging")
            .expect("insert cust");
        db.insert_file_hash("messaging/telegram", "telegram/channel.toml", "old_hash")
            .expect("insert hash");
        db.insert_file_hash("messaging/telegram", "telegram/channel.toml", "new_hash")
            .expect("upsert hash");
        let hashes = db.get_file_hashes("messaging/telegram").expect("get");
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].1, "new_hash");
    }

    #[test]
    fn migrate_v6_creates_delivery_queue() {
        let db = test_db();
        let version: String = db.get_meta("schema_version").unwrap().unwrap_or_default();
        assert_eq!(version, Database::CURRENT_VERSION.to_string());

        // Table should exist
        let count: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='delivery_queue'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    fn new_delivery<'a>(
        id: &'a str,
        channel_name: &'a str,
        sender_id: &'a str,
        channel_id: Option<&'a str>,
        payload: &'a str,
        max_retries: i32,
    ) -> NewDelivery<'a> {
        NewDelivery {
            id,
            channel_name,
            sender_id,
            channel_id,
            session_id: None,
            payload_json: payload,
            max_retries,
        }
    }

    #[test]
    fn delivery_queue_enqueue_and_claim() {
        let mut db = test_db();
        db.enqueue_delivery(&new_delivery(
            "d1",
            "slack",
            "user1",
            Some("C123"),
            r#"{"text":"hi"}"#,
            3,
        ))
        .unwrap();
        db.enqueue_delivery(&new_delivery(
            "d2",
            "slack",
            "user2",
            None,
            r#"{"text":"bye"}"#,
            3,
        ))
        .unwrap();

        let claimed = db.claim_pending_deliveries(10).unwrap();
        assert_eq!(claimed.len(), 2);
        assert_eq!(claimed[0].id, "d1");
        assert_eq!(claimed[0].channel_name, "slack");
    }

    #[test]
    fn delivery_queue_mark_delivered() {
        let mut db = test_db();
        db.enqueue_delivery(&new_delivery("d1", "slack", "user1", None, "{}", 3))
            .unwrap();
        let claimed = db.claim_pending_deliveries(10).unwrap();
        assert_eq!(claimed.len(), 1);

        db.mark_delivered("d1").unwrap();

        // Should not be claimable again
        let claimed2 = db.claim_pending_deliveries(10).unwrap();
        assert!(claimed2.is_empty());
    }

    #[test]
    fn delivery_queue_mark_failed_with_retry() {
        let mut db = test_db();
        db.enqueue_delivery(&new_delivery("d1", "slack", "user1", None, "{}", 3))
            .unwrap();
        let _ = db.claim_pending_deliveries(10).unwrap();

        let future = chrono::Utc::now().timestamp() + 60;
        db.mark_failed("d1", "timeout", Some(future)).unwrap();

        // Should not be claimable yet (next_retry_at is in the future)
        let claimed = db.claim_pending_deliveries(10).unwrap();
        assert!(claimed.is_empty());
    }

    #[test]
    fn mark_failed_no_next_retry_immediately_reclaimable() {
        let mut db = test_db();
        db.enqueue_delivery(&new_delivery("d1", "slack", "user1", None, "{}", 3))
            .unwrap();
        let _ = db.claim_pending_deliveries(10).unwrap();

        // Mark failed with no next_retry_at (None) -> immediately reclaimable
        db.mark_failed("d1", "transient error", None).unwrap();

        let claimed = db.claim_pending_deliveries(10).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, "d1");
    }

    #[test]
    fn claim_pending_deliveries_respects_limit() {
        let mut db = test_db();
        db.enqueue_delivery(&new_delivery("d1", "slack", "u1", None, "{}", 3))
            .unwrap();
        db.enqueue_delivery(&new_delivery("d2", "slack", "u2", None, "{}", 3))
            .unwrap();
        db.enqueue_delivery(&new_delivery("d3", "slack", "u3", None, "{}", 3))
            .unwrap();

        let claimed = db.claim_pending_deliveries(2).unwrap();
        assert_eq!(claimed.len(), 2);
    }

    #[test]
    fn load_session_messages_unknown_session_empty() {
        let db = test_db();
        let msgs = db
            .load_session_messages("nonexistent-session-id")
            .expect("load");
        assert!(msgs.is_empty());
    }

    #[test]
    fn list_sessions_ordered_by_most_recent() {
        let db = test_db();
        db.upsert_session("s1", 100, 100, 500, "gpt-4", "First")
            .expect("upsert");
        db.upsert_session("s2", 200, 300, 1000, "gpt-4", "Second")
            .expect("upsert");
        db.upsert_session("s3", 150, 200, 750, "gpt-4", "Third")
            .expect("upsert");

        let sessions = db.list_sessions(10).expect("list");
        assert_eq!(sessions.len(), 3);
        // Most recently updated first
        assert_eq!(sessions[0].id, "s2"); // updated_at = 300
        assert_eq!(sessions[1].id, "s3"); // updated_at = 200
        assert_eq!(sessions[2].id, "s1"); // updated_at = 100
    }

    #[test]
    fn insert_credential_round_trip() {
        let db = test_db();
        db.insert_plugin("email/gmail", "Gmail", "tool", "email")
            .expect("insert cust");
        db.insert_credential(
            "email/gmail",
            "GMAIL_TOKEN",
            "env",
            None,
            Some("GMAIL_TOKEN"),
        )
        .expect("insert cred 1");
        db.insert_credential(
            "email/gmail",
            "GMAIL_SECRET",
            "env",
            None,
            Some("GMAIL_SECRET"),
        )
        .expect("insert cred 2");
        let deleted = db.delete_credentials_for("email/gmail").expect("delete");
        assert_eq!(deleted, 2);
    }

    #[test]
    fn delivery_queue_replay_unfinished() {
        let mut db = test_db();
        db.enqueue_delivery(&new_delivery("d1", "slack", "user1", None, "{}", 3))
            .unwrap();
        let _ = db.claim_pending_deliveries(10).unwrap();

        // d1 is now in_progress
        let reset = db.replay_unfinished().unwrap();
        assert_eq!(reset, 1);

        // Should be claimable again
        let claimed = db.claim_pending_deliveries(10).unwrap();
        assert_eq!(claimed.len(), 1);
    }

    #[test]
    fn v10_migration_creates_embeddings_table() {
        let db = test_db();
        let version = db.get_meta("schema_version").unwrap().unwrap();
        assert_eq!(version, Database::CURRENT_VERSION.to_string());
        // Table should exist
        let count: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='memory_embeddings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn upsert_and_get_embedding() {
        let db = test_db();
        let embedding = vec![1.0f32, 2.0, 3.0];
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        db.upsert_embedding(
            "global",
            "notes.md",
            "hash123",
            &bytes,
            3,
            "text-embedding-3-small",
        )
        .unwrap();

        let row = db.get_embedding("global", "notes.md").unwrap().unwrap();
        assert_eq!(row.filename, "notes.md");
        assert_eq!(row.scope, "global");
        assert_eq!(row.content_hash, "hash123");
        assert_eq!(row.dimension, 3);
        assert_eq!(row.model, "text-embedding-3-small");
        assert_eq!(row.embedding, bytes);
    }

    #[test]
    fn upsert_embedding_updates_on_conflict() {
        let db = test_db();
        let bytes1 = vec![0u8; 12];
        let bytes2 = vec![1u8; 12];

        db.upsert_embedding("global", "notes.md", "hash1", &bytes1, 3, "model-a")
            .unwrap();
        db.upsert_embedding("global", "notes.md", "hash2", &bytes2, 3, "model-b")
            .unwrap();

        let row = db.get_embedding("global", "notes.md").unwrap().unwrap();
        assert_eq!(row.content_hash, "hash2");
        assert_eq!(row.embedding, bytes2);
        assert_eq!(row.model, "model-b");

        // Should still be only one row
        assert_eq!(db.count_embeddings("global").unwrap(), 1);
    }

    #[test]
    fn get_all_embeddings_filters_by_scope() {
        let db = test_db();
        let bytes = vec![0u8; 12];

        db.upsert_embedding("global", "a.md", "h1", &bytes, 3, "m")
            .unwrap();
        db.upsert_embedding("global", "b.md", "h2", &bytes, 3, "m")
            .unwrap();
        db.upsert_embedding("local", "c.md", "h3", &bytes, 3, "m")
            .unwrap();

        let global = db.get_all_embeddings("global").unwrap();
        assert_eq!(global.len(), 2);

        let local = db.get_all_embeddings("local").unwrap();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].filename, "c.md");
    }

    #[test]
    fn delete_embedding_works() {
        let db = test_db();
        let bytes = vec![0u8; 12];

        db.upsert_embedding("global", "notes.md", "h1", &bytes, 3, "m")
            .unwrap();
        assert_eq!(db.count_embeddings("global").unwrap(), 1);

        let deleted = db.delete_embedding("global", "notes.md").unwrap();
        assert!(deleted);
        assert_eq!(db.count_embeddings("global").unwrap(), 0);

        // Deleting again returns false
        let deleted = db.delete_embedding("global", "notes.md").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn get_embedding_returns_none_for_missing() {
        let db = test_db();
        let result = db.get_embedding("global", "nonexistent.md").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn count_embeddings_empty() {
        let db = test_db();
        assert_eq!(db.count_embeddings("global").unwrap(), 0);
    }

    #[test]
    fn monthly_usage_by_model_groups_correctly() {
        let db = test_db();
        db.log_token_usage(
            100,
            50,
            150,
            "openrouter",
            "anthropic/claude-sonnet-4",
            Some(0.00105),
        )
        .expect("log");
        db.log_token_usage(
            200,
            100,
            300,
            "openrouter",
            "anthropic/claude-sonnet-4",
            Some(0.0021),
        )
        .expect("log");
        db.log_token_usage(500, 200, 700, "openai", "gpt-4o", Some(0.00325))
            .expect("log");

        let rows = db.monthly_usage_by_model().expect("query");
        assert_eq!(rows.len(), 2);
        // Ordered by total_tokens DESC
        assert_eq!(rows[0].model, "gpt-4o");
        assert_eq!(rows[0].total_tokens, 700);
        assert_eq!(rows[1].model, "anthropic/claude-sonnet-4");
        assert_eq!(rows[1].total_tokens, 450);
        assert_eq!(rows[1].prompt_tokens, 300);
        assert_eq!(rows[1].completion_tokens, 150);
    }

    #[test]
    fn monthly_total_cost_sums_correctly() {
        let db = test_db();
        db.log_token_usage(100, 50, 150, "openai", "gpt-4o", Some(0.001))
            .expect("log");
        db.log_token_usage(200, 100, 300, "openai", "gpt-4o", Some(0.002))
            .expect("log");

        let cost = db.monthly_total_cost().expect("query");
        assert!((cost.unwrap() - 0.003).abs() < 1e-9);
    }

    #[test]
    fn old_rows_without_provider_handled() {
        let db = test_db();
        // Simulate pre-V11 row with no provider/model/cost
        db.conn
            .execute(
                "INSERT INTO token_usage (timestamp, prompt_tokens, completion_tokens, total_tokens)
                 VALUES (?1, 100, 50, 150)",
                params![chrono::Utc::now().timestamp()],
            )
            .expect("insert old-style");
        db.log_token_usage(200, 100, 300, "openai", "gpt-4o", Some(0.002))
            .expect("log new");

        let rows = db.monthly_usage_by_model().expect("query");
        assert_eq!(rows.len(), 2);
        // One row with empty provider/model (old), one with real values
        let old_row = rows.iter().find(|r| r.model.is_empty());
        assert!(old_row.is_some());
        assert_eq!(old_row.unwrap().total_tokens, 150);
    }

    #[test]
    fn migrate_v12_creates_memory_chunks() {
        let db = test_db();
        let version = db.schema_version().expect("get version");
        assert_eq!(version, Database::CURRENT_VERSION);
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM memory_chunks", [], |r| r.get(0))
            .expect("memory_chunks table should exist");
        assert_eq!(count, 0);
    }

    #[test]
    fn migrate_v12_creates_fts_table() {
        let db = test_db();
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM memory_chunks_fts", [], |r| r.get(0))
            .expect("FTS table should exist");
        assert_eq!(count, 0);
    }

    #[test]
    fn upsert_and_get_chunks() {
        let db = test_db();
        let chunks = vec![
            ChunkData {
                chunk_index: 0,
                content: "First chunk about Rust programming".into(),
                content_hash: "hash0".into(),
                embedding: Some(vec![0u8; 12]),
                dimension: Some(3),
                model: Some("test-model".into()),
                start_line: Some(1),
                end_line: Some(10),
            },
            ChunkData {
                chunk_index: 1,
                content: "Second chunk about memory systems".into(),
                content_hash: "hash1".into(),
                embedding: None,
                dimension: None,
                model: None,
                start_line: Some(11),
                end_line: Some(20),
            },
        ];
        db.upsert_chunks("global", "notes.md", &chunks)
            .expect("upsert");
        let loaded = db.get_all_chunks("global").expect("get all");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].filename, "notes.md");
        assert_eq!(loaded[0].chunk_index, 0);
        assert_eq!(loaded[1].chunk_index, 1);
    }

    #[test]
    fn upsert_chunks_replaces_existing() {
        let db = test_db();
        let chunks_v1 = vec![ChunkData {
            chunk_index: 0,
            content: "Old content".into(),
            content_hash: "old_hash".into(),
            embedding: None,
            dimension: None,
            model: None,
            start_line: Some(1),
            end_line: Some(5),
        }];
        db.upsert_chunks("global", "notes.md", &chunks_v1)
            .expect("v1");

        let chunks_v2 = vec![ChunkData {
            chunk_index: 0,
            content: "New content".into(),
            content_hash: "new_hash".into(),
            embedding: None,
            dimension: None,
            model: None,
            start_line: Some(1),
            end_line: Some(8),
        }];
        db.upsert_chunks("global", "notes.md", &chunks_v2)
            .expect("v2");

        let loaded = db.get_all_chunks("global").expect("get");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "New content");
    }

    #[test]
    fn fts_search_returns_matching_chunks() {
        let db = test_db();
        let chunks = vec![
            ChunkData {
                chunk_index: 0,
                content: "The quick brown fox jumps over the lazy dog".into(),
                content_hash: "h0".into(),
                embedding: None,
                dimension: None,
                model: None,
                start_line: Some(1),
                end_line: Some(1),
            },
            ChunkData {
                chunk_index: 1,
                content: "Rust programming language is fast and safe".into(),
                content_hash: "h1".into(),
                embedding: None,
                dimension: None,
                model: None,
                start_line: Some(2),
                end_line: Some(2),
            },
        ];
        db.upsert_chunks("global", "test.md", &chunks)
            .expect("upsert");

        let results = db.fts_search("global", "fox", 10).expect("fts search");
        assert_eq!(results.len(), 1);
        assert!(results[0].0.content.contains("fox"));

        let results2 = db
            .fts_search("global", "Rust programming", 10)
            .expect("fts");
        assert_eq!(results2.len(), 1);
        assert!(results2[0].0.content.contains("Rust"));
    }

    #[test]
    fn fts_search_no_results() {
        let db = test_db();
        let chunks = vec![ChunkData {
            chunk_index: 0,
            content: "Hello world".into(),
            content_hash: "h".into(),
            embedding: None,
            dimension: None,
            model: None,
            start_line: Some(1),
            end_line: Some(1),
        }];
        db.upsert_chunks("global", "test.md", &chunks)
            .expect("upsert");
        let results = db.fts_search("global", "nonexistent", 10).expect("fts");
        assert!(results.is_empty());
    }

    #[test]
    fn delete_chunks_for_file_works() {
        let db = test_db();
        let chunks = vec![ChunkData {
            chunk_index: 0,
            content: "content".into(),
            content_hash: "h".into(),
            embedding: None,
            dimension: None,
            model: None,
            start_line: Some(1),
            end_line: Some(1),
        }];
        db.upsert_chunks("global", "a.md", &chunks)
            .expect("upsert a");
        db.upsert_chunks("global", "b.md", &chunks)
            .expect("upsert b");
        assert_eq!(db.get_all_chunks("global").unwrap().len(), 2);

        db.delete_chunks_for_file("global", "a.md").expect("delete");
        let remaining = db.get_all_chunks("global").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].filename, "b.md");
    }

    #[test]
    fn chunks_scoped_isolation() {
        let db = test_db();
        let chunk = vec![ChunkData {
            chunk_index: 0,
            content: "scoped content".into(),
            content_hash: "h".into(),
            embedding: None,
            dimension: None,
            model: None,
            start_line: Some(1),
            end_line: Some(1),
        }];
        db.upsert_chunks("global", "g.md", &chunk).expect("global");
        db.upsert_chunks("local", "l.md", &chunk).expect("local");

        assert_eq!(db.get_all_chunks("global").unwrap().len(), 1);
        assert_eq!(db.get_all_chunks("local").unwrap().len(), 1);
    }

    #[test]
    fn fts_triggers_stay_in_sync_after_upsert() {
        let db = test_db();
        let v1 = vec![ChunkData {
            chunk_index: 0,
            content: "alpha beta gamma".into(),
            content_hash: "h1".into(),
            embedding: None,
            dimension: None,
            model: None,
            start_line: Some(1),
            end_line: Some(1),
        }];
        db.upsert_chunks("global", "test.md", &v1).expect("v1");
        assert_eq!(db.fts_search("global", "alpha", 10).unwrap().len(), 1);

        let v2 = vec![ChunkData {
            chunk_index: 0,
            content: "delta epsilon zeta".into(),
            content_hash: "h2".into(),
            embedding: None,
            dimension: None,
            model: None,
            start_line: Some(1),
            end_line: Some(1),
        }];
        db.upsert_chunks("global", "test.md", &v2).expect("v2");

        assert!(db.fts_search("global", "alpha", 10).unwrap().is_empty());
        assert_eq!(db.fts_search("global", "delta", 10).unwrap().len(), 1);
    }

    // ── Pairing tests ──

    #[test]
    fn migrate_v13_creates_pairing_tables() {
        let db = test_db();
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM pairing_requests", [], |r| r.get(0))
            .expect("pairing_requests table should exist");
        assert_eq!(count, 0);
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM approved_senders", [], |r| r.get(0))
            .expect("approved_senders table should exist");
        assert_eq!(count, 0);
    }

    #[test]
    fn create_and_find_pairing_request() {
        let db = test_db();
        let id = db
            .create_pairing_request("telegram", "user123", "ABCD1234", None, 3600)
            .expect("create");
        assert!(!id.is_empty());

        let found = db
            .find_pending_pairing("telegram", "ABCD1234")
            .expect("find")
            .expect("should exist");
        assert_eq!(found.channel_name, "telegram");
        assert_eq!(found.sender_id, "user123");
        assert_eq!(found.code, "ABCD1234");
        assert_eq!(found.status, "pending");

        // Not found for wrong channel
        assert!(db
            .find_pending_pairing("slack", "ABCD1234")
            .expect("find")
            .is_none());
    }

    #[test]
    fn find_pending_for_sender_reuses_code() {
        let db = test_db();
        db.create_pairing_request("telegram", "user123", "CODE1111", None, 3600)
            .expect("create");

        let found = db
            .find_pending_for_sender("telegram", "user123")
            .expect("find")
            .expect("should exist");
        assert_eq!(found.code, "CODE1111");
    }

    #[test]
    fn approve_pairing() {
        let db = test_db();
        db.create_pairing_request("telegram", "user456", "WXYZ9876", None, 3600)
            .expect("create");

        let approved = db.approve_pairing("telegram", "WXYZ9876").expect("approve");
        assert_eq!(approved.sender_id, "user456");

        // Sender should now be approved
        assert!(db.is_sender_approved("telegram", "user456").expect("check"));

        // Pending request should be gone
        assert!(db
            .find_pending_pairing("telegram", "WXYZ9876")
            .expect("find")
            .is_none());
    }

    #[test]
    fn approve_nonexistent_code_errors() {
        let db = test_db();
        let result = db.approve_pairing("telegram", "NOCODE");
        assert!(result.is_err());
    }

    #[test]
    fn is_sender_approved_false_by_default() {
        let db = test_db();
        assert!(!db.is_sender_approved("telegram", "nobody").expect("check"));
    }

    #[test]
    fn revoke_sender() {
        let db = test_db();
        db.create_pairing_request("telegram", "user789", "REVO1234", None, 3600)
            .expect("create");
        db.approve_pairing("telegram", "REVO1234").expect("approve");
        assert!(db.is_sender_approved("telegram", "user789").expect("check"));

        assert!(db.revoke_sender("telegram", "user789").expect("revoke"));
        assert!(!db.is_sender_approved("telegram", "user789").expect("check"));

        // Revoking again returns false
        assert!(!db.revoke_sender("telegram", "user789").expect("revoke"));
    }

    #[test]
    fn list_pairings_filters_by_channel() {
        let db = test_db();
        db.create_pairing_request("telegram", "u1", "CODE0001", None, 3600)
            .expect("create");
        db.create_pairing_request("slack", "u2", "CODE0002", None, 3600)
            .expect("create");

        let all = db.list_pairings(None).expect("list");
        assert_eq!(all.len(), 2);

        let tg = db.list_pairings(Some("telegram")).expect("list");
        assert_eq!(tg.len(), 1);
        assert_eq!(tg[0].channel_name, "telegram");

        let sl = db.list_pairings(Some("slack")).expect("list");
        assert_eq!(sl.len(), 1);
        assert_eq!(sl[0].channel_name, "slack");
    }

    #[test]
    fn list_approved_senders_filters_by_channel() {
        let db = test_db();
        db.create_pairing_request("telegram", "u1", "APPR0001", None, 3600)
            .expect("create");
        db.create_pairing_request("slack", "u2", "APPR0002", None, 3600)
            .expect("create");
        db.approve_pairing("telegram", "APPR0001").expect("approve");
        db.approve_pairing("slack", "APPR0002").expect("approve");

        let all = db.list_approved_senders(None).expect("list");
        assert_eq!(all.len(), 2);

        let tg = db.list_approved_senders(Some("telegram")).expect("list");
        assert_eq!(tg.len(), 1);
        assert_eq!(tg[0].sender_id, "u1");
    }

    #[test]
    fn expired_pairing_not_found() {
        let db = test_db();
        // Create with TTL of 0 — immediately expired
        db.create_pairing_request("telegram", "user_exp", "EXPR1234", None, 0)
            .expect("create");

        // Should not be findable
        assert!(db
            .find_pending_pairing("telegram", "EXPR1234")
            .expect("find")
            .is_none());
        assert!(db
            .find_pending_for_sender("telegram", "user_exp")
            .expect("find")
            .is_none());

        // Cannot approve expired code
        assert!(db.approve_pairing("telegram", "EXPR1234").is_err());
    }

    #[test]
    fn approve_pairing_case_insensitive() {
        let db = test_db();
        db.create_pairing_request("telegram", "user_ci", "ABCD5678", None, 3600)
            .expect("create");

        // Approve with lowercase — should still work
        let approved = db.approve_pairing("telegram", "abcd5678").expect("approve");
        assert_eq!(approved.sender_id, "user_ci");
        assert_eq!(approved.status, "approved");
        assert!(approved.approved_at.is_some());
    }

    #[test]
    fn approve_pairing_returns_updated_status() {
        let db = test_db();
        db.create_pairing_request("telegram", "user_st", "STAT1234", None, 3600)
            .expect("create");

        let approved = db.approve_pairing("telegram", "STAT1234").expect("approve");
        assert_eq!(approved.status, "approved");
        assert!(approved.approved_at.is_some());
    }

    #[test]
    fn cleanup_expired_pairings() {
        let db = test_db();
        // Create one expired (TTL=0) and one valid
        db.create_pairing_request("telegram", "u_exp", "EXP00001", None, 0)
            .expect("create");
        db.create_pairing_request("telegram", "u_valid", "VAL00001", None, 3600)
            .expect("create");

        let cleaned = db.cleanup_expired_pairings().expect("cleanup");
        assert_eq!(cleaned, 1);

        // Valid one should still be findable
        assert!(db
            .find_pending_for_sender("telegram", "u_valid")
            .expect("find")
            .is_some());
    }

    #[test]
    fn duplicate_sender_approval_is_idempotent() {
        let db = test_db();
        db.create_pairing_request("telegram", "u_dup", "DUP00001", None, 3600)
            .expect("create first");
        db.approve_pairing("telegram", "DUP00001")
            .expect("approve first");
        assert!(db.is_sender_approved("telegram", "u_dup").expect("check"));

        // Create a second request and approve it — should update, not duplicate
        db.create_pairing_request("telegram", "u_dup", "DUP00002", None, 3600)
            .expect("create second");
        db.approve_pairing("telegram", "DUP00002")
            .expect("approve second");

        // Still only one approved sender row
        let senders = db.list_approved_senders(Some("telegram")).expect("list");
        let matching: Vec<_> = senders.iter().filter(|s| s.sender_id == "u_dup").collect();
        assert_eq!(matching.len(), 1);
    }

    // ── V14 scheduled task retry/delivery tests ──

    #[test]
    fn migrate_v14_adds_task_columns() {
        let db = test_db();
        let version = db.get_meta("schema_version").unwrap().unwrap();
        assert_eq!(version, Database::CURRENT_VERSION.to_string());

        // Create a task and verify new columns have defaults
        db.create_task(&simple_task(
            "t1",
            "test",
            "prompt",
            "interval",
            "30m",
            Some(100),
        ))
        .expect("create");
        let task = db.get_task_by_id("t1").expect("get").expect("some");
        assert_eq!(task.max_retries, 3);
        assert_eq!(task.retry_count, 0);
        assert!(task.retry_after.is_none());
        assert!(task.last_error.is_none());
        assert_eq!(task.timeout_ms, 300_000);
        assert!(task.delivery_channel.is_none());
        assert!(task.delivery_target.is_none());
    }

    #[test]
    fn create_task_with_delivery_config() {
        let db = test_db();
        db.create_task(&NewTask {
            id: "t1",
            name: "notify task",
            prompt: "do stuff",
            schedule_type: "interval",
            schedule_expr: "1h",
            timezone: "local",
            next_run: Some(100),
            max_retries: Some(5),
            timeout_ms: Some(60_000),
            delivery_channel: Some("telegram"),
            delivery_target: Some("12345"),
        })
        .expect("create");
        let task = db.get_task_by_id("t1").expect("get").expect("some");
        assert_eq!(task.max_retries, 5);
        assert_eq!(task.timeout_ms, 60_000);
        assert_eq!(task.delivery_channel.as_deref(), Some("telegram"));
        assert_eq!(task.delivery_target.as_deref(), Some("12345"));
    }

    #[test]
    fn set_and_clear_task_retry() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "test",
            "prompt",
            "interval",
            "30m",
            Some(100),
        ))
        .expect("create");

        db.set_task_retry("t1", 2, "connection timeout", 9999)
            .expect("set retry");
        let task = db.get_task_by_id("t1").expect("get").expect("some");
        assert_eq!(task.retry_count, 2);
        assert_eq!(task.retry_after, Some(9999));
        assert_eq!(task.last_error.as_deref(), Some("connection timeout"));

        db.clear_task_retry("t1").expect("clear");
        let task = db.get_task_by_id("t1").expect("get").expect("some");
        assert_eq!(task.retry_count, 0);
        assert!(task.retry_after.is_none());
        assert!(task.last_error.is_none());
    }

    #[test]
    fn get_tasks_pending_retry() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "retry-me",
            "prompt",
            "interval",
            "30m",
            Some(100),
        ))
        .expect("create");
        db.create_task(&simple_task(
            "t2",
            "not-retry",
            "prompt",
            "interval",
            "30m",
            Some(100),
        ))
        .expect("create");

        db.set_task_retry("t1", 1, "timeout", 50).expect("set");

        // t1 has retry_after=50, query with now=60 should find it
        let pending = db.get_tasks_pending_retry(60).expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "t1");

        // query with now=40 should find nothing (not yet due)
        let pending = db.get_tasks_pending_retry(40).expect("pending");
        assert!(pending.is_empty());
    }

    #[test]
    fn get_due_tasks_excludes_retry_pending() {
        let db = test_db();
        db.create_task(&simple_task(
            "t1",
            "normal",
            "prompt",
            "interval",
            "30m",
            Some(50),
        ))
        .expect("create");
        db.create_task(&simple_task(
            "t2",
            "retrying",
            "prompt",
            "interval",
            "30m",
            Some(50),
        ))
        .expect("create");

        // t2 is pending retry — should not appear in get_due_tasks
        db.set_task_retry("t2", 1, "error", 9999).expect("set");

        let due = db.get_due_tasks(100).expect("due");
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "t1");
    }
}
