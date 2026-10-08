// Jev decision logging

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevDecision {
    pub id: String,
    pub memory_id: Option<String>,
    pub decision_type: String,
    pub input: String,
    pub output: String,
    pub confidence: Option<f64>,
    pub engine: String,
    pub created_at: String,
}

/// Input for logging a new Jev decision.
pub struct NewDecision<'a> {
    pub decision_type: &'a str,
    pub agent_id: &'a str,
    pub bank_id: Option<&'a str>,
    pub input_text: &'a str,
    pub input_memory_id: Option<&'a str>,
    pub output: &'a str,
    pub confidence: Option<f64>,
    pub model_used: &'a str,
    pub latency_ms: Option<i64>,
}

/// Log a Jev decision.
pub fn log_decision(conn: &Connection, input: &NewDecision) -> Result<JevDecision> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO jev_decisions (id, memory_id, decision_type, input, output, confidence, engine, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            input.input_memory_id,
            input.decision_type,
            input.input_text,
            input.output,
            input.confidence,
            input.model_used,
            now,
        ],
    )
    .context("Failed to log Jev decision")?;

    Ok(JevDecision {
        id,
        memory_id: input.input_memory_id.map(|s| s.to_string()),
        decision_type: input.decision_type.to_string(),
        input: input.input_text.to_string(),
        output: input.output.to_string(),
        confidence: input.confidence,
        engine: input.model_used.to_string(),
        created_at: now,
    })
}

/// Get a decision by ID.
pub fn get_decision(conn: &Connection, decision_id: &str) -> Result<Option<JevDecision>> {
    let mut stmt = conn.prepare(
        "SELECT id, memory_id, decision_type, input, output, confidence, engine, created_at
         FROM jev_decisions WHERE id = ?1",
    )?;

    let decision = stmt
        .query_row(params![decision_id], |row| {
            Ok(JevDecision {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                decision_type: row.get(2)?,
                input: row.get(3)?,
                output: row.get(4)?,
                confidence: row.get(5)?,
                engine: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .optional()?;

    Ok(decision)
}

/// Get recent decisions for a memory.
pub fn get_decisions_for_memory(
    conn: &Connection,
    memory_id: &str,
    limit: usize,
) -> Result<Vec<JevDecision>> {
    let mut stmt = conn.prepare(
        "SELECT id, memory_id, decision_type, input, output, confidence, engine, created_at
         FROM jev_decisions WHERE memory_id = ?1
         ORDER BY created_at DESC
         LIMIT ?2",
    )?;

    let decisions = stmt
        .query_map(params![memory_id, limit as i64], |row| {
            Ok(JevDecision {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                decision_type: row.get(2)?,
                input: row.get(3)?,
                output: row.get(4)?,
                confidence: row.get(5)?,
                engine: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(decisions)
}

/// Get recent decisions by type.
pub fn get_decisions_by_type(
    conn: &Connection,
    decision_type: &str,
    limit: usize,
) -> Result<Vec<JevDecision>> {
    let mut stmt = conn.prepare(
        "SELECT id, memory_id, decision_type, input, output, confidence, engine, created_at
         FROM jev_decisions WHERE decision_type = ?1
         ORDER BY created_at DESC
         LIMIT ?2",
    )?;

    let decisions = stmt
        .query_map(params![decision_type, limit as i64], |row| {
            Ok(JevDecision {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                decision_type: row.get(2)?,
                input: row.get(3)?,
                output: row.get(4)?,
                confidence: row.get(5)?,
                engine: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(decisions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Connection, String) {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/0001_initial.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO banks (id, name, agent_id) VALUES ('b1', 'test-bank', 'default')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memories (id, bank_id, content) VALUES ('m1', 'b1', 'test memory')",
            [],
        )
        .unwrap();
        (conn, "m1".to_string())
    }

    #[test]
    fn test_log_and_get_decision() {
        let (conn, memory_id) = setup();

        let input = NewDecision {
            decision_type: "classify",
            agent_id: "default",
            bank_id: Some("b1"),
            input_text: "The sky is blue",
            input_memory_id: Some(&memory_id),
            output: r#"{"type": "world_fact", "confidence": 0.92}"#,
            confidence: Some(0.92),
            model_used: "kev-0.8b",
            latency_ms: Some(45),
        };

        let decision = log_decision(&conn, &input).unwrap();
        assert_eq!(decision.decision_type, "classify");
        assert_eq!(decision.engine, "kev-0.8b");

        let fetched = get_decision(&conn, &decision.id).unwrap().unwrap();
        assert_eq!(fetched.id, decision.id);
        assert_eq!(fetched.memory_id, Some(memory_id));
    }

    #[test]
    fn test_get_decisions_for_memory() {
        let (conn, memory_id) = setup();

        for i in 0..3 {
            let input = NewDecision {
                decision_type: "classify",
                agent_id: "default",
                bank_id: Some("b1"),
                input_text: &format!("Input {}", i),
                input_memory_id: Some(&memory_id),
                output: "{}",
                confidence: Some(0.8),
                model_used: "mock",
                latency_ms: None,
            };
            log_decision(&conn, &input).unwrap();
        }

        let decisions = get_decisions_for_memory(&conn, &memory_id, 10).unwrap();
        assert_eq!(decisions.len(), 3);
    }

    #[test]
    fn test_get_decisions_by_type() {
        let (conn, memory_id) = setup();

        let input = NewDecision {
            decision_type: "score",
            agent_id: "default",
            bank_id: Some("b1"),
            input_text: "test",
            input_memory_id: Some(&memory_id),
            output: "{}",
            confidence: Some(0.7),
            model_used: "mock",
            latency_ms: None,
        };
        log_decision(&conn, &input).unwrap();

        let decisions = get_decisions_by_type(&conn, "score", 10).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].decision_type, "score");
    }
}
