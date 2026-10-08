# Mímir - Agent Memory System

Norse-themed, Rust-built, Jev-powered agent memory with day/night cognition cycle.

## Architecture

```
┌─────────────────────────────────────────┐
│           MÍMIR (Memory System)         │
├─────────────────────────────────────────┤
│                                         │
│  DAY — Kev-0.8B (System One)            │
│  ├── Classify: world fact? experience?  │
│  ├── Route to appropriate bank          │
│  ├── Score candidates                   │
│  ├── Judge: merge? contradict?          │
│  └── Log all decisions + confidence     │
│                                         │
│  NIGHT — Qwen2.5-7B (System Two)        │
│  ├── Audit day's Kev decisions          │
│  ├── Build mental models                │
│  ├── Write knowledge pages              │
│  ├── Generate markdown views            │
│  └── Log corrections for feedback       │
│                                         │
│  STORAGE — SQLite + Files               │
│  ├── Per-agent banks (write)            │
│  ├── Shared bank (read-only)            │
│  ├── Cold: JSONL transcripts            │
│  └── Views: Markdown (generated)        │
│                                         │
│  INTERFACE                              │
│  ├── MCP Server (OpenClaw integration)  │
│  ├── HTTP REST API                      │
│  └── Embedded Web UI                    │
│                                         │
└─────────────────────────────────────────┘
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust |
| Web Framework | Axum |
| Database | SQLite (rusqlite) |
| Scheduler | tokio-cron |
| ML Runtime | ONNX Runtime (Kev) + llama.cpp (Qwen) |
| MCP | rmcp or custom JSON-RPC |
| Web UI | Embedded static files |

## Project Structure

```
mimir/
├── Cargo.toml
├── src/
│   ├── main.rs           # Entry point
│   ├── config.rs         # SQLite-backed config
│   ├── db/
│   │   ├── mod.rs        # Connection, migrations
│   │   ├── banks.rs      # Bank CRUD
│   │   ├── memories.rs   # Memory storage, provenance
│   │   ├── agents.rs     # Agent permissions
│   │   └── jev_log.rs    # Decision logging
│   ├── jev/
│   │   ├── mod.rs        # DecisionEngine trait
│   │   ├── kev.rs        # Kev-0.8B via ONNX
│   │   └── mock.rs       # Rule-based fallback
│   ├── night/
│   │   ├── mod.rs        # Scheduler
│   │   ├── audit.rs      # Review day's decisions
│   │   ├── consolidate.rs# Build mental models
│   │   └── llm.rs        # Qwen2.5-7B via llama.cpp
│   ├── api/
│   │   ├── mod.rs        # Axum routes
│   │   ├── rest.rs       # HTTP endpoints
│   │   └── mcp.rs        # MCP server
│   └── ui/
│       └── mod.rs        # Serve embedded static files
├── ui/                   # Web UI source
│   └── dist/             # Built assets (embedded)
├── migrations/           # SQL migrations
└── docs/                 # Documentation
```

## Core Concepts

### Memory Types (biomimetic)

| Type | Description | Example |
|------|-------------|---------|
| **World Fact** | Objective truth about the world | "The stove gets hot" |
| **Experience** | Agent's own experience | "I touched the stove and it hurt" |
| **Observation** | Consolidated, evidence-backed belief | "Stoves are dangerous when hot" |
| **Mental Model** | Standing answer to recurring question | "What are Q's preferences?" |

### Memory Banks

Isolated memory stores per agent/user/project. Strict isolation, no cross-bank leakage by default.

### Operations

| Operation | Description |
|-----------|-------------|
| **retain** | Store new memory with Jev classification |
| **recall** | Retrieve relevant memories (hybrid search) |
| **reflect** | Generate answer from memories with provenance |

### Day/Night Cycle

| Phase | Actor | Function |
|-------|-------|----------|
| Day | Kev-0.8B | Fast judgments, routing, storage |
| Night | Qwen2.5-7B | Deep audit, consolidation, repair |

## MCP Tools

```json
[
  {
    "name": "mimir_retain",
    "description": "Store memory in agent's bank",
    "input": { "content": "string", "context?": "string", "bank?": "string" }
  },
  {
    "name": "mimir_recall",
    "description": "Retrieve relevant memories",
    "input": { "query": "string", "bank?": "string", "limit?": "number" }
  },
  {
    "name": "mimir_reflect",
    "description": "Answer from memories with provenance",
    "input": { "query": "string", "bank?": "string" }
  },
  {
    "name": "mimir_ask",
    "description": "Query another agent's bank (read-only)",
    "input": { "agent": "string", "query": "string" }
  }
]
```

## Configuration

All config via Web UI, stored in SQLite. No config files.

| Setting | Default | Description |
|---------|---------|-------------|
| Port | 8080 | HTTP server port |
| Kev Model | kev-0.8b | Jev backend |
| Night Model | qwen2.5-7b | Night shift LLM |
| Night Schedule | 0 2 * * * | Cron schedule |
| Storage Path | ~/.mimir | Data directory |

## License

MIT
