use anyhow::Result;
use rusqlite::params;

use super::models::ActivityEntry;
use super::Database;

impl Database {
    /// Insert an activity log entry with the current unix timestamp.
    pub fn log_activity(
        &self,
        level: &str,
        category: &str,
        message: &str,
        detail: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO activity_log (level, category, message, detail, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![level, category, message, detail, now],
        )?;
        Ok(())
    }

    /// Query activity log entries, ordered by created_at DESC.
    ///
    /// `min_level` filters by level hierarchy: error > warn > info > debug.
    /// If `None`, defaults to info+ (error, warn, info).
    /// `category` optionally filters to a single category.
    pub fn query_activity(
        &self,
        count: usize,
        min_level: Option<&str>,
        category: Option<&str>,
    ) -> Result<Vec<ActivityEntry>> {
        let levels = match min_level {
            Some("error") => vec!["error"],
            Some("warn") => vec!["error", "warn"],
            Some("info") | None => vec!["error", "warn", "info"],
            Some("debug") | Some("all") => vec!["error", "warn", "info", "debug"],
            Some(_) => vec!["error", "warn", "info"],
        };

        let placeholders: Vec<String> = (1..=levels.len()).map(|i| format!("?{i}")).collect();
        let level_clause = format!("level IN ({})", placeholders.join(", "));

        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(cat) = category {
                let param_idx = levels.len() + 1;
                let count_idx = levels.len() + 2;
                (
                    format!(
                        "SELECT id, level, category, message, detail, created_at
                     FROM activity_log
                     WHERE {level_clause} AND category = ?{param_idx}
                     ORDER BY created_at DESC
                     LIMIT ?{count_idx}"
                    ),
                    {
                        let mut p: Vec<Box<dyn rusqlite::types::ToSql>> = levels
                            .iter()
                            .map(|l| Box::new(l.to_string()) as _)
                            .collect();
                        p.push(Box::new(cat.to_string()));
                        p.push(Box::new(count as i64));
                        p
                    },
                )
            } else {
                let count_idx = levels.len() + 1;
                (
                    format!(
                        "SELECT id, level, category, message, detail, created_at
                     FROM activity_log
                     WHERE {level_clause}
                     ORDER BY created_at DESC
                     LIMIT ?{count_idx}"
                    ),
                    {
                        let mut p: Vec<Box<dyn rusqlite::types::ToSql>> = levels
                            .iter()
                            .map(|l| Box::new(l.to_string()) as _)
                            .collect();
                        p.push(Box::new(count as i64));
                        p
                    },
                )
            };

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| &**p).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(ActivityEntry {
                id: row.get(0)?,
                level: row.get(1)?,
                category: row.get(2)?,
                message: row.get(3)?,
                detail: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Delete activity log entries older than the given cutoff timestamp.
    /// Returns the number of deleted rows.
    pub fn prune_activity_before(&self, cutoff_timestamp: i64) -> Result<usize> {
        let deleted = self.conn.execute(
            "DELETE FROM activity_log WHERE created_at < ?1",
            params![cutoff_timestamp],
        )?;
        Ok(deleted)
    }
}
