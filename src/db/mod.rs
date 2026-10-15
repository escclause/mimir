// Database layer
// SQLite with migrations

pub mod agents;
pub mod banks;
pub mod jev_log;
pub mod memories;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Thread-safe database handle
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").finish_non_exhaustive()
    }
}

impl Db {
    /// Open or create database at the given path
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open database at {}", path.display()))?;

        // Enable WAL mode for better concurrency
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        db.migrate()?;
        Ok(db)
    }

    /// Run migrations
    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Create migrations tracking table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS _migrations (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Check which migrations have been applied
        let applied: Vec<String> = {
            let mut stmt = conn.prepare("SELECT name FROM _migrations")?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        // Run initial migration if not applied
        if !applied.contains(&"0001_initial".to_string()) {
            tracing::info!("applying migration 0001_initial");
            conn.execute_batch(include_str!("../../migrations/0001_initial.sql"))?;
            conn.execute(
                "INSERT INTO _migrations (name) VALUES ('0001_initial')",
                [],
            )?;
        }

        // Rebuild FTS index with Porter stemmer
        if !applied.contains(&"0002_fts_porter".to_string()) {
            tracing::info!("applying migration 0002_fts_porter");
            conn.execute_batch(include_str!("../../migrations/0002_fts_porter.sql"))?;
            conn.execute(
                "INSERT INTO _migrations (name) VALUES ('0002_fts_porter')",
                [],
            )?;
        }

        // Seed default banks on first run
        Self::seed_default_banks(&conn)?;

        Ok(())
    }

    /// Seed default banks on first run.
    /// Creates a shared knowledge bank that all agents can read.
    fn seed_default_banks(conn: &Connection) -> Result<()> {
        // Check if any banks exist
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM banks", [], |row| row.get(0))?;

        if count == 0 {
            tracing::info!("seeding default banks");

            // Create shared knowledge bank (owned by 'system', readable by all)
            let id = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO banks (id, name, agent_id, bank_type, created_at, updated_at)
                 VALUES (?1, 'shared_knowledge', 'system', 'shared', ?2, ?2)",
                params![id, now],
            )?;

            tracing::info!("created shared_knowledge bank");
        }

        Ok(())
    }

    /// Get a cloned connection handle
    pub fn conn(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.conn)
    }

    /// Check if an agent can read from a bank
    pub fn can_read(&self, agent_id: &str, bank_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        agents::can_read(&conn, agent_id, bank_id)
    }

    /// Check if an agent can write to a bank
    pub fn can_write(&self, agent_id: &str, bank_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        agents::can_write(&conn, agent_id, bank_id)
    }

    /// Log a Jev decision (convenience wrapper for MCP layer).
    #[allow(clippy::too_many_arguments)]
    pub fn log_jev_decision(
        &self,
        memory_id: Option<&str>,
        decision_type: &str,
        input_text: &str,
        output: &str,
        confidence: Option<f32>,
        model_used: &str,
        agent_id: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let input = jev_log::NewDecision {
            decision_type,
            agent_id,
            bank_id: None,
            input_text,
            input_memory_id: memory_id,
            output,
            confidence: confidence.map(|c| c as f64),
            model_used,
            latency_ms: None,
        };
        jev_log::log_decision(&conn, &input)?;
        Ok(())
    }

    /// Get recent Jev decisions (for night shift audit).
    pub fn get_recent_jev_decisions(&self, limit: usize) -> Result<Vec<jev_log::JevDecision>> {
        let conn = self.conn.lock().unwrap();
        jev_log::get_recent_decisions(&conn, limit)
    }

    /// Get memories ready for consolidation (for night shift).
    /// Returns memories that are observations or experiences with importance >= 2.
    pub fn get_memories_for_consolidation(&self, limit: usize) -> Result<Vec<memories::Memory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT rowid, id, bank_id, content, memory_type, importance, confidence,
                    source_agent, source_context, provenance, created_at, updated_at,
                    last_recalled_at, recall_count
             FROM memories
             WHERE memory_type IN ('observation', 'experience', 'mental_model')
               AND importance >= 2
             ORDER BY importance DESC, created_at DESC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], memories::Memory::from_row)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Store a synthesis result (for night shift).
    pub fn store_synthesis(&self, synthesis: &str, memory_count: usize) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS syntheses (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                memory_count INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;
        conn.execute(
            "INSERT INTO syntheses (content, memory_count) VALUES (?1, ?2)",
            params![synthesis, memory_count as i64],
        )?;
        Ok(())
    }

    /// Update memory importance (for night shift repair/decay).
    pub fn update_memory_importance(&self, memory_id: &str, importance: u8) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET importance = ?1, updated_at = ?2 WHERE id = ?3",
            params![importance, now, memory_id],
        )?;
        Ok(())
    }

    /// Log a night shift audit run.
    pub fn log_night_audit(
        &self,
        audit_date: &str,
        decisions_reviewed: i64,
        corrections_made: i64,
        models_built: i64,
        summary: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO night_audits (id, audit_date, decisions_reviewed, corrections_made, models_built, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, audit_date, decisions_reviewed, corrections_made, models_built, summary],
        )?;
        Ok(())
    }
}
