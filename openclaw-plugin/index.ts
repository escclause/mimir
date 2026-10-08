import { defineToolPlugin } from "openclaw/plugin-sdk/tool-plugin";
import { Type } from "@sinclair/typebox";
import { spawn, ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";

// ---------------------------------------------------------------------------
// Mímir MCP client over stdio
// ---------------------------------------------------------------------------

interface MimirConfig {
  binaryPath?: string;
  databasePath?: string;
  defaultBank?: string;
  defaultAgent?: string;
}

class MimirClient {
  private process: ChildProcess | null = null;
  private requestId = 0;
  private pending = new Map<
    number,
    { resolve: (v: unknown) => void; reject: (e: Error) => void }
  >();
  private buffer = "";
  private ready = false;
  private readyResolve!: () => void;
  private readyPromise: Promise<void>;
  private config: MimirConfig;

  constructor(config: MimirConfig) {
    this.config = config;
    this.readyPromise = new Promise((r) => (this.readyResolve = r));
  }

  private resolveBinary(): string {
    if (this.config.binaryPath && existsSync(this.config.binaryPath)) {
      return this.config.binaryPath;
    }
    // Check common locations
    const candidates = [
      join(homedir(), ".mimir", "bin", "mimir"),
      join(homedir(), ".local", "bin", "mimir"),
      "/usr/local/bin/mimir",
      "/usr/bin/mimir",
    ];
    for (const p of candidates) {
      if (existsSync(p)) return p;
    }
    // Fall back to PATH
    return "mimir";
  }

  async start(): Promise<void> {
    if (this.process) return;

    const binary = this.resolveBinary();
    const args: string[] = [];

    // Pass database path if configured
    if (this.config.databasePath) {
      args.push("--db", this.config.databasePath);
    }

    this.process = spawn(binary, args, {
      stdio: ["pipe", "pipe", "pipe"],
    });

    this.process.stdout!.on("data", (chunk: Buffer) => {
      this.buffer += chunk.toString("utf-8");
      this.drainBuffer();
    });

    this.process.stderr!.on("data", (chunk: Buffer) => {
      console.error("[mimir stderr]", chunk.toString("utf-8"));
    });

    this.process.on("exit", (code) => {
      this.process = null;
      this.ready = false;
      const err = new Error(`Mímir process exited with code ${code}`);
      for (const [, pending] of this.pending) {
        pending.reject(err);
      }
      this.pending.clear();
    });

    // Wait for the server to be ready by sending initialize
    await this.sendRequest("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "openclaw-mimir-plugin", version: "0.1.0" },
    });

    // Send initialized notification
    this.sendNotification("notifications/initialized", {});
    this.ready = true;
    this.readyResolve();
  }

  private drainBuffer() {
    let idx: number;
    while ((idx = this.buffer.indexOf("\n")) >= 0) {
      const line = this.buffer.slice(0, idx).trim();
      this.buffer = this.buffer.slice(idx + 1);
      if (!line) continue;
      try {
        const msg = JSON.parse(line);
        if (msg.id !== undefined && this.pending.has(msg.id)) {
          const { resolve, reject } = this.pending.get(msg.id)!;
          this.pending.delete(msg.id);
          if (msg.error) {
            reject(new Error(msg.error.message || "MCP error"));
          } else {
            resolve(msg.result);
          }
        }
      } catch {
        // Ignore non-JSON lines
      }
    }
  }

  private sendNotification(method: string, params: unknown): void {
    if (!this.process?.stdin) return;
    const msg = JSON.stringify({ jsonrpc: "2.0", method, params }) + "\n";
    this.process.stdin.write(msg);
  }

  private async sendRequest(method: string, params: unknown): Promise<unknown> {
    if (!this.process?.stdin) {
      throw new Error("Mímir process not running");
    }
    const id = ++this.requestId;
    const msg = JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n";

    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.process!.stdin!.write(msg);
    });
  }

  async callTool(name: string, args: Record<string, unknown>): Promise<unknown> {
    await this.start();
    const result = (await this.sendRequest("tools/call", {
      name,
      arguments: args,
    })) as { content?: Array<{ type: string; text?: string }> };

    // Extract JSON from content
    if (result.content && result.content.length > 0) {
      const text = result.content[0].text;
      if (text) {
        try {
          return JSON.parse(text);
        } catch {
          return text;
        }
      }
    }
    return result;
  }

  async stop(): Promise<void> {
    if (this.process) {
      this.process.kill("SIGTERM");
      this.process = null;
    }
  }
}

