# Mímir

**Norse-themed agent memory system with day/night cognition cycle.**

Mímir (pronounced "MEE-meer") is named after the Norse god of wisdom who guarded the well of knowledge. Like its namesake, Mímir remembers everything — but more importantly, it *understands* what it remembers.

## Architecture

Mímir separates memory into fast judgment (System One) and deep reasoning (System Two):

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

## Why Mímir?

| Problem | Existing Solutions | Mímir's Approach |
|---------|-------------------|------------------|
| **Hallucination** | LLM-based memory makes things up | Jev-type models make *constrained* judgments — no free generation |
| **Token bloat** | Dump full history into context | Pre-filtered, ranked, deduplicated memories |
| **No structure** | Flat vector search | Hierarchical banks + typed memories + provenance |
| **No learning** | Static after initial setup | Night shift audits and corrects day shift |
| **Vendor lock** | Cloud APIs required | Fully local, user-swappable models |

## Memory Types (Biomimetic)

| Type | Description | Example |
|------|-------------|---------|
| **World Fact** | Objective truth about the world | "The stove gets hot" |
| **Experience** | Agent's own experience | "I touched the stove and it hurt" |
| **Observation** | Consolidated, evidence-backed belief | "Stoves are dangerous when hot" |
| **Mental Model** | Standing answer to recurring question | "What are Q's preferences?" |

## Quick Start

### Prerequisites

- Rust 1.70+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- SQLite (bundled with rusqlite)

### Build & Run

```bash
# Clone
git clone https://github.com/escclause/mimir.git
cd mimir

# Build
cargo build --release

# Run (MCP server over stdio)
./target/release/mimir
```

### OpenClaw Integration

Add to your OpenClaw MCP config:

```json
{
  "mcpServers": {
    "mimir": {
      "command": "/path/to/mimir",
      "args": []
    }
  }
}
```

### MCP Tools

| Tool | Description |
|------|-------------|
| `mimir_retain` | Store a memory (classified, scored, routed) |
| `mimir_recall` | Retrieve relevant memories |
| `mimir_reflect` | Answer from memories with provenance |
| `mimir_ask` | Query another agent's bank (read-only) |

## Configuration

All configuration via Web UI (coming soon), stored in SQLite. No config files.

| Setting | Default | Description |
|---------|---------|-------------|
| Port | 8080 | HTTP server port |
| Kev Model | kev-0.8b | Jev backend |
| Night Model | qwen2.5-7b | Night shift LLM |
| Night Schedule | 0 2 * * * | Cron schedule |
| Storage Path | ~/.mimir | Data directory |

## Project Status

**Early development.** Core database, MCP server, and mock Jev engine are functional. Kev-0.8B ONNX integration is stubbed. Night shift not yet implemented.

### Roadmap

- [x] Database schema + migrations
- [x] MCP server (4 tools)
- [x] Mock Jev engine
- [ ] Kev-0.8B ONNX integration
- [ ] Night shift (Qwen2.5-7B)
- [ ] REST API
- [ ] Web UI
- [ ] OpenClaw plugin
- [ ] Import/export (MEMORY.md, conversation history)
- [ ] LoRA fine-tuning pipeline

## License

MIT — see [LICENSE](LICENSE)

## Contributing

Contributions welcome. Please open an issue first to discuss what you'd like to change.

## Acknowledgments

- [Kev](https://github.com/jaredpalmer/kev) — Jev-compatible decision models
- [Hindsight](https://github.com/vectorize-io/hindsight) — Mental models & knowledge pages inspiration
- [fast-memory](https://github.com/KonghaYao/fast-memory) — Jev+LLM memory validation
- [Jev-Mem paper](https://arxiv.org/abs/2609.23986) — System-One-controlled memory architecture

---

*"Mímir speaks wisely, Mímir measures all things."* — Völuspá
