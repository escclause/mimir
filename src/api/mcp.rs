// MCP server for OpenClaw integration
// Serves mimir_retain, mimir_recall, mimir_reflect, mimir_ask over stdio

use std::sync::Arc;

use anyhow::Result;
use rmcp::handler::server::tool::{ToolCallContext, ToolBox};
use rmcp::model::{
    CallToolRequestParam, CallToolResult, Content, Implementation, ListToolsResult,
    PaginatedRequestParam, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::service::{Peer, RequestContext, RoleServer};
use rmcp::{tool, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::db::memories::NewMemory;
use crate::db::Db;
use crate::jev::mock::MockEngine;
use crate::jev::DecisionEngine;

pub struct MimirServer {
    db: Arc<Db>,
    engine: Arc<dyn DecisionEngine>,
    peer: Option<Peer<RoleServer>>,
}

impl std::fmt::Debug for MimirServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MimirServer").finish_non_exhaustive()
    }
}

impl Clone for MimirServer {
    fn clone(&self) -> Self {
        Self {
            db: Arc::clone(&self.db),
            engine: Arc::clone(&self.engine),
            peer: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RetainInput {
    /// Content to remember
    pub content: String,
    /// Optional context about the memory
    #[serde(default)]
    pub context: Option<String>,
    /// Target bank (default: agent's default bank)
    #[serde(default)]
    pub bank: Option<String>,
    /// Agent ID (default: "default")
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RecallInput {
    /// Query to search for
    pub query: String,
    /// Bank to search (default: agent's default bank)
    #[serde(default)]
    pub bank: Option<String>,
    /// Maximum results (default: 10)
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReflectInput {
    /// Question to answer from memories
    pub query: String,
    /// Bank to reflect on (default: agent's default bank)
    #[serde(default)]
    pub bank: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AskInput {
    /// Agent whose bank to query
    pub agent: String,
    /// Query to search for
    pub query: String,
    /// Maximum results (default: 5)
    #[serde(default)]
    pub limit: Option<usize>,
}

impl MimirServer {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            engine: Arc::new(MockEngine::new()),
            peer: None,
        }
    }

    /// Store a memory in the agent's bank
    #[tool(
        name = "mimir_retain",
        description = "Store a memory in Mímir. Content is classified, scored, and routed to the appropriate memory bank."
    )]
    pub async fn mimir_retain(
        &self,
        #[tool(aggr)] input: RetainInput,
    ) -> Result<Content, String> {
        // Validate input
        if input.content.trim().is_empty() {
            return Err("content cannot be empty".to_string());
        }
        if input.content.len() > 100_000 {
            return Err("content exceeds maximum length of 100,000 characters".to_string());
        }

        let agent_id = input.agent.unwrap_or_else(|| "default".to_string());
        let bank_name = input.bank.unwrap_or_else(|| format!("{}_bank", agent_id));

        // Get or create bank
        let bank = self
            .db
            .get_or_create_bank(&bank_name, &agent_id)
            .map_err(|e| format!("failed to get/create bank: {}", e))?;

        // Check write permission
        let can_write = self
            .db
            .can_write(&agent_id, &bank.id)
            .map_err(|e| format!("permission check failed: {}", e))?;

        if !can_write {
            return Err(format!(
                "agent '{}' does not have write permission for bank '{}'",
                agent_id, bank.name
            ));
        }

        // Classify with Jev
        let classification = self
            .engine
            .classify(&input.content)
            .map_err(|e| format!("classification failed: {}", e))?;

        // Score importance
        let importance = self
            .engine
            .score_importance(&input.content)
            .map_err(|e| format!("importance scoring failed: {}", e))?;

        // Store memory
        let memory = self
            .db
            .store_memory(
                &bank.id,
                NewMemory {
                    content: input.content.clone(),
                    memory_type: classification.memory_type.as_str().to_string(),
                    importance: importance.level,
                    confidence: classification.confidence,
                    source_agent: agent_id.clone(),
                    source_context: input.context.clone(),
                    provenance: None,
                },
            )
            .map_err(|e| format!("failed to store memory: {}", e))?;

        // Log the decision
        let _ = self.db.log_jev_decision(
            Some(&memory.id),
            "classify",
            &input.content,
            classification.memory_type.as_str(),
            Some(classification.confidence),
            "mock",
        );

        let response = serde_json::json!({
            "id": memory.id,
            "bank": bank.name,
            "memory_type": classification.memory_type.as_str(),
            "importance": importance.level,
            "confidence": classification.confidence,
        });

        Content::json(response).map_err(|e| format!("failed to serialize response: {}", e))
    }

    /// Recall memories from a bank
    #[tool(
        name = "mimir_recall",
        description = "Recall relevant memories from Mímir. Uses hybrid search (FTS + importance ranking)."
    )]
    pub async fn mimir_recall(
        &self,
        #[tool(aggr)] input: RecallInput,
    ) -> Result<Content, String> {
        let bank_name = input.bank.unwrap_or_else(|| "default_bank".to_string());
        let limit = input.limit.unwrap_or(10);

        let bank = self
            .db
            .get_bank_by_name(&bank_name)
            .map_err(|e| format!("failed to lookup bank: {}", e))?
            .ok_or_else(|| format!("bank not found: {}", bank_name))?;

        let memories = self
            .db
            .recall_memories(&bank.id, &input.query, limit)
            .map_err(|e| format!("recall failed: {}", e))?;

        let results: Vec<serde_json::Value> = memories
            .iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "content": m.content,
                    "memory_type": m.memory_type,
                    "importance": m.importance,
                    "confidence": m.confidence,
                    "created_at": m.created_at,
                })
            })
            .collect();

        let response = serde_json::json!({
            "query": input.query,
            "bank": bank.name,
            "count": results.len(),
            "memories": results,
        });

        Content::json(response).map_err(|e| format!("failed to serialize response: {}", e))
    }

    /// Reflect on memories to answer a question
    #[tool(
        name = "mimir_reflect",
        description = "Generate an answer from memories with provenance. Retrieves relevant memories and synthesizes a response."
    )]
    pub async fn mimir_reflect(
        &self,
        #[tool(aggr)] input: ReflectInput,
    ) -> Result<Content, String> {
        let bank_name = input.bank.unwrap_or_else(|| "default_bank".to_string());

        let bank = self
            .db
            .get_bank_by_name(&bank_name)
            .map_err(|e| format!("failed to lookup bank: {}", e))?
            .ok_or_else(|| format!("bank not found: {}", bank_name))?;

        // Get all memories for reflection
        let memories = self
            .db
            .all_memories(&bank.id)
            .map_err(|e| format!("failed to retrieve memories: {}", e))?;

        // Simple relevance: use recall for query-based filtering
        let relevant = self
            .db
            .recall_memories(&bank.id, &input.query, 20)
            .map_err(|e| format!("recall failed: {}", e))?;

        // Build reflection with provenance
        let sources: Vec<serde_json::Value> = relevant
            .iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "content": m.content,
                    "memory_type": m.memory_type,
                    "importance": m.importance,
                    "confidence": m.confidence,
                })
            })
            .collect();

        let total_memories = memories.len();

        let response = serde_json::json!({
            "query": input.query,
            "bank": bank.name,
            "total_memories_in_bank": total_memories,
            "relevant_memories": relevant.len(),
            "answer": format!(
                "Based on {} relevant memories from bank '{}' ({} total memories):",
                relevant.len(),
                bank.name,
                total_memories
            ),
            "sources": sources,
        });

        Content::json(response).map_err(|e| format!("failed to serialize response: {}", e))
    }

    /// Ask another agent's bank (read-only)
    #[tool(
        name = "mimir_ask",
        description = "Query another agent's memory bank (read-only). Requires read permission."
    )]
    pub async fn mimir_ask(
        &self,
        #[tool(aggr)] input: AskInput,
    ) -> Result<Content, String> {
        let limit = input.limit.unwrap_or(5);

        // Find the agent's bank
        let banks = self
            .db
            .list_banks_for_agent(&input.agent)
            .map_err(|e| format!("failed to list banks: {}", e))?;

        if banks.is_empty() {
            return Err(format!("no banks found for agent: {}", input.agent));
        }

        // Use the first (default) bank
        let bank = &banks[0];

        // Check read permission using the agents module
        let permitted = self
            .db
            .can_read(&input.agent, &bank.id)
            .map_err(|e| format!("permission check failed: {}", e))?;

        if !permitted {
            return Err(format!(
                "agent '{}' does not have read permission for bank '{}'",
                input.agent, bank.name
            ));
        }

        let memories = self
            .db
            .recall_memories(&bank.id, &input.query, limit)
            .map_err(|e| format!("recall failed: {}", e))?;

        let results: Vec<serde_json::Value> = memories
            .iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "content": m.content,
                    "memory_type": m.memory_type,
                    "importance": m.importance,
                    "confidence": m.confidence,
                    "created_at": m.created_at,
                })
            })
            .collect();

        let response = serde_json::json!({
            "agent": input.agent,
            "bank": bank.name,
            "query": input.query,
            "count": results.len(),
            "memories": results,
        });

        Content::json(response).map_err(|e| format!("failed to serialize response: {}", e))
    }
}

