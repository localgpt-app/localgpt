//! Durable outbound message queue with retry and exponential backoff.
//!
//! Persists outbound messages to SQLite before sending. On send failure, queues
//! for retry with exponential backoff. On daemon startup, replays pending messages.

use anyhow::{Result, anyhow};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};
use uuid::Uuid;

/// Exponential backoff schedule in milliseconds: [5s, 25s, 2m, 10m].
const BACKOFF_MS: &[u64] = &[5_000, 25_000, 120_000, 600_000];

/// Maximum retry attempts before giving up.
const DEFAULT_MAX_ATTEMPTS: i64 = 5;

/// An outbox entry representing a queued outbound message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub id: String,
    pub channel: String,
    pub target: String,
    pub payload: String,
    pub session_id: Option<String>,
    pub attempts: i64,
    pub max_attempts: i64,
    pub last_error: Option<String>,
    pub next_retry_at: i64,
    pub enqueued_at: i64,
    pub delivered_at: Option<i64>,
    pub permanent_failure: bool,
}

/// SQLite-backed durable outbox for outbound messages.
#[derive(Clone)]
pub struct Outbox {
    conn: Arc<Mutex<Connection>>,
    max_attempts: i64,
}

impl Outbox {
    /// Open or create an outbox using the given SQLite database path.
    pub fn new(db_path: &Path) -> Result<Self> {
        Self::new_with_max_attempts(db_path, DEFAULT_MAX_ATTEMPTS)
    }

