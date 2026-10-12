#![allow(dead_code)]

mod api;
mod config;
mod db;
mod import;
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
use crate::night::scheduler::NightScheduler;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let rest_only = args.iter().any(|a| a == "--rest-only" || a == "--no-mcp");
    let install_service = args.iter().any(|a| a == "--install-service");

    // Initialize tracing — log to stderr so stdout stays clean for JSON-RPC
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mimir=debug,tower_http=debug,axum::rejection=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    tracing::info!("🧠 Mímir starting...");

    // Handle --install-service flag
    if install_service {
        return install_service_interactive();
    }

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

    // Load configuration (defaults → file → env vars)
    let config = config::Config::load();
    tracing::info!("\n{}", config.summary());
    let night_scheduler = Arc::new(NightScheduler::new(db.clone(), config.night_schedule.clone()));
    if let Err(e) = night_scheduler.clone().start().await {
        tracing::warn!(error = %e, "Failed to start night shift scheduler");
    }

    // Start REST API server
    let rest_router = api::rest::router(db.clone());
    let rest_port = config.rest_port;
    tokio::spawn(async move {
        match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", rest_port)).await {
            Ok(listener) => {
                tracing::info!("REST API listening on http://0.0.0.0:{}", rest_port);
                if let Err(e) = axum::serve(listener, rest_router).await {
                    tracing::error!(error = %e, "REST API server failed");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to bind REST API port {}", rest_port);
            }
        }
    });

    if rest_only {
        tracing::info!("Mímir ready — REST-only mode on port {}", rest_port);
        // Keep running forever
        tokio::signal::ctrl_c().await?;
        tracing::info!("Shutting down Mímir");
        return Ok(());
    }

    // Start MCP server over stdio
    tracing::info!("starting MCP server on stdio");
    let server = MimirServer::new(db.clone());

    // Give background D1 engine a moment to start (non-blocking)
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .context("failed to start MCP server")?;

    tracing::info!("Mímir ready — serving MCP over stdio + REST on port {}", rest_port);
    service.waiting().await?;

    Ok(())
}

/// Interactive service installation
fn install_service_interactive() -> Result<()> {
    use std::io::{self, Write};

    println!("🧠 Mímir Service Installation\n");
    println!("This will install Mímir as a systemd user service.");
    println!("The service will start automatically on boot and restart on failure.\n");

    // Ask user if they want to proceed
    print!("Install Mímir as a systemd service? [y/N]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    if input != "y" && input != "yes" {
        println!("Installation cancelled.");
        return Ok(());
    }

    // Ask for mode
    println!("\nSelect mode:");
    println!("  1. REST-only (Web UI + REST API, no MCP)");
    println!("  2. MCP + REST (full featured, requires stdio)");
    print!("Choice [1]: ");
    io::stdout().flush()?;

    let mut mode_input = String::new();
    io::stdin().read_line(&mut mode_input)?;
    let mode = mode_input.trim();
    let mode = if mode == "2" { "mcp" } else { "rest-only" };

    // Get binary path
    let binary_path = std::env::current_exe()
        .context("failed to get current executable path")?;
    let binary_path = binary_path.to_string_lossy();

    // Create systemd user directory
    let systemd_dir = dirs::home_dir()
        .context("failed to resolve home directory")?
        .join(".config")
        .join("systemd")
        .join("user");
    std::fs::create_dir_all(&systemd_dir)?;

    let service_name = "mimir.service";
    let service_path = systemd_dir.join(service_name);

    // Check if service already exists
    if service_path.exists() {
        print!("Service already exists. Overwrite? [y/N]: ");
        io::stdout().flush()?;
        let mut overwrite = String::new();
        io::stdin().read_line(&mut overwrite)?;
        let overwrite = overwrite.trim().to_lowercase();
        if overwrite != "y" && overwrite != "yes" {
            println!("Installation cancelled.");
            return Ok(());
        }
    }

    // Create service file
    let exec_start = if mode == "mcp" {
        format!("{} --mcp", binary_path)
    } else {
        format!("{} --rest-only", binary_path)
    };

    let service_content = format!("[Unit]
Description=Mímir Memory System
After=network.target

[Service]
Type=simple
ExecStart={}
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=mimir=info

[Install]
WantedBy=default.target
", exec_start);

    std::fs::write(&service_path, &service_content)
        .context("failed to write service file")?;

    println!("\n✅ Service file created: {}", service_path.display());
    println!("\nTo enable and start the service:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable {}", service_name);
    println!("  systemctl --user start {}", service_name);
    println!("\nTo check status:");
    println!("  systemctl --user status {}", service_name);
    println!("\nTo view logs:");
    println!("  journalctl --user -u {} -f", service_name);

    // Ask if user wants to enable now
    print!("\nEnable and start service now? [y/N]: ");
    io::stdout().flush()?;

    let mut enable = String::new();
    io::stdin().read_line(&mut enable)?;
    let enable = enable.trim().to_lowercase();

    if enable == "y" || enable == "yes" {
        // Run systemctl commands
        let status = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()?;
        if !status.success() {
            eprintln!("Warning: systemctl daemon-reload failed");
        }

        let status = std::process::Command::new("systemctl")
            .args(["--user", "enable", service_name])
            .status()?;
        if !status.success() {
            eprintln!("Warning: systemctl enable failed");
        }

        let status = std::process::Command::new("systemctl")
            .args(["--user", "start", service_name])
            .status()?;
        if !status.success() {
            eprintln!("Warning: systemctl start failed");
        } else {
            println!("\n🚀 Mímir service started!");
            println!("Access the Web UI at: http://localhost:8083");
        }
    } else {
        println!("\nService installed but not started.");
        println!("Run the commands above when ready.");
    }

    Ok(())
}
