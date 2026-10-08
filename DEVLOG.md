# Mímir Development Log

## 2026-10-08: Project Initialization

### Decisions Made
- **Name**: Mímir (Norse god of wisdom, keeper of knowledge)
- **Language**: Rust with Axum
- **Jev Backend**: Kev-0.8B (confirmed)
- **Night Shift LLM**: Qwen2.5-7B (local, user-swappable)
- **Training**: Start with threshold tuning, upgrade to LoRA if needed
- **Integration**: OpenClaw-first via MCP, fork later for universal use
- **Config**: GUI-first, no config files

### Architecture Locked
- Day/night cognition cycle
- Per-agent banks (write) + shared bank (read)
- SQLite storage + JSONL cold transcripts + Markdown views
- MCP server + REST API + embedded Web UI

### Current State
- Rust project initialized
- Dependencies configured (Axum, rusqlite, tokio, etc.)
- Module skeleton created
- Binary builds and runs

### Next Steps
1. Database schema and migrations
2. Kev-0.8B ONNX integration
3. MCP server implementation
4. Web UI scaffolding
5. OpenClaw plugin integration

### Open Questions
- Web UI framework choice (Leptos vs SvelteKit vs plain HTML/JS)
- MCP SDK choice (rmcp vs custom JSON-RPC)
- llama.cpp binding for Qwen (llama-cpp-2 vs custom CGO)

---

## 2026-10-08: MCP Server Implementation

### Completed
- **Database layer**: SQLite with migrations, banks, memories, agents, jev_log modules
- **Migration 0001_initial**: Full schema with FTS5, indexes, foreign keys
- **MCP server**: `rmcp`-based stdio server with 4 tools
- **Jev engine**: `DecisionEngine` trait + `MockEngine` fallback (rule-based)
- **All 4 MCP tools working**:
  - `mimir_retain` — classify, score, store memory
  - `mimir_recall` — hybrid FTS + LIKE search with importance ranking
  - `mimir_reflect` — answer from memories with provenance
  - `mimir_ask` — query another agent's bank (read-only)
- **End-to-end verified**: initialize → tools/list → tools/call all respond correctly

### Technical Notes
- Used `rmcp` 0.1.5 with `server`, `macros`, `transport-io` features
- `tool_box!` macro generates `list_tools`/`call_tool` from `#[tool]` annotated methods
- `MockEngine` uses keyword heuristics for classification/importance (placeholder for Kev-0.8B)
- FTS5 virtual table for full-text search, falls back to LIKE
- Database at `~/.mimir/mimir.db` with WAL mode

### Deferred
- Kev-0.8B ONNX integration (stubbed, `ort` API unstable)
- Night shift Qwen2.5-7B (llama.cpp bindings not integrated)
- Web UI (Leptos/SvelteKit choice pending)
- REST API endpoints
- OpenClaw plugin registration

---

## 2026-10-08: Code Review & Cleanup

### Issues Found & Fixed

| Severity | Issue | Fix |
|----------|-------|-----|
| **Critical** | FTS table schema mismatch — memories used TEXT UUID PK, FTS5 needs INTEGER rowid | Added explicit `rowid INTEGER PRIMARY KEY AUTOINCREMENT` to memories table |
| **Critical** | FTS insert/query used wrong rowid reference | Updated all SQL to use explicit rowid column |
| **High** | `mimir_ask` ignored permissions | Added proper `can_read()` check via agents module |
| **High** | Memory insert + FTS insert not transactional | Wrapped in `unchecked_transaction()` |
| **High** | No input validation on MCP tools | Added empty content check, 100K char limit, permission check on retain |
| **Medium** | Missing `can_read`/`can_write` wrappers on Db | Added to `db/mod.rs` |

### Warnings Suppressed
- Added `#![allow(dead_code)]` to main.rs — most warnings were unused methods on intentionally-complete APIs

### Build Status
- `cargo build`: ✅ zero errors, zero warnings
- `cargo test`: ✅ 6/6 passing

### Remaining Known Issues (for later)
| Issue | Priority | Notes |
|-------|----------|-------|
| Kev heuristics too naive | Medium | Will improve when real model is loaded |
| No connection pooling | Low | Single Mutex'd connection is fine for v1 |
| Hardcoded agent_id "default" | Low | Should derive from MCP session context |
| Bank type hardcoded to "private" | Medium | Need shared bank creation path |
| Recall stats update on failed query | Low | Minor, cosmetic |
