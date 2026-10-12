// REST API endpoints
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::sync::Arc;

use crate::db::memories::NewMemory;
use crate::db::Db;

/// Embedded UI assets
#[derive(RustEmbed)]
#[folder = "ui/dist/"]
struct UiAssets;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
}

/// Health check
async fn health() -> &'static str {
    "OK"
}

/// List all banks
async fn list_banks(State(state): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let banks = state.db.list_banks().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "banks": banks })))
}

/// Create a new bank
#[derive(Debug, Deserialize)]
pub struct CreateBankRequest {
    pub name: String,
    pub agent_id: Option<String>,
    pub bank_type: Option<String>,
}

async fn create_bank(
    State(state): State<AppState>,
    Json(req): Json<CreateBankRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let agent_id = req.agent_id.unwrap_or_else(|| "default".to_string());
    let bank_type = req.bank_type.unwrap_or_else(|| "private".to_string());

    let bank = state
        .db
        .create_bank(&req.name, &agent_id, &bank_type)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "bank": bank })))
}

/// Get bank details
async fn get_bank(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let bank = state
        .db
        .get_bank_by_name(&name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let count = state
        .db
        .count_memories(&bank.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "bank": bank,
        "memory_count": count
    })))
}

/// List memories in a bank
#[derive(Debug, Deserialize)]
pub struct ListMemoriesQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

async fn list_memories(
    State(state): State<AppState>,
    Path(bank_name): Path<String>,
    Query(query): Query<ListMemoriesQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let limit = query.limit.unwrap_or(50);
    let memories = state
        .db
        .recent_memories(&bank.id, limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "bank": bank_name,
        "count": memories.len(),
        "memories": memories
    })))
}

/// Search memories in a bank
#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<usize>,
}

async fn search_memories(
    State(state): State<AppState>,
    Path(bank_name): Path<String>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let limit = req.limit.unwrap_or(10);
    let memories = state
        .db
        .recall_memories(&bank.id, &req.query, limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "bank": bank_name,
        "query": req.query,
        "count": memories.len(),
        "memories": memories
    })))
}

/// Store a new memory
#[derive(Debug, Deserialize)]
pub struct StoreMemoryRequest {
    pub content: String,
    pub memory_type: Option<String>,
    pub importance: Option<u8>,
    pub confidence: Option<f32>,
    pub source_agent: Option<String>,
    pub source_context: Option<String>,
}

async fn store_memory(
    State(state): State<AppState>,
    Path(bank_name): Path<String>,
    Json(req): Json<StoreMemoryRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Check for duplicate
    if let Ok(Some(existing)) = state.db.find_duplicate(&bank.id, &req.content) {
        let _ = state.db.touch_memory(&existing.id);
        return Ok(Json(serde_json::json!({
            "duplicate": true,
            "memory": existing,
            "message": "Duplicate detected, recall count incremented"
        })));
    }

    let memory = state
        .db
        .store_memory(
            &bank.id,
            NewMemory {
                content: req.content,
                memory_type: req.memory_type.unwrap_or_else(|| "world_fact".to_string()),
                importance: req.importance.unwrap_or(3),
                confidence: req.confidence.unwrap_or(1.0),
                source_agent: req.source_agent.unwrap_or_else(|| "api".to_string()),
                source_context: req.source_context,
                provenance: None,
            },
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "duplicate": false,
        "memory": memory
    })))
}

/// Get a specific memory by ID
async fn get_memory(
    State(state): State<AppState>,
    Path((bank_name, memory_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let memory = state
        .db
        .get_memory(&memory_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Verify memory belongs to the specified bank
    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if memory.bank_id != bank.id {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(serde_json::json!({ "memory": memory })))
}

/// Delete a memory
async fn delete_memory(
    State(state): State<AppState>,
    Path((bank_name, memory_id)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    let memory = state
        .db
        .get_memory(&memory_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if memory.bank_id != bank.id {
        return Err(StatusCode::NOT_FOUND);
    }

    state
        .db
        .delete_memory(&memory_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Get all memories across all banks (for reflection/ask)
#[derive(Debug, Deserialize)]
pub struct ReflectRequest {
    pub query: String,
    pub bank: Option<String>,
}

async fn reflect(
    State(state): State<AppState>,
    Json(req): Json<ReflectRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let bank_name = req.bank.unwrap_or_else(|| "default_bank".to_string());

    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let memories = state
        .db
        .all_memories(&bank.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let relevant = state
        .db
        .recall_memories(&bank.id, &req.query, 20)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let answer = if relevant.is_empty() {
        format!(
            "No relevant memories found for query '{}' in bank '{}' ({} total memories).",
            req.query,
            bank_name,
            memories.len()
        )
    } else {
        let context: Vec<String> = relevant
            .iter()
            .enumerate()
            .map(|(i, m)| format!("{}. {} (type: {}, importance: {})", i + 1, m.content, m.memory_type, m.importance))
            .collect();
        format!(
            "Based on {} relevant memories from bank '{}' ({} total memories):\n\n{}",
            relevant.len(),
            bank_name,
            memories.len(),
            context.join("\n")
        )
    };

    Ok(Json(serde_json::json!({
        "query": req.query,
        "bank": bank_name,
        "total_memories": memories.len(),
        "relevant_count": relevant.len(),
        "answer": answer,
        "sources": relevant
    })))
}

/// Serve embedded UI assets
async fn serve_ui(Path(path): Path<String>) -> Result<axum::response::Response, StatusCode> {
    let path = if path.is_empty() { "index.html" } else { &path };
    
    match UiAssets::get(path) {
        Some(content) => {
            let mime = match path.rsplit('.').next() {
                Some("html") => "text/html",
                Some("js") => "application/javascript",
                Some("css") => "text/css",
                Some("json") => "application/json",
                Some("png") => "image/png",
                Some("svg") => "image/svg+xml",
                _ => "application/octet-stream",
            };
            
            Ok(axum::response::Response::builder()
                .header("content-type", mime)
                .body(axum::body::Body::from(content.data))
                .unwrap())
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Build the REST API router
pub fn router(db: Arc<Db>) -> Router {
    let state = AppState { db };

    Router::new()
        .route("/", get(|| async { serve_ui(Path("".to_string())).await }))
        .route("/*path", get(serve_ui))
        .route("/health", get(health))
        .route("/banks", get(list_banks).post(create_bank))
        .route("/banks/:name", get(get_bank))
        .route("/banks/:name/memories", get(list_memories).post(store_memory))
        .route("/banks/:name/memories/:id", get(get_memory).delete(delete_memory))
        .route("/banks/:name/search", post(search_memories))
        .route("/reflect", post(reflect))
        .with_state(state)
}
