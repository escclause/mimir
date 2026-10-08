// Agent permissions

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPermission {
    pub id: String,
    pub agent_id: String,
    pub bank_id: String,
    pub can_write: bool,
    pub can_read: bool,
    pub created_at: String,
}

/// Grant an agent read (and optionally write) access to a bank.
pub fn grant_permission(
    conn: &Connection,
    agent_id: &str,
    bank_id: &str,
    can_write: bool,
) -> Result<AgentPermission> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO agent_permissions (id, agent_id, bank_id, can_write, can_read, created_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5)
         ON CONFLICT(agent_id, bank_id) DO UPDATE SET
            can_write = excluded.can_write,
            can_read = 1",
        params![id, agent_id, bank_id, can_write as i64, now],
    )
    .context("Failed to grant permission")?;

    Ok(AgentPermission {
        id,
        agent_id: agent_id.to_string(),
        bank_id: bank_id.to_string(),
        can_write,
        can_read: true,
        created_at: now,
    })
}

/// Revoke an agent's access to a bank.
pub fn revoke_permission(conn: &Connection, agent_id: &str, bank_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM agent_permissions WHERE agent_id = ?1 AND bank_id = ?2",
        params![agent_id, bank_id],
    )?;
    Ok(())
}

/// Check if an agent can read from a bank.
/// Agents can always read their own banks; shared banks are read-only for everyone.
pub fn can_read(conn: &Connection, agent_id: &str, bank_id: &str) -> Result<bool> {
    // Own bank
    let owns: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM banks WHERE id = ?1 AND agent_id = ?2",
            params![bank_id, agent_id],
            |row| {
                let c: i64 = row.get(0)?;
                Ok(c > 0)
            },
        )
        .unwrap_or(false);

    if owns {
        return Ok(true);
    }

    // Shared bank
    let is_shared: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM banks WHERE id = ?1 AND bank_type = 'shared'",
            params![bank_id],
            |row| {
                let c: i64 = row.get(0)?;
                Ok(c > 0)
            },
        )
        .unwrap_or(false);

    if is_shared {
        return Ok(true);
    }

    // Explicit permission
    let permitted: bool = conn
        .query_row(
            "SELECT can_read FROM agent_permissions WHERE agent_id = ?1 AND bank_id = ?2",
            params![agent_id, bank_id],
            |row| {
                let v: i64 = row.get(0)?;
                Ok(v != 0)
            },
        )
        .unwrap_or(false);

    Ok(permitted)
}

/// Check if an agent can write to a bank.
/// Agents can always write to their own banks; shared banks are read-only.
pub fn can_write(conn: &Connection, agent_id: &str, bank_id: &str) -> Result<bool> {
    // Own bank
    let owns: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM banks WHERE id = ?1 AND agent_id = ?2",
            params![bank_id, agent_id],
            |row| {
                let c: i64 = row.get(0)?;
                Ok(c > 0)
            },
        )
        .unwrap_or(false);

    if owns {
        return Ok(true);
    }

    // Shared banks are read-only
    let is_shared: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM banks WHERE id = ?1 AND bank_type = 'shared'",
            params![bank_id],
            |row| {
                let c: i64 = row.get(0)?;
                Ok(c > 0)
            },
        )
        .unwrap_or(false);

    if is_shared {
        return Ok(false);
    }

    // Explicit permission
    let permitted: bool = conn
        .query_row(
            "SELECT can_write FROM agent_permissions WHERE agent_id = ?1 AND bank_id = ?2",
            params![agent_id, bank_id],
            |row| {
                let v: i64 = row.get(0)?;
                Ok(v != 0)
            },
        )
        .unwrap_or(false);

    Ok(permitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Connection, String, String) {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/0001_initial.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO banks (id, name, agent_id, bank_type) VALUES ('b1', 'bank-1', 'alice', 'private')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO banks (id, name, agent_id, bank_type) VALUES ('b2', 'shared-bank', 'system', 'shared')",
            [],
        )
        .unwrap();
        ("alice".to_string(), "bob".to_string(), "b1".to_string());
        (conn, "alice".to_string(), "bob".to_string())
    }

    #[test]
    fn test_own_bank_implicit_access() {
        let (conn, alice, _) = setup();
        assert!(can_read(&conn, &alice, "b1").unwrap());
        assert!(can_write(&conn, &alice, "b1").unwrap());
    }

    #[test]
    fn test_shared_bank_readonly() {
        let (conn, _, bob) = setup();
        // Bob can read shared bank
        assert!(can_read(&conn, &bob, "b2").unwrap());
        // But cannot write
        assert!(!can_write(&conn, &bob, "b2").unwrap());
    }

    #[test]
    fn test_grant_and_revoke() {
        let (conn, _, bob) = setup();

        // No access initially
        assert!(!can_read(&conn, &bob, "b1").unwrap());

        // Grant read
        grant_permission(&conn, &bob, "b1", false).unwrap();
        assert!(can_read(&conn, &bob, "b1").unwrap());
        assert!(!can_write(&conn, &bob, "b1").unwrap());

        // Upgrade to write
        grant_permission(&conn, &bob, "b1", true).unwrap();
        assert!(can_write(&conn, &bob, "b1").unwrap());

        // Revoke
        revoke_permission(&conn, &bob, "b1").unwrap();
        assert!(!can_read(&conn, &bob, "b1").unwrap());
        assert!(!can_write(&conn, &bob, "b1").unwrap());
    }
}
