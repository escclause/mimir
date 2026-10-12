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
│  DAY — D1-OMNI-600M (System One)        │
│  ├── Classify: world fact? experience?  │
│  ├── Score importance (1-5)             │
│  ├── Route to appropriate bank          │
│  └── Log all decisions + confidence     │
│                                         │
│  NIGHT — LFM2.5-2.6B (System Two)       │
│  ├── Audit day's D1 decisions           │
│  ├── Consolidate related memories       │
│  ├── Build mental models                │
│  ├── Repair contradictions              │
│  └── Decay stale memories               │
│                                         │
│  STORAGE — SQLite + FTS5                │
│  ├── Per-agent banks (write)            │
│  ├── Shared bank (read-only)            │
│  └── Full-text search with stemming     │
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
| **Hallucination** | LLM-based memory makes things up | D1 makes *constrained* judgments — no free generation |
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
- Optional: [llama.cpp](https://github.com/ggerganov/llama.cpp) for D1/night shift models

### Build & Run

```bash
# Clone
git clone https://github.com/escclause/mimir.git
cd mimir

# Build
cargo build --release

# Run (MCP server over stdio + REST API + Web UI)
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

Or install as an OpenClaw plugin (auto-discovers tools):

```bash
openclaw plugins install /path/to/mimir/openclaw-plugin
```

### MCP Tools

| Tool | Description |
|------|-------------|
| `mimir_retain` | Store a memory (classified, scored, routed) |
| `mimir_recall` | Retrieve relevant memories |
| `mimir_reflect` | Answer from memories with provenance |
| `mimir_ask` | Query another agent's bank (read-only) |

## Web UI

Mímir includes an embedded web interface for browsing and managing memories.

**Access:** `http://127.0.0.1:8083` (when Mímir is running)

**Features:**
- Browse memories by bank
- Search across all banks (FTS with stemming)
- Ask questions (reflect) with sourced answers
- Add memories manually
- Delete memories
- View stats (total memories, banks, types)

### OpenClaw Portal Integration

To add Mímir to your OpenClaw Control UI sidebar:

1. Start Mímir (so the Web UI is running)
2. In any OpenClaw session, run:
   ```
   portal(action="open", port=8083, title="Mímir Memory", description="Browse and manage agent memories")
   ```
3. A "Mímir Memory" link appears in Control UI → click to open the Web UI

**Note:** The portal link only works while Mímir is running. If you restart Mímir, re-run the portal command.

## REST API

Mímir exposes a REST API on `http://127.0.0.1:8083` for external tool integration.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/banks` | GET | List all banks |
| `/banks` | POST | Create new bank |
| `/banks/:name` | GET | Get bank details |
| `/banks/:name/memories` | GET | List memories (with `?limit=`) |
| `/banks/:name/memories` | POST | Store new memory |
| `/banks/:name/memories/:id` | GET | Get specific memory |
| `/banks/:name/memories/:id` | DELETE | Delete memory |
| `/banks/:name/search` | POST | FTS search memories |
| `/reflect` | POST | Generate answer from memories |

### Example: Store a Memory

```bash
curl -X POST http://127.0.0.1:8083/banks/my_bank/memories \
  -H "Content-Type: application/json" \
  -d '{"content": "I prefer dark mode interfaces"}'
```

### Example: Search Memories

```bash
curl -X POST http://127.0.0.1:8083/banks/my_bank/search \
  -H "Content-Type: application/json" \
  -d '{"query": "dark mode", "limit": 5}'
```

## Configuration

Configuration is stored in SQLite and editable via the Web UI (coming soon). Defaults:

| Setting | Default | Description |
|---------|---------|-------------|
| REST Port | 8083 | HTTP server port |
| D1 Model | d1-omni-600m | Day-shift decision engine |
| D1 Port | 8081 | llama-server port for D1 |
| Night Model | lfm2.5-2.6b | Night-shift synthesis engine |
| Night Port | 8082 | llama-server port for night shift |
| Night Schedule | 0 2 * * * | Cron schedule (daily at 2am) |
| Storage Path | ~/.mimir | Data directory |

## Project Status

**v1.0 — Feature complete.** All core features implemented and tested.

### Implemented

- [x] Database schema + migrations
- [x] MCP server (4 tools)
- [x] D1 day-shift engine (llama.cpp)
- [x] Night shift engine (LFM2.5-2.6B)
- [x] Night shift scheduler (cron)
- [x] Deduplication
- [x] Shared banks
- [x] REST API
- [x] Web UI
- [x] OpenClaw plugin

### Roadmap

- [ ] Config persistence (currently hardcoded defaults)
- [ ] Memory export/import
- [ ] LoRA fine-tuning pipeline
- [ ] Multi-user support
- [ ] Cloud sync (optional)

## License

MIT — see [LICENSE](LICENSE)

## Contributing

Contributions welcome. Please open an issue first to discuss what you'd like to change.

## Acknowledgments

- [Liquid AI](https://www.liquid.ai/) — LFM2.5 models (D1-OMNI, LFM2.5-2.6B)
- [llama.cpp](https://github.com/ggerganov/llama.cpp) — Local inference engine
- [Hindsight](https://github.com/vectorize-io/hindsight) — Mental models inspiration
- [Jev-Mem paper](https://arxiv.org/abs/2609.23986) — System-One-controlled memory architecture

---

*"Mímir speaks wisely, Mímir measures all things."* — Völuspá
