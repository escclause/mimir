// MCP server for OpenClaw integration
// Serves mimir_retain, mimir_recall, mimir_reflect, mimir_ask over stdio

use std::path::PathBuf;
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
use crate::jev::d1::D1Engine;
use crate::jev::mock::MockEngine;
use crate::jev::DecisionEngine;

/// Find llama-server binary in common locations.
/// Prefers official llama.cpp build for D1 model support.
fn find_llama_server() -> Option<PathBuf> {
    // Check official build first (required for D1 LFM2.5 architecture)
    let candidates = [
        "/home/glitch/llama.cpp-official/build/bin/llama-server",
        "/home/glitch/llama.cpp-turboquant/build/bin/llama-server",
        "/usr/local/bin/llama-server",
        "/usr/bin/llama-server",
    ];

    for path in &candidates {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // Check PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let p = PathBuf::from(dir).join("llama-server");
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}

/// Engine kind label used in Jev decision logs.
#[derive(Debug, Clone, Copy)]
pub enum EngineKind {
    D1,
    Mock,
}

impl EngineKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EngineKind::D1 => "d1-omni-600m",
            EngineKind::Mock => "mock",
        }
    }
}

pub struct MimirServer {
    db: Arc<Db>,
    engine: Arc<dyn DecisionEngine>,
    engine_kind: EngineKind,
    peer: Option<Peer<RoleServer>>,
    /// Shared engine slot for background D1 upgrade
    engine_slot: Arc<std::sync::RwLock<(Arc<dyn DecisionEngine>, EngineKind)>>,
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
            engine_kind: self.engine_kind,
            peer: None,
            engine_slot: Arc::clone(&self.engine_slot),
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
    /// Agent ID (default: "default")
    #[serde(default)]
    pub agent: Option<String>,
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
    /// Agent ID (default: "default")
    #[serde(default)]
    pub agent: Option<String>,
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
    /// Create a new MimirServer with MockEngine, then spawn background
    /// thread to upgrade to D1 engine when ready.
    pub fn new(db: Arc<Db>) -> Self {
        let config = crate::config::Config::default();
        let mock_engine: Arc<dyn DecisionEngine> = Arc::new(MockEngine::new());
        let engine_slot = Arc::new(std::sync::RwLock::new((
            Arc::clone(&mock_engine),
            EngineKind::Mock,
        )));

        // Check if D1 model exists, spawn background upgrade
        if D1Engine::model_exists(&config.kev_model_path, &config.d1_mmproj_path) {
            let server_binary = find_llama_server().unwrap_or_else(|| {
                PathBuf::from("/home/glitch/llama.cpp-official/build/bin/llama-server")
            });

            if server_binary.exists() {
                let engine_slot_clone = Arc::clone(&engine_slot);
                let model_path = config.kev_model_path.clone();
                let mmproj_path = config.d1_mmproj_path.clone();
                let port = config.kev_port;

                std::thread::spawn(move || {
                    tracing::info!("Background: starting D1 engine...");
                    match D1Engine::spawn(
                        &model_path,
                        &mmproj_path,
                        &server_binary,
                        port,
                    ) {
                        Ok(d1) => {
                            tracing::info!("Background: D1 engine ready, upgrading");
                            if let Ok(mut slot) = engine_slot_clone.write() {
                                *slot = (Arc::new(d1), EngineKind::D1);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Background: D1 engine failed to start");
                        }
                    }
                });
            } else {
                tracing::warn!("llama-server not found, staying with MockEngine");
            }
        } else {
            tracing::info!("D1 model not found, using MockEngine");
        }

        Self {
            db,
            engine: mock_engine,
            engine_kind: EngineKind::Mock,
            peer: None,
            engine_slot,
        }
    }

    /// Create a MimirServer with an explicit engine (for testing).
    #[allow(dead_code)]
    pub fn with_engine(db: Arc<Db>, engine: Arc<dyn DecisionEngine>, kind: EngineKind) -> Self {
        let engine_slot = Arc::new(std::sync::RwLock::new((
            Arc::clone(&engine),
            kind,
        )));
        Self {
            db,
            engine,
            engine_kind: kind,
            peer: None,
            engine_slot,
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

        // Get current engine from slot (may have been upgraded to D1)
        let (engine, engine_kind) = {
            match self.engine_slot.read() {
                Ok(slot) => (Arc::clone(&slot.0), slot.1),
                Err(_) => (Arc::clone(&self.engine), self.engine_kind),
            }
        };

        // Classify with Jev
        let classification = engine
            .classify(&input.content)
            .map_err(|e| format!("classification failed: {}", e))?;

        // Score importance
        let importance = engine
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
            engine_kind.as_str(),
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
        let agent_id = input.agent.clone().unwrap_or_else(|| "default".to_string());
        let bank_name = input
            .bank
            .clone()
            .unwrap_or_else(|| format!("{}_bank", agent_id));
        let limit = input.limit.unwrap_or(10);

        let bank = self
            .db
            .get_bank_by_name(&bank_name)
            .map_err(|e| format!("failed to lookup bank: {}", e))?
            .ok_or_else(|| format!("bank not found: {}", bank_name))?;

        // Check read permission
        let can_read = self
            .db
            .can_read(&agent_id, &bank.id)
            .map_err(|e| format!("permission check failed: {}", e))?;

        if !can_read {
            return Err(format!(
                "agent '{}' does not have read permission for bank '{}'",
                agent_id, bank.name
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
        let agent_id = input.agent.clone().unwrap_or_else(|| "default".to_string());
        let bank_name = input
            .bank
            .clone()
            .unwrap_or_else(|| format!("{}_bank", agent_id));

        let bank = self
            .db
            .get_bank_by_name(&bank_name)
            .map_err(|e| format!("failed to lookup bank: {}", e))?
            .ok_or_else(|| format!("bank not found: {}", bank_name))?;

        // Check read permission
        let can_read = self
            .db
            .can_read(&agent_id, &bank.id)
            .map_err(|e| format!("permission check failed: {}", e))?;

        if !can_read {
            return Err(format!(
                "agent '{}' does not have read permission for bank '{}'",
                agent_id, bank.name
            ));
        }

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

// Build the tool box manually with explicit initialization
fn mimir_tool_box() -> &'static ToolBox<MimirServer> {
    use rmcp::handler::server::tool::ToolBoxItem;
    use std::sync::OnceLock;

    static TOOL_BOX: OnceLock<ToolBox<MimirServer>> = OnceLock::new();
    TOOL_BOX.get_or_init(|| {
        tracing::info!("initializing Mímir tool box");
        let mut tb = ToolBox::new();
        
        let retain_attr = MimirServer::mimir_retain_tool_attr();
        tracing::info!(tool = %retain_attr.name, "registering tool");
        tb.add(ToolBoxItem::new(
            retain_attr,
            |ctx| Box::pin(MimirServer::mimir_retain_tool_call(ctx)),
        ));
        
        let recall_attr = MimirServer::mimir_recall_tool_attr();
        tracing::info!(tool = %recall_attr.name, "registering tool");
        tb.add(ToolBoxItem::new(
            recall_attr,
            |ctx| Box::pin(MimirServer::mimir_recall_tool_call(ctx)),
        ));
        
        let reflect_attr = MimirServer::mimir_reflect_tool_attr();
        tracing::info!(tool = %reflect_attr.name, "registering tool");
        tb.add(ToolBoxItem::new(
            reflect_attr,
            |ctx| Box::pin(MimirServer::mimir_reflect_tool_call(ctx)),
        ));
        
        let ask_attr = MimirServer::mimir_ask_tool_attr();
        tracing::info!(tool = %ask_attr.name, "registering tool");
        tb.add(ToolBoxItem::new(
            ask_attr,
            |ctx| Box::pin(MimirServer::mimir_ask_tool_call(ctx)),
        ));
        
        tracing::info!(tool_count = tb.map.len(), "tool box initialized");
        tb
    })
}

impl ServerHandler for MimirServer {
    async fn list_tools(
        &self,
        _: PaginatedRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::Error> {
        let tb = mimir_tool_box();
        tracing::info!(tool_count = tb.map.len(), "list_tools called");
        Ok(ListToolsResult {
            next_cursor: None,
            tools: tb.list(),
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
