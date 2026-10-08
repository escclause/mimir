-- Mímir database schema
-- Migration: 0001_initial

-- Memory banks (isolated per agent/user/project)
CREATE TABLE IF NOT EXISTS banks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    agent_id TEXT NOT NULL DEFAULT 'default',
    bank_type TEXT NOT NULL DEFAULT 'private', -- 'private', 'shared'
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Memories with provenance
-- rowid is explicit for FTS5 join (SQLite internal rowid)
CREATE TABLE IF NOT EXISTS memories (
    rowid INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    bank_id TEXT NOT NULL REFERENCES banks(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    memory_type TEXT NOT NULL DEFAULT 'world_fact', -- 'world_fact', 'experience', 'observation', 'mental_model'
    importance INTEGER NOT NULL DEFAULT 3, -- 1-5
    confidence REAL NOT NULL DEFAULT 1.0,
    source_agent TEXT NOT NULL DEFAULT 'unknown',
    source_context TEXT,
    provenance TEXT, -- JSON array of source memory IDs
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_recalled_at TEXT,
    recall_count INTEGER NOT NULL DEFAULT 0
);

-- Indexes for recall
CREATE INDEX IF NOT EXISTS idx_memories_bank ON memories(bank_id);
CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);

-- Full-text search
-- Uses internal rowid for join with memories table
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    content,
    bank_id UNINDEXED
);

-- Jev decision log
CREATE TABLE IF NOT EXISTS jev_decisions (
    id TEXT PRIMARY KEY,
    memory_id TEXT REFERENCES memories(id) ON DELETE SET NULL,
    decision_type TEXT NOT NULL, -- 'classify', 'score', 'contradiction', 'route'
    input TEXT NOT NULL,
    output TEXT NOT NULL,
    confidence REAL,
    engine TEXT NOT NULL DEFAULT 'mock',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_jev_decisions_memory ON jev_decisions(memory_id);
CREATE INDEX IF NOT EXISTS idx_jev_decisions_type ON jev_decisions(decision_type);

-- Agent permissions
CREATE TABLE IF NOT EXISTS agent_permissions (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    bank_id TEXT NOT NULL REFERENCES banks(id) ON DELETE CASCADE,
    can_write INTEGER NOT NULL DEFAULT 0,
    can_read INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(agent_id, bank_id)
);

-- Night shift audit log
CREATE TABLE IF NOT EXISTS night_audits (
    id TEXT PRIMARY KEY,
    audit_date TEXT NOT NULL,
    decisions_reviewed INTEGER NOT NULL DEFAULT 0,
    corrections_made INTEGER NOT NULL DEFAULT 0,
    models_built INTEGER NOT NULL DEFAULT 0,
    summary TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Contradiction tracking
CREATE TABLE IF NOT EXISTS contradictions (
    id TEXT PRIMARY KEY,
    new_memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    existing_memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    resolution TEXT, -- 'kept_new', 'kept_old', 'merged', 'unresolved'
    resolved_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