// Build the tool box manually
fn mimir_tool_box() -> &'static ToolBox<MimirServer> {
    use rmcp::handler::server::tool::ToolBoxItem;
    use std::sync::OnceLock;

    static TOOL_BOX: OnceLock<ToolBox<MimirServer>> = OnceLock::new();
    TOOL_BOX.get_or_init(|| {
        let mut tb = ToolBox::new();
        tb.add(ToolBoxItem::new(
            MimirServer::mimir_retain_tool_attr(),
            |ctx| Box::pin(MimirServer::mimir_retain_tool_call(ctx)),
        ));
        tb.add(ToolBoxItem::new(
            MimirServer::mimir_recall_tool_attr(),
            |ctx| Box::pin(MimirServer::mimir_recall_tool_call(ctx)),
        ));
        tb.add(ToolBoxItem::new(
            MimirServer::mimir_reflect_tool_attr(),
            |ctx| Box::pin(MimirServer::mimir_reflect_tool_call(ctx)),
        ));
        tb.add(ToolBoxItem::new(
            MimirServer::mimir_ask_tool_attr(),
            |ctx| Box::pin(MimirServer::mimir_ask_tool_call(ctx)),
        ));
        tb
    })
}

impl ServerHandler for MimirServer {
    async fn list_tools(
        &self,
        _: PaginatedRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::Error> {
        Ok(ListToolsResult {
            next_cursor: None,
            tools: mimir_tool_box().list(),
        })
    }

    async fn call_tool(
        &self,
        call_tool_request_param: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::Error> {
        let ctx = ToolCallContext::new(self, call_tool_request_param, context);
        mimir_tool_box().call(ctx).await
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "mimir".to_string(),
                version: "0.1.0".to_string(),
            },
            instructions: Some(
                "Mímir - Norse-themed agent memory system. Use mimir_retain to store memories, mimir_recall to retrieve, mimir_reflect to answer from memories, mimir_ask to query other agents."
                    .to_string(),
            ),
        }
    }

    fn get_peer(&self) -> Option<Peer<RoleServer>> {
        self.peer.clone()
    }

    fn set_peer(&mut self, peer: Peer<RoleServer>) {
        self.peer = Some(peer);
    }
}
