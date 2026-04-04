use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::Database;

impl Database {
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
}
