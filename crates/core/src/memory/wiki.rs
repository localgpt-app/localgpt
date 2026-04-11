//! Memory Wiki — structured knowledge management with claims, evidence, and staleness tracking.
//!
//! Claims are first-class objects with confidence scores, categories, and linked evidence.
//! Freshness degrades over time: Fresh → Aging → Stale based on configurable thresholds.

use anyhow::{Result, anyhow};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;
use uuid::Uuid;

/// A structured knowledge claim with evidence and staleness tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub text: String,
    pub category: ClaimCategory,
    pub confidence: f32,
    pub status: ClaimStatus,
    pub evidence: Vec<Evidence>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimCategory {
    Fact,
    Preference,
    Decision,
    Question,
}

impl ClaimCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
            Self::Decision => "decision",
            Self::Question => "question",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "fact" => Ok(Self::Fact),
            "preference" => Ok(Self::Preference),
            "decision" => Ok(Self::Decision),
            "question" => Ok(Self::Question),
            _ => Err(anyhow!("Unknown claim category: {}", s)),
        }
    }
}

impl std::fmt::Display for ClaimCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimStatus {
    Active,
    Contested,
    Refuted,
    Superseded,
}

impl ClaimStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Contested => "contested",
            Self::Refuted => "refuted",
            Self::Superseded => "superseded",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "active" => Ok(Self::Active),
            "contested" => Ok(Self::Contested),
            "refuted" => Ok(Self::Refuted),
            "superseded" => Ok(Self::Superseded),
            _ => Err(anyhow!("Unknown claim status: {}", s)),
        }
    }
}

impl std::fmt::Display for ClaimStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Evidence supporting a claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: i64,
    pub source: String,
    pub excerpt: String,
    pub weight: f32,
    pub added_at: i64,
}

/// Freshness tier based on time since last update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Freshness {
    Fresh,
    Aging,
    Stale,
}

impl Freshness {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Aging => "aging",
            Self::Stale => "stale",
        }
    }
}

impl std::fmt::Display for Freshness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Knowledge base health summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiStatus {
    pub total_claims: i64,
    pub by_category: Vec<(String, i64)>,
    pub by_status: Vec<(String, i64)>,
    pub by_freshness: Vec<(String, i64)>,
    pub top_stale: Vec<Claim>,
}

/// SQLite-backed wiki store for structured knowledge management.
#[derive(Clone)]
pub struct WikiStore {
    conn: Arc<Mutex<Connection>>,
    fresh_days: u32,
    stale_days: u32,
}