// ---------------------------------------------------------------------------
// Plugin singleton client
// ---------------------------------------------------------------------------

let client: MimirClient | null = null;

function getClient(config: MimirConfig): MimirClient {
  if (!client) {
    client = new MimirClient(config);
  }
  return client;
}

// ---------------------------------------------------------------------------
// Plugin definition
// ---------------------------------------------------------------------------

export default defineToolPlugin({
  id: "mimir",
  name: "Mímir Memory",
  description:
    "Norse-themed agent memory system with day/night cognition. Store, recall, and reflect on memories across isolated per-agent banks.",

  configSchema: Type.Object({
    binaryPath: Type.Optional(Type.String()),
    databasePath: Type.Optional(Type.String()),
    defaultBank: Type.Optional(Type.String()),
    defaultAgent: Type.Optional(Type.String()),
  }),

  tools: (tool) => [
    // ------------------------------------------------------- mimir_retain
    tool({
      name: "mimir_retain",
      label: "Mímir Retain",
      description:
        "Store a memory in Mímir. Content is classified (world fact / experience / observation / mental model), scored for importance, and routed to the appropriate memory bank.",
      parameters: Type.Object({
        content: Type.String({
          description: "The memory content to store",
        }),
        context: Type.Optional(
          Type.String({
            description: "Optional context about the memory (source, situation)",
          })
        ),
        bank: Type.Optional(
          Type.String({
            description: "Target memory bank (default: agent's default bank)",
          })
        ),
        agent: Type.Optional(
          Type.String({
            description: "Agent ID (default: 'default')",
          })
        ),
      }),
      execute: async (params, config) => {
        const c = getClient(config);
        return c.callTool("mimir_retain", {
          content: params.content,
          context: params.context,
          bank: params.bank ?? config.defaultBank,
          agent: params.agent ?? config.defaultAgent,
        });
      },
    }),

    // ------------------------------------------------------- mimir_recall
    tool({
      name: "mimir_recall",
      label: "Mímir Recall",
      description:
        "Retrieve relevant memories from Mímir using hybrid search (full-text search + importance ranking). Returns ranked, pre-filtered memories.",
      parameters: Type.Object({
        query: Type.String({
          description: "Search query to find relevant memories",
        }),
        bank: Type.Optional(
          Type.String({
            description: "Memory bank to search (default: agent's default bank)",
          })
        ),
        limit: Type.Optional(
          Type.Number({
            description: "Maximum number of results (default: 10)",
          })
        ),
      }),
      execute: async (params, config) => {
        const c = getClient(config);
        return c.callTool("mimir_recall", {
          query: params.query,
          bank: params.bank ?? config.defaultBank,
          limit: params.limit,
        });
      },
    }),

    // ------------------------------------------------------- mimir_reflect
    tool({
      name: "mimir_reflect",
      label: "Mímir Reflect",
      description:
        "Generate an answer from memories with provenance. Retrieves relevant memories and synthesizes a response with source attribution.",
      parameters: Type.Object({
        query: Type.String({
          description: "Question to answer from stored memories",
        }),
        bank: Type.Optional(
          Type.String({
            description: "Memory bank to reflect on (default: agent's default bank)",
          })
        ),
      }),
      execute: async (params, config) => {
        const c = getClient(config);
        return c.callTool("mimir_reflect", {
          query: params.query,
          bank: params.bank ?? config.defaultBank,
        });
      },
    }),

    // ------------------------------------------------------- mimir_ask
    tool({
      name: "mimir_ask",
      label: "Mímir Ask",
      description:
        "Query another agent's memory bank (read-only). Requires read permission on the target bank.",
      parameters: Type.Object({
        agent: Type.String({
          description: "Agent whose bank to query",
        }),
        query: Type.String({
          description: "Search query",
        }),
        limit: Type.Optional(
          Type.Number({
            description: "Maximum results (default: 5)",
          })
        ),
      }),
      execute: async (params, _config) => {
        const c = getClient(_config);
        return c.callTool("mimir_ask", {
          agent: params.agent,
          query: params.query,
          limit: params.limit,
        });
      },
    }),
  ],
});
