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

    fn db_path() -> Result<PathBuf> {
        Config::db_path()
    }

    fn run_migrations(&self) -> Result<()> {
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

            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
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
}