    pub fn new_with_max_attempts(db_path: &Path, max_attempts: i64) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            max_attempts,
        })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS outbox (
                id TEXT PRIMARY KEY,
                channel TEXT NOT NULL,
                target TEXT NOT NULL,
                payload TEXT NOT NULL,
                session_id TEXT,
                attempts INTEGER NOT NULL DEFAULT 0,
                max_attempts INTEGER NOT NULL DEFAULT 5,
                last_error TEXT,
                next_retry_at INTEGER NOT NULL,
                enqueued_at INTEGER NOT NULL,
                delivered_at INTEGER,
                permanent_failure INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_outbox_pending ON outbox(next_retry_at)
                WHERE delivered_at IS NULL AND permanent_failure = 0;
            "#,
        )?;
        Ok(())
    }

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    /// Enqueue a message for delivery. Returns the entry ID.
    pub fn enqueue(
        &self,
        channel: &str,
        target: &str,
        payload: &str,
        session_id: Option<&str>,
    ) -> Result<String> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let id = Uuid::new_v4().to_string();
        let now = Self::now_ms();

        conn.execute(
            "INSERT INTO outbox (id, channel, target, payload, session_id, max_attempts, next_retry_at, enqueued_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, channel, target, payload, session_id, self.max_attempts, 0i64, now],
        )?;

        debug!("Outbox enqueued: {} → {}:{}", id, channel, target);
        Ok(id)
    }

    /// Claim the next message ready for delivery (next_retry_at <= now).
    pub fn claim_next(&self) -> Result<Option<OutboxEntry>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let now = Self::now_ms();

        let entry = conn.query_row(
            "SELECT id, channel, target, payload, session_id, attempts, max_attempts, last_error, next_retry_at, enqueued_at, delivered_at, permanent_failure
             FROM outbox
             WHERE delivered_at IS NULL AND permanent_failure = 0 AND attempts < max_attempts AND next_retry_at <= ?1
             ORDER BY next_retry_at ASC
             LIMIT 1",
            params![now],
            |row| {
                Ok(OutboxEntry {
                    id: row.get(0)?,
                    channel: row.get(1)?,
                    target: row.get(2)?,
                    payload: row.get(3)?,
                    session_id: row.get(4)?,
                    attempts: row.get(5)?,
                    max_attempts: row.get(6)?,
                    last_error: row.get(7)?,
                    next_retry_at: row.get(8)?,
                    enqueued_at: row.get(9)?,
                    delivered_at: row.get(10)?,
                    permanent_failure: row.get::<_, i64>(11)? != 0,
                })
            },
        );

        match entry {
            Ok(e) => Ok(Some(e)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Mark a message as successfully delivered.
    pub fn mark_delivered(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let now = Self::now_ms();
        conn.execute(
            "UPDATE outbox SET delivered_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        debug!("Outbox delivered: {}", id);
        Ok(())
    }

    /// Record a transient failure and schedule the next retry with backoff.
    pub fn record_failure(&self, id: &str, error: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let now = Self::now_ms();

        let attempts: i64 = conn.query_row(
            "SELECT attempts FROM outbox WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;

        let backoff = backoff_ms(attempts as u32);
        let next_retry = now + backoff as i64;

        conn.execute(
            "UPDATE outbox SET attempts = attempts + 1, last_error = ?1, next_retry_at = ?2 WHERE id = ?3",
            params![error, next_retry, id],
        )?;

        warn!(
            "Outbox failure: {} (attempt {}, retry in {}ms): {}",
            id,
            attempts + 1,
            backoff,
            error
        );
        Ok(())
    }

    /// Mark a message as permanently failed (unrecoverable error).
    pub fn mark_permanent_failure(&self, id: &str, error: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        conn.execute(
            "UPDATE outbox SET permanent_failure = 1, last_error = ?1 WHERE id = ?2",
            params![error, id],
        )?;
        warn!("Outbox permanent failure: {}: {}", id, error);
        Ok(())
    }

    /// Get all pending messages ready for immediate delivery (for startup recovery).
    /// Resets next_retry_at to 0 so they are immediately eligible.
    pub fn recovery_sweep(&self) -> Result<Vec<OutboxEntry>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;

        // Reset all pending messages to immediate retry
        conn.execute(
            "UPDATE outbox SET next_retry_at = 0 WHERE delivered_at IS NULL AND permanent_failure = 0 AND attempts < max_attempts",
            [],
        )?;

        let mut stmt = conn.prepare(
            "SELECT id, channel, target, payload, session_id, attempts, max_attempts, last_error, next_retry_at, enqueued_at, delivered_at, permanent_failure
             FROM outbox
             WHERE delivered_at IS NULL AND permanent_failure = 0 AND attempts < max_attempts
             ORDER BY enqueued_at ASC",
        )?;

        let entries: Vec<OutboxEntry> = stmt
            .query_map([], |row| {
                Ok(OutboxEntry {
                    id: row.get(0)?,
                    channel: row.get(1)?,
                    target: row.get(2)?,
                    payload: row.get(3)?,
                    session_id: row.get(4)?,
                    attempts: row.get(5)?,
                    max_attempts: row.get(6)?,
                    last_error: row.get(7)?,
                    next_retry_at: row.get(8)?,
                    enqueued_at: row.get(9)?,
                    delivered_at: row.get(10)?,
                    permanent_failure: row.get::<_, i64>(11)? != 0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        if !entries.is_empty() {
            debug!("Outbox recovery sweep: {} pending messages", entries.len());
        }
        Ok(entries)
    }

    /// Delete delivered messages older than `retain_days` days.
    pub fn cleanup_delivered(&self, retain_days: u32) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let cutoff = Self::now_ms() - (retain_days as i64 * 86400 * 1000);

        let deleted = conn.execute(
            "DELETE FROM outbox WHERE delivered_at IS NOT NULL AND delivered_at < ?1",
            params![cutoff],
        )? as i64;

        if deleted > 0 {
            debug!("Outbox cleanup: removed {} delivered messages", deleted);
        }
        Ok(deleted)
    }

    /// Count pending (undelivered, non-permanent-failure) messages.
    pub fn pending_count(&self) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE delivered_at IS NULL AND permanent_failure = 0 AND attempts < max_attempts",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }
}

/// Compute backoff delay in milliseconds for a given attempt number.
pub fn backoff_ms(attempt: u32) -> u64 {
    BACKOFF_MS
        .get(attempt as usize)
        .copied()
        .unwrap_or(*BACKOFF_MS.last().unwrap_or(&600_000))
}

/// Check if an error is permanent (unrecoverable) for Telegram.
pub fn is_permanent_telegram_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("chat not found")
        || lower.contains("bot was blocked")
        || lower.contains("bot was kicked")
        || lower.contains("user is deactivated")
        || lower.contains("group chat was deactivated")
        || lower.contains("chat_write_forbidden")
        || lower.contains("have no rights to send")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_outbox() -> (Outbox, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("outbox_test.sqlite");
        let outbox = Outbox::new(&db).unwrap();
        (outbox, dir)
    }

    #[test]
    fn test_enqueue_and_claim() {
        let (outbox, _dir) = test_outbox();

        let id = outbox
            .enqueue("telegram", "12345", "Hello world", Some("session-1"))
            .unwrap();
        assert!(!id.is_empty());

        let entry = outbox.claim_next().unwrap().unwrap();
        assert_eq!(entry.id, id);
        assert_eq!(entry.channel, "telegram");
        assert_eq!(entry.target, "12345");
        assert_eq!(entry.payload, "Hello world");
        assert_eq!(entry.session_id.as_deref(), Some("session-1"));
        assert_eq!(entry.attempts, 0);
    }

    #[test]
    fn test_mark_delivered() {
        let (outbox, _dir) = test_outbox();

        let id = outbox.enqueue("telegram", "12345", "Hello", None).unwrap();
        outbox.mark_delivered(&id).unwrap();

        // Should no longer be claimable
        assert!(outbox.claim_next().unwrap().is_none());
        assert_eq!(outbox.pending_count().unwrap(), 0);
    }

    #[test]
    fn test_retry_with_backoff() {
        let (outbox, _dir) = test_outbox();

        let id = outbox.enqueue("telegram", "12345", "Hello", None).unwrap();

        // Record failure — next retry should be in the future
        outbox.record_failure(&id, "network timeout").unwrap();

        // Can't claim immediately (next_retry_at is in the future)
        assert!(outbox.claim_next().unwrap().is_none());

        // But pending count still shows it
        assert_eq!(outbox.pending_count().unwrap(), 1);
    }

    #[test]
    fn test_permanent_failure() {
        let (outbox, _dir) = test_outbox();

        let id = outbox.enqueue("telegram", "12345", "Hello", None).unwrap();
        outbox
            .mark_permanent_failure(&id, "chat not found")
            .unwrap();

        // Not claimable and not counted as pending
        assert!(outbox.claim_next().unwrap().is_none());
        assert_eq!(outbox.pending_count().unwrap(), 0);
    }

    #[test]
    fn test_recovery_sweep() {
        let (outbox, _dir) = test_outbox();

        // Enqueue and simulate a failed message with future retry
        let id = outbox.enqueue("telegram", "12345", "Hello", None).unwrap();
        outbox.record_failure(&id, "timeout").unwrap();

        // Before recovery, can't claim (retry is in the future)
        assert!(outbox.claim_next().unwrap().is_none());

        // Recovery resets retry times
        let pending = outbox.recovery_sweep().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);

        // Now it's claimable
        assert!(outbox.claim_next().unwrap().is_some());
    }

    #[test]
    fn test_cleanup_delivered() {
        let (outbox, _dir) = test_outbox();

        let id = outbox.enqueue("telegram", "12345", "Hello", None).unwrap();
        outbox.mark_delivered(&id).unwrap();

        // Cleanup with 0 days retention removes everything
        let deleted = outbox.cleanup_delivered(0).unwrap();
        assert_eq!(deleted, 1);
    }

    #[test]
    fn test_backoff_schedule() {
        assert_eq!(backoff_ms(0), 5_000);
        assert_eq!(backoff_ms(1), 25_000);
        assert_eq!(backoff_ms(2), 120_000);
        assert_eq!(backoff_ms(3), 600_000);
        assert_eq!(backoff_ms(4), 600_000); // Capped at last value
        assert_eq!(backoff_ms(100), 600_000);
    }

    #[test]
    fn test_permanent_error_detection() {
        assert!(is_permanent_telegram_error(
            "Forbidden: bot was blocked by the user"
        ));
        assert!(is_permanent_telegram_error("Bad Request: chat not found"));
        assert!(is_permanent_telegram_error(
            "Forbidden: bot was kicked from the group chat"
        ));
        assert!(is_permanent_telegram_error(
            "Forbidden: user is deactivated"
        ));
        assert!(!is_permanent_telegram_error("Request timeout"));
        assert!(!is_permanent_telegram_error("Internal server error"));
    }

    #[test]
    fn test_multiple_messages_ordering() {
        let (outbox, _dir) = test_outbox();

        let id1 = outbox.enqueue("telegram", "111", "First", None).unwrap();
        let id2 = outbox.enqueue("telegram", "222", "Second", None).unwrap();

        // Claims should come in enqueue order (next_retry_at = 0 for both)
        let entry1 = outbox.claim_next().unwrap().unwrap();
        outbox.mark_delivered(&entry1.id).unwrap();
        let entry2 = outbox.claim_next().unwrap().unwrap();

        assert_eq!(entry1.id, id1);
        assert_eq!(entry2.id, id2);
    }
}
