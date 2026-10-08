# Mímir OpenClaw Plugin

Official OpenClaw plugin for [Mímir](https://github.com/escclause/mimir) — Norse-themed agent memory system with day/night cognition.

## Install

```sh
openclaw plugins install @openclaw/mimir-plugin
```

## Prerequisites

Build the Mímir binary:

```sh
cd mimir
cargo build --release
# Binary at target/release/mimir
```

## Configure

Add to your OpenClaw config (`openclaw.json`):

```json
{
  "plugins": {
    "entries": {
      "mimir": {
        "enabled": true,
        "config": {
          "mimir": {
            "binaryPath": "/path/to/mimir/target/release/mimir",
            "defaultBank": "default_bank",
            "defaultAgent": "default"
          }
        }
      }
    }
  }
}
```

## Tools

| Tool | Description |
|------|-------------|
| `mimir_retain` | Store a memory (classified, scored, routed) |
| `mimir_recall` | Retrieve relevant memories (hybrid search) |
| `mimir_reflect` | Answer from memories with provenance |
| `mimir_ask` | Query another agent's bank (read-only) |

## Architecture

The plugin spawns the Mímir binary as a child process and communicates via MCP over stdio. Each tool call is bridged to the corresponding MCP tool on the Mímir server.

```
OpenClaw Agent → mimir_retain() → Plugin → MCP stdio → Mímir Binary → SQLite
```

## License

MIT
