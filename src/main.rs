#![allow(dead_code)]

mod api;
mod config;
mod db;
mod jev;
mod night;
mod ui;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::ServiceExt;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::api::mcp::MimirServer;
use crate::db::Db;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing — log to stderr so stdout stays clean for JSON-RPC
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mimir=debug,tower_http=debug,axum::rejection=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    tracing::info!("🧠 Mímir starting...");

    // Resolve storage path
    let storage_path = dirs::home_dir()
        .context("failed to resolve home directory")?
        .join(".mimir");
    std::fs::create_dir_all(&storage_path)?;

    // Initialize database
    let db_path: PathBuf = storage_path.join("mimir.db");
    tracing::info!("opening database at {}", db_path.display());
    let db = Arc::new(Db::open(&db_path)?);

    // Seed default bank
    let _default_bank = db.get_or_create_bank("default_bank", "default")?;
    tracing::info!("default bank ready");

    // Start MCP server over stdio
    tracing::info!("starting MCP server on stdio");
    let server = MimirServer::new(db);

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .context("failed to start MCP server")?;

    tracing::info!("Mímir ready — serving MCP over stdio");
    service.waiting().await?;

    Ok(())
}
