// Bank CRUD operations

use anyhow::{Context, Result};
use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bank {
    pub id: String,
    pub name: String,
    pub agent_id: String,
    pub bank_type: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Bank {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            agent_id: row.get(2)?,
            bank_type: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    }
}

impl Db {
    /// Create a new bank
    pub fn create_bank(&self, name: &str, agent_id: &str, bank_type: &str) -> Result<Bank> {
        let conn = self.conn.lock().unwrap();
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO banks (id, name, agent_id, bank_type, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, name, agent_id, bank_type, now],
        )
        .context("failed to create bank")?;

        Ok(Bank {
            id,
            name: name.to_string(),
            agent_id: agent_id.to_string(),
            bank_type: bank_type.to_string(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// Get bank by ID
    pub fn get_bank(&self, id: &str) -> Result<Option<Bank>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, agent_id, bank_type, created_at, updated_at FROM banks WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            Ok(Some(Bank::from_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get bank by name
    pub fn get_bank_by_name(&self, name: &str) -> Result<Option<Bank>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, agent_id, bank_type, created_at, updated_at FROM banks WHERE name = ?1")?;
        let mut rows = stmt.query(params![name])?;

        if let Some(row) = rows.next()? {
            Ok(Some(Bank::from_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// Get or create bank by name
    pub fn get_or_create_bank(&self, name: &str, agent_id: &str) -> Result<Bank> {
        if let Some(bank) = self.get_bank_by_name(name)? {
            Ok(bank)
        } else {
            self.create_bank(name, agent_id, "private")
        }
    }

    /// List all banks
    pub fn list_banks(&self) -> Result<Vec<Bank>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, agent_id, bank_type, created_at, updated_at FROM banks ORDER BY created_at")?;
        let rows = stmt.query_map([], Bank::from_row)?;

        let mut banks = Vec::new();
        for row in rows {
            banks.push(row?);
        }
        Ok(banks)
    }

    /// List banks for an agent
    pub fn list_banks_for_agent(&self, agent_id: &str) -> Result<Vec<Bank>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, agent_id, bank_type, created_at, updated_at FROM banks WHERE agent_id = ?1 ORDER BY created_at")?;
        let rows = stmt.query_map(params![agent_id], Bank::from_row)?;

        let mut banks = Vec::new();
        for row in rows {
            banks.push(row?);
        }
        Ok(banks)
    }

    /// Delete a bank
    pub fn delete_bank(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM banks WHERE id = ?1", params![id])?;
        Ok(())
    }
}
