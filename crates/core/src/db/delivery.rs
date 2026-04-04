use anyhow::Result;
use rusqlite::params;

use super::models::{DeliveryRow, NewDelivery};
use super::Database;

impl Database {
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
        {
            let mut upd = tx.prepare_cached(
                "UPDATE delivery_queue SET status = 'in_progress', updated_at = ?1 WHERE id = ?2",
            )?;
            for row in &rows {
                upd.execute(params![now, row.id])?;
            }
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
}
