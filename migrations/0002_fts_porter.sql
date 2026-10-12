-- Rebuild FTS index with Porter stemmer
-- Fixes: "preferences" now matches "prefer"

DROP TABLE IF EXISTS memories_fts;

CREATE VIRTUAL TABLE memories_fts USING fts5(
    content,
    bank_id UNINDEXED,
    tokenize='porter unicode61'
);

-- Rebuild index from existing memories
INSERT INTO memories_fts (rowid, content, bank_id)
SELECT rowid, content, bank_id FROM memories;
