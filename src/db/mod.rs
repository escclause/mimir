// Database layer
// SQLite with migrations

pub mod agents;
pub mod banks;
pub mod jev_log;
pub mod memories;

use anyhow::{Context, Result};
use rusqlite::Connection;
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
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let input = jev_log::NewDecision {
            decision_type,
            agent_id: "default",
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
}
