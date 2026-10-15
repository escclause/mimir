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
    let offset = query.offset.unwrap_or(0);
    let memories = state
        .db
        .recent_memories_with_offset(&bank.id, limit, offset)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "bank": bank_name,
        "count": memories.len(),
        "offset": offset,
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
    // Input validation
    if req.content.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.content.len() > 100_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Permission check: derive agent from request or use "api" default
    let agent_id = req.source_agent.clone().unwrap_or_else(|| "api".to_string());
    let can_write = state
        .db
        .can_write(&agent_id, &bank.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !can_write {
        return Err(StatusCode::FORBIDDEN);
    }

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
                source_agent: agent_id,
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

/// Import memories from file
#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub path: String,
    pub source: String, // "openclaw_daily", "openclaw_memory", "openclaw_user", "markdown", "json", "text"
    pub agent: Option<String>,
}

async fn import_memories(
    State(state): State<AppState>,
    Path(bank_name): Path<String>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let source = match req.source.as_str() {
        "openclaw_daily" => crate::import::ImportSource::OpenClawDaily,
        "openclaw_memory" => crate::import::ImportSource::OpenClawMemory,
        "openclaw_user" => crate::import::ImportSource::OpenClawUser,
        "markdown" => crate::import::ImportSource::Markdown,
        "json" => crate::import::ImportSource::Json,
        "text" => crate::import::ImportSource::Text,
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let path = std::path::Path::new(&req.path);
    let agent = req.agent.unwrap_or_else(|| "import".to_string());

    let result = crate::import::import_from_file(&state.db, &bank.id, path, source, &agent)
        .map_err(|e| {
            tracing::error!(error = %e, "import failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(serde_json::json!({
        "source": req.source,
        "path": req.path,
        "total_found": result.total_found,
        "imported": result.imported,
        "skipped_duplicates": result.skipped_duplicates,
        "errors": result.errors,
    })))
}

/// Import from OpenClaw workspace
#[derive(Debug, Deserialize)]
pub struct ImportOpenClawRequest {
    pub workspace_path: String,
    pub agent: Option<String>,
}

async fn import_openclaw_workspace(
    State(state): State<AppState>,
    Path(bank_name): Path<String>,
    Json(req): Json<ImportOpenClawRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let workspace = std::path::Path::new(&req.workspace_path);
    let agent = req.agent.unwrap_or_else(|| "openclaw_import".to_string());

    let results = crate::import::import_openclaw_workspace(&state.db, &bank.id, workspace, &agent)
        .map_err(|e| {
            tracing::error!(error = %e, "OpenClaw import failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let total_found: usize = results.iter().map(|r| r.total_found).sum();
    let total_imported: usize = results.iter().map(|r| r.imported).sum();
    let total_skipped: usize = results.iter().map(|r| r.skipped_duplicates).sum();

    Ok(Json(serde_json::json!({
        "workspace_path": req.workspace_path,
        "files_processed": results.len(),
        "total_found": total_found,
        "total_imported": total_imported,
        "total_skipped_duplicates": total_skipped,
        "details": results,
    })))
}

/// Export memories to JSON
#[derive(Debug, Deserialize)]
pub struct ExportRequest {
    pub output_path: String,
}

async fn export_memories(
    State(state): State<AppState>,
    Path(bank_name): Path<String>,
    Json(req): Json<ExportRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let bank = state
        .db
        .get_bank_by_name(&bank_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let output = std::path::Path::new(&req.output_path);
    let count = crate::import::export_to_json(&state.db, &bank.id, output)
        .map_err(|e| {
            tracing::error!(error = %e, "export failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(serde_json::json!({
        "output_path": req.output_path,
        "exported": count,
    })))
}

/// Trigger night shift manually (for testing)
async fn trigger_night_shift(State(state): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let db = state.db.clone();
    
    // Use tokio::task::spawn_blocking instead of creating a new runtime
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let scheduler = crate::night::scheduler::NightScheduler::new(db.clone(), "0 2 * * *".to_string());
        match rt.block_on(scheduler.run_now()) {
            Ok(report) => {
                tracing::info!(
                    audited = report.audited,
                    consolidated = report.consolidated,
                    models = report.models_built,
                    repaired = report.repaired,
                    errors = report.errors.len(),
                    "Manual night shift complete"
                );
                
                // Log to database
                let _ = db.log_night_audit(
                    &chrono::Local::now().format("%Y-%m-%d").to_string(),
                    report.audited as i64,
                    0,
                    report.models_built as i64,
                    Some(&format!(
                        "Manual trigger. Consolidated: {}, Repaired: {}, Errors: {}",
                        report.consolidated,
                        report.repaired,
                        report.errors.len()
                    )),
                );
            }
            Err(e) => {
                tracing::error!(error = %e, "Manual night shift failed");
            }
        }
    });
    
    Ok(Json(serde_json::json!({
        "status": "started",
        "message": "Night shift running in background. Check logs for results."
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

    // API routes first (more specific)
    let api_routes = Router::new()
        .route("/health", get(health))
        .route("/banks", get(list_banks).post(create_bank))
        .route("/banks/:name", get(get_bank))
        .route("/banks/:name/memories", get(list_memories).post(store_memory))
        .route("/banks/:name/memories/:id", get(get_memory).delete(delete_memory))
        .route("/banks/:name/search", post(search_memories))
        .route("/banks/:name/import", post(import_memories))
        .route("/banks/:name/import-openclaw", post(import_openclaw_workspace))
        .route("/banks/:name/export", post(export_memories))
        .route("/reflect", post(reflect))
        .route("/night-shift", post(trigger_night_shift));

    // UI routes (catch-all)
    let ui_routes = Router::new()
        .route("/", get(|| async { serve_ui(Path("".to_string())).await }))
        .route("/*path", get(serve_ui));

    Router::new()
        .merge(api_routes)
        .merge(ui_routes)
        .with_state(state)
}
