// Memory storage and provenance

use anyhow::{Context, Result};
use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub rowid: i64,
    pub id: String,
    pub bank_id: String,
    pub content: String,
    pub memory_type: String,
    pub importance: u8,
    pub confidence: f32,
    pub source_agent: String,
    pub source_context: Option<String>,
    pub provenance: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_recalled_at: Option<String>,
    pub recall_count: u32,
}

impl Memory {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            rowid: row.get(0)?,
            id: row.get(1)?,
            bank_id: row.get(2)?,
            content: row.get(3)?,
            memory_type: row.get(4)?,
            importance: row.get(5)?,
            confidence: row.get(6)?,
            source_agent: row.get(7)?,
            source_context: row.get(8)?,
            provenance: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
            last_recalled_at: row.get(12)?,
            recall_count: row.get(13)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMemory {
    pub content: String,
    pub memory_type: String,
    pub importance: u8,
    pub confidence: f32,
    pub source_agent: String,
    pub source_context: Option<String>,
    pub provenance: Option<String>,
}

impl Db {
    /// Store a new memory
    pub fn store_memory(&self, bank_id: &str, new_memory: NewMemory) -> Result<Memory> {
        let conn = self.conn.lock().unwrap();
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        // Use a transaction to ensure both inserts succeed or neither
        let tx = conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO memories (id, bank_id, content, memory_type, importance, confidence,
                                   source_agent, source_context, provenance, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                id,
                bank_id,
                new_memory.content,
                new_memory.memory_type,
                new_memory.importance,
                new_memory.confidence,
                new_memory.source_agent,
                new_memory.source_context,
                new_memory.provenance,
                now,
            ],
        )
        .context("failed to store memory")?;

        // Update FTS index using the explicit rowid
        let rowid: i64 = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO memories_fts (rowid, content, bank_id) VALUES (?1, ?2, ?3)",
            params![rowid, new_memory.content, bank_id],
        )?;

        tx.commit()?;

        Ok(Memory {
            rowid,
            id,
            bank_id: bank_id.to_string(),
            content: new_memory.content,
            memory_type: new_memory.memory_type,
            importance: new_memory.importance,
            confidence: new_memory.confidence,
            source_agent: new_memory.source_agent,
            source_context: new_memory.source_context,
            provenance: new_memory.provenance,
            created_at: now.clone(),
            updated_at: now,
            last_recalled_at: None,
            recall_count: 0,
        })
    }

    /// Get memory by ID
    pub fn get_memory(&self, id: &str) -> Result<Option<Memory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT rowid, id, bank_id, content, memory_type, importance, confidence,
                    source_agent, source_context, provenance, created_at, updated_at,
                    last_recalled_at, recall_count
             FROM memories WHERE id = ?1"
        )?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            Ok(Some(Memory::from_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// Recall memories from a bank using FTS + importance ranking
    pub fn recall_memories(&self, bank_id: &str, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();

        // Try FTS first
        let fts_sql = "
            SELECT m.rowid, m.id, m.bank_id, m.content, m.memory_type, m.importance, m.confidence,
                   m.source_agent, m.source_context, m.provenance, m.created_at, m.updated_at,
                   m.last_recalled_at, m.recall_count
            FROM memories m
            JOIN memories_fts fts ON m.rowid = fts.rowid
            WHERE memories_fts MATCH ?1 AND m.bank_id = ?2
            ORDER BY m.importance DESC, m.created_at DESC
            LIMIT ?3
        ";

        let mut stmt = conn.prepare(fts_sql)?;
        let rows = stmt.query_map(params![query, bank_id, limit as i64], Memory::from_row);

        let mut memories = Vec::new();
        match rows {
            Ok(rs) => {
                for row in rs {
                    memories.push(row?);
                }
            }
            Err(_) => {
                // FTS failed, fall back to LIKE
                let like_sql = "
                    SELECT rowid, id, bank_id, content, memory_type, importance, confidence,
                           source_agent, source_context, provenance, created_at, updated_at,
                           last_recalled_at, recall_count
                    FROM memories
                    WHERE bank_id = ?1 AND content LIKE ?2
                    ORDER BY importance DESC, created_at DESC
                    LIMIT ?3
                ";
                let like_pattern = format!("%{}%", query);
                let mut stmt = conn.prepare(like_sql)?;
                let rows = stmt.query_map(params![bank_id, like_pattern, limit as i64], Memory::from_row)?;
                for row in rows {
                    memories.push(row?);
                }
            }
        }

        // If FTS returned nothing, also try LIKE as fallback
        if memories.is_empty() {
            let like_sql = "
                SELECT rowid, id, bank_id, content, memory_type, importance, confidence,
                       source_agent, source_context, provenance, created_at, updated_at,
                       last_recalled_at, recall_count
                FROM memories
                WHERE bank_id = ?1 AND content LIKE ?2
                ORDER BY importance DESC, created_at DESC
                LIMIT ?3
            ";
            let like_pattern = format!("%{}%", query);
            let mut stmt = conn.prepare(like_sql)?;
            let rows = stmt.query_map(params![bank_id, like_pattern, limit as i64], Memory::from_row)?;
            for row in rows {
                memories.push(row?);
            }
        }

        // Update recall stats
        let now = chrono::Utc::now().to_rfc3339();
        for mem in &memories {
            let _ = conn.execute(
                "UPDATE memories SET last_recalled_at = ?1, recall_count = recall_count + 1 WHERE id = ?2",
                params![now, mem.id],
            );
        }

        Ok(memories)
    }

    /// Get recent memories from a bank
    pub fn recent_memories(&self, bank_id: &str, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT rowid, id, bank_id, content, memory_type, importance, confidence,
                    source_agent, source_context, provenance, created_at, updated_at,
                    last_recalled_at, recall_count
             FROM memories
             WHERE bank_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![bank_id, limit as i64], Memory::from_row)?;

        let mut memories = Vec::new();
        for row in rows {
            memories.push(row?);
        }
        Ok(memories)
    }

    /// Get all memories from a bank (for reflection)
    pub fn all_memories(&self, bank_id: &str) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT rowid, id, bank_id, content, memory_type, importance, confidence,
                    source_agent, source_context, provenance, created_at, updated_at,
                    last_recalled_at, recall_count
             FROM memories
             WHERE bank_id = ?1
             ORDER BY importance DESC, created_at DESC"
        )?;
        let rows = stmt.query_map(params![bank_id], Memory::from_row)?;

        let mut memories = Vec::new();
        for row in rows {
            memories.push(row?);
        }
        Ok(memories)
    }

    /// Delete a memory
    pub fn delete_memory(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Count memories in a bank
    pub fn count_memories(&self, bank_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE bank_id = ?1",
            params![bank_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }
}