impl WikiStore {
    /// Open or create a wiki store using the given SQLite database path.
    pub fn new(db_path: &Path, fresh_days: u32, stale_days: u32) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            fresh_days,
            stale_days,
        })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS wiki_claims (
                id TEXT PRIMARY KEY,
                text TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT 'fact',
                confidence REAL NOT NULL DEFAULT 0.8,
                status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS wiki_evidence (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                claim_id TEXT NOT NULL REFERENCES wiki_claims(id) ON DELETE CASCADE,
                source TEXT NOT NULL,
                excerpt TEXT NOT NULL,
                weight REAL NOT NULL DEFAULT 0.5,
                added_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_wiki_claims_category ON wiki_claims(category);
            CREATE INDEX IF NOT EXISTS idx_wiki_claims_status ON wiki_claims(status);
            CREATE INDEX IF NOT EXISTS idx_wiki_evidence_claim ON wiki_evidence(claim_id);
            "#,
        )?;

        // FTS5 virtual table for claim text search
        conn.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS wiki_claims_fts USING fts5(
                text,
                id UNINDEXED,
                category UNINDEXED
            );
            "#,
        )?;

        Ok(())
    }

    fn now_secs() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    /// Compute freshness tier for a given `updated_at` timestamp.
    pub fn freshness(&self, updated_at: i64) -> Freshness {
        freshness_at(
            updated_at,
            Self::now_secs(),
            self.fresh_days,
            self.stale_days,
        )
    }

    /// Add a new claim with optional evidence. If a similar claim exists (FTS match),
    /// update its evidence and bump `updated_at` instead of creating a duplicate.
    pub fn add_claim(
        &self,
        text: &str,
        category: ClaimCategory,
        confidence: f32,
        evidence_source: Option<&str>,
        evidence_excerpt: Option<&str>,
    ) -> Result<String> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;

        // Check for duplicate via FTS5 — exact or very similar text
        let existing_id: Option<String> = {
            // Quote the text for FTS5 phrase matching
            let fts_query = format!("\"{}\"", text.replace('"', "\"\""));
            conn.query_row(
                "SELECT id FROM wiki_claims_fts WHERE wiki_claims_fts MATCH ?1 LIMIT 1",
                params![fts_query],
                |row| row.get(0),
            )
            .ok()
        };

        let now = Self::now_secs();

        if let Some(id) = existing_id {
            // Update existing claim
            debug!("Updating existing wiki claim: {}", id);
            conn.execute(
                "UPDATE wiki_claims SET confidence = ?1, updated_at = ?2 WHERE id = ?3",
                params![confidence, now, id],
            )?;

            // Add new evidence if provided
            if let (Some(source), Some(excerpt)) = (evidence_source, evidence_excerpt) {
                conn.execute(
                    "INSERT INTO wiki_evidence (claim_id, source, excerpt, weight, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, source, excerpt, 0.5f32, now],
                )?;
            }

            Ok(id)
        } else {
            // Create new claim
            let id = Uuid::new_v4().to_string();
            debug!("Creating new wiki claim: {}", id);

            conn.execute(
                "INSERT INTO wiki_claims (id, text, category, confidence, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6)",
                params![id, text, category.as_str(), confidence, now, now],
            )?;

            // Insert into FTS
            conn.execute(
                "INSERT INTO wiki_claims_fts (text, id, category) VALUES (?1, ?2, ?3)",
                params![text, id, category.as_str()],
            )?;

            // Add evidence if provided
            if let (Some(source), Some(excerpt)) = (evidence_source, evidence_excerpt) {
                conn.execute(
                    "INSERT INTO wiki_evidence (claim_id, source, excerpt, weight, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, source, excerpt, 0.5f32, now],
                )?;
            }

            Ok(id)
        }
    }

    /// Search claims by text query with optional filters.
    pub fn search(
        &self,
        query: &str,
        category: Option<ClaimCategory>,
        include_stale: bool,
        limit: usize,
    ) -> Result<Vec<Claim>> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let now = Self::now_secs();

        // Build FTS query — split words with OR for broader matching
        let fts_query = query
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ");

        // FTS search returns matching claim IDs
        let mut stmt =
            conn.prepare("SELECT id FROM wiki_claims_fts WHERE wiki_claims_fts MATCH ?1 LIMIT ?2")?;
        let ids: Vec<String> = stmt
            .query_map(params![fts_query, (limit * 2) as i64], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut claims = Vec::new();
        for id in &ids {
            if let Some(claim) = self.load_claim_with_conn(&conn, id)? {
                // Filter by category
                if let Some(ref cat) = category
                    && claim.category != *cat
                {
                    continue;
                }
                // Filter stale unless requested
                if !include_stale {
                    let freshness =
                        freshness_at(claim.updated_at, now, self.fresh_days, self.stale_days);
                    if freshness == Freshness::Stale {
                        continue;
                    }
                }
                claims.push(claim);
                if claims.len() >= limit {
                    break;
                }
            }
        }

        Ok(claims)
    }

    /// Load a single claim with its evidence.
    fn load_claim_with_conn(&self, conn: &Connection, id: &str) -> Result<Option<Claim>> {
        let row = conn.query_row(
            "SELECT id, text, category, confidence, status, created_at, updated_at FROM wiki_claims WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f32>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        );

        let (id, text, category_str, confidence, status_str, created_at, updated_at) = match row {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        // Load evidence
        let mut stmt = conn.prepare(
            "SELECT id, source, excerpt, weight, added_at FROM wiki_evidence WHERE claim_id = ?1 ORDER BY added_at DESC",
        )?;
        let evidence: Vec<Evidence> = stmt
            .query_map(params![id], |row| {
                Ok(Evidence {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    excerpt: row.get(2)?,
                    weight: row.get(3)?,
                    added_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(Some(Claim {
            id,
            text,
            category: ClaimCategory::parse(&category_str).unwrap_or(ClaimCategory::Fact),
            confidence,
            status: ClaimStatus::parse(&status_str).unwrap_or(ClaimStatus::Active),
            evidence,
            created_at,
            updated_at,
        }))
    }

    /// Get knowledge base health overview.
    pub fn status(&self) -> Result<WikiStatus> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let now = Self::now_secs();

        let total_claims: i64 =
            conn.query_row("SELECT COUNT(*) FROM wiki_claims", [], |row| row.get(0))?;

        // Breakdown by category
        let mut stmt = conn.prepare(
            "SELECT category, COUNT(*) FROM wiki_claims GROUP BY category ORDER BY COUNT(*) DESC",
        )?;
        let by_category: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        // Breakdown by status
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM wiki_claims GROUP BY status ORDER BY COUNT(*) DESC",
        )?;
        let by_status: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        // Breakdown by freshness
        let fresh_cutoff = now - (self.fresh_days as i64 * 86400);
        let stale_cutoff = now - (self.stale_days as i64 * 86400);

        let fresh_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wiki_claims WHERE updated_at >= ?1",
            params![fresh_cutoff],
            |row| row.get(0),
        )?;
        let stale_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wiki_claims WHERE updated_at < ?1",
            params![stale_cutoff],
            |row| row.get(0),
        )?;
        let aging_count = total_claims - fresh_count - stale_count;

        let by_freshness = vec![
            ("fresh".to_string(), fresh_count),
            ("aging".to_string(), aging_count),
            ("stale".to_string(), stale_count),
        ];

        // Top 5 stale claims
        let mut stmt = conn.prepare(
            "SELECT id FROM wiki_claims WHERE updated_at < ?1 ORDER BY updated_at ASC LIMIT 5",
        )?;
        let stale_ids: Vec<String> = stmt
            .query_map(params![stale_cutoff], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let mut top_stale = Vec::new();
        for id in &stale_ids {
            if let Some(claim) = self.load_claim_with_conn(&conn, id)? {
                top_stale.push(claim);
            }
        }

        Ok(WikiStatus {
            total_claims,
            by_category,
            by_status,
            by_freshness,
            top_stale,
        })
    }

    /// Get claim count (for tests).
    pub fn claim_count(&self) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM wiki_claims", [], |row| row.get(0))?;
        Ok(count)
    }
}

/// Compute freshness from timestamps and thresholds (pure function for testing).
pub fn freshness_at(updated_at: i64, now: i64, fresh_days: u32, stale_days: u32) -> Freshness {
    let age_days = (now - updated_at) / 86400;
    if age_days < fresh_days as i64 {
        Freshness::Fresh
    } else if age_days < stale_days as i64 {
        Freshness::Aging
    } else {
        Freshness::Stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_store() -> (WikiStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("wiki_test.sqlite");
        let store = WikiStore::new(&db, 30, 90).unwrap();
        (store, dir)
    }

    #[test]
    fn test_add_and_search_claim() {
        let (store, _dir) = test_store();

        let id = store
            .add_claim(
                "Rust is a systems programming language",
                ClaimCategory::Fact,
                0.9,
                Some("docs"),
                Some("From the Rust book"),
            )
            .unwrap();
        assert!(!id.is_empty());

        let results = store.search("Rust programming", None, false, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
        assert_eq!(results[0].evidence.len(), 1);
        assert_eq!(results[0].evidence[0].source, "docs");
    }

    #[test]
    fn test_deduplication() {
        let (store, _dir) = test_store();

        let id1 = store
            .add_claim("The sky is blue", ClaimCategory::Fact, 0.8, None, None)
            .unwrap();

        // Adding the same text should update, not create a new claim
        let id2 = store
            .add_claim(
                "The sky is blue",
                ClaimCategory::Fact,
                0.95,
                Some("observation"),
                Some("Looked outside"),
            )
            .unwrap();

        assert_eq!(id1, id2);
        assert_eq!(store.claim_count().unwrap(), 1);
    }

    #[test]
    fn test_category_filter() {
        let (store, _dir) = test_store();

        store
            .add_claim(
                "User prefers dark mode",
                ClaimCategory::Preference,
                0.9,
                None,
                None,
            )
            .unwrap();
        store
            .add_claim(
                "User decided to use Rust",
                ClaimCategory::Decision,
                0.8,
                None,
                None,
            )
            .unwrap();

        let prefs = store
            .search("User", Some(ClaimCategory::Preference), false, 10)
            .unwrap();
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0].category, ClaimCategory::Preference);
    }

    #[test]
    fn test_freshness_calculation() {
        let now = 1_000_000;
        // Updated today → fresh
        assert_eq!(freshness_at(now, now, 30, 90), Freshness::Fresh);
        // Updated 15 days ago → fresh
        assert_eq!(
            freshness_at(now - 15 * 86400, now, 30, 90),
            Freshness::Fresh
        );
        // Updated 45 days ago → aging
        assert_eq!(
            freshness_at(now - 45 * 86400, now, 30, 90),
            Freshness::Aging
        );
        // Updated 100 days ago → stale
        assert_eq!(
            freshness_at(now - 100 * 86400, now, 30, 90),
            Freshness::Stale
        );
    }

    #[test]
    fn test_stale_filtering() {
        let (store, _dir) = test_store();

        // Add a claim then manually make it stale
        let id = store
            .add_claim("Old knowledge", ClaimCategory::Fact, 0.5, None, None)
            .unwrap();
        {
            let conn = store.conn.lock().unwrap();
            let stale_time = WikiStore::now_secs() - 100 * 86400;
            conn.execute(
                "UPDATE wiki_claims SET updated_at = ?1 WHERE id = ?2",
                params![stale_time, id],
            )
            .unwrap();
        }

        // Default search excludes stale
        let results = store.search("Old knowledge", None, false, 10).unwrap();
        assert_eq!(results.len(), 0);

        // include_stale brings it back
        let results = store.search("Old knowledge", None, true, 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_status_overview() {
        let (store, _dir) = test_store();

        store
            .add_claim("Fact one", ClaimCategory::Fact, 0.9, None, None)
            .unwrap();
        store
            .add_claim("Fact two", ClaimCategory::Fact, 0.8, None, None)
            .unwrap();
        store
            .add_claim("A preference", ClaimCategory::Preference, 0.7, None, None)
            .unwrap();

        let status = store.status().unwrap();
        assert_eq!(status.total_claims, 3);
        assert!(
            status
                .by_freshness
                .iter()
                .any(|(k, v)| k == "fresh" && *v == 3)
        );
    }

    #[test]
    fn test_evidence_accumulation() {
        let (store, _dir) = test_store();

        store
            .add_claim(
                "Water boils at 100C",
                ClaimCategory::Fact,
                0.9,
                Some("textbook"),
                Some("Chapter 3"),
            )
            .unwrap();

        // Add more evidence via deduplication path
        store
            .add_claim(
                "Water boils at 100C",
                ClaimCategory::Fact,
                0.95,
                Some("experiment"),
                Some("Lab result"),
            )
            .unwrap();

        let results = store.search("Water boils", None, false, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].evidence.len(), 2);
    }

    #[test]
    fn test_claim_category_roundtrip() {
        for cat in [
            ClaimCategory::Fact,
            ClaimCategory::Preference,
            ClaimCategory::Decision,
            ClaimCategory::Question,
        ] {
            assert_eq!(ClaimCategory::parse(cat.as_str()).unwrap(), cat);
        }
    }

    #[test]
    fn test_claim_status_roundtrip() {
        for status in [
            ClaimStatus::Active,
            ClaimStatus::Contested,
            ClaimStatus::Refuted,
            ClaimStatus::Superseded,
        ] {
            assert_eq!(ClaimStatus::parse(status.as_str()).unwrap(), status);
        }
    }
}
