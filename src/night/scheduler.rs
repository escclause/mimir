// Night shift scheduler
// Runs tasks at configured time (default: 2am)

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::db::Db;
use crate::night::llm::NightEngine;

/// Night shift scheduler.
///
/// Runs consolidation tasks at a configured time (default 2am).
/// Uses LFM2.5-2.6B for deep synthesis.
pub struct NightScheduler {
    db: Arc<Db>,
    engine: Arc<Mutex<Option<NightEngine>>>,
    schedule: String, // cron expression
}

impl NightScheduler {
    pub fn new(db: Arc<Db>, schedule: String) -> Self {
        Self {
            db,
            engine: Arc::new(Mutex::new(None)),
            schedule,
        }
    }

    /// Start the scheduler loop.
    ///
    /// This spawns a background task that checks the time periodically
    /// and runs night shift tasks when scheduled.
    pub async fn start(self: Arc<Self>) -> Result<()> {
        tokio::spawn(async move {
            loop {
                // Check if it's time to run (simplified: check every hour)
                // In production, use a proper cron parser
                tokio::time::sleep(Duration::from_secs(3600)).await;

                // TODO: Parse cron schedule and check if we should run
                // For now, just log that scheduler is alive
                tracing::debug!("Night shift scheduler tick (schedule: {})", self.schedule);
            }
        });

        Ok(())
    }

    /// Run night shift tasks manually (for testing or immediate execution).
    pub async fn run_now(&self) -> Result<NightShiftReport> {
        tracing::info!("Starting night shift tasks");

        let mut report = NightShiftReport::default();

        // Ensure engine is available
        self.ensure_engine().await?;

        // Task 1: Audit day's Jev decisions
        match self.audit_decisions().await {
            Ok(audit_count) => {
                report.audited = audit_count;
                tracing::info!(audited = audit_count, "Audit complete");
            }
            Err(e) => {
                report.errors.push(format!("Audit failed: {}", e));
                tracing::warn!(error = %e, "Audit failed");
            }
        }

        // Task 2: Consolidate related memories
        match self.consolidate_memories().await {
            Ok(consolidated) => {
                report.consolidated = consolidated;
                tracing::info!(consolidated = consolidated, "Consolidation complete");
            }
            Err(e) => {
                report.errors.push(format!("Consolidation failed: {}", e));
                tracing::warn!(error = %e, "Consolidation failed");
            }
        }

        // Task 3: Build mental models
        match self.build_mental_models().await {
            Ok(models_built) => {
                report.models_built = models_built;
                tracing::info!(models = models_built, "Mental model building complete");
            }
            Err(e) => {
                report.errors.push(format!("Mental model building failed: {}", e));
                tracing::warn!(error = %e, "Mental model building failed");
            }
        }

        // Task 4: Repair (resolve contradictions, decay stale memories)
        match self.repair_memories().await {
            Ok(repaired) => {
                report.repaired = repaired;
                tracing::info!(repaired = repaired, "Repair complete");
            }
            Err(e) => {
                report.errors.push(format!("Repair failed: {}", e));
                tracing::warn!(error = %e, "Repair failed");
            }
        }

        tracing::info!(
            audited = report.audited,
            consolidated = report.consolidated,
            models = report.models_built,
            repaired = report.repaired,
            errors = report.errors.len(),
            "Night shift complete"
        );

        Ok(report)
    }

    /// Ensure the night shift engine is available.
    async fn ensure_engine(&self) -> Result<()> {
        let mut engine_guard = self.engine.lock().await;

        if engine_guard.is_some() {
            return Ok(());
        }

        let config = crate::config::Config::default();

        if !NightEngine::model_exists(&config.night_model_path) {
            return Err(anyhow::anyhow!(
                "Night shift model not found: {}",
                config.night_model_path.display()
            ));
        }

        let server_binary = find_llama_server().ok_or_else(|| {
            anyhow::anyhow!("llama-server binary not found")
        })?;

        let engine = NightEngine::spawn(
            &config.night_model_path,
            &server_binary,
            config.night_port,
        )?;

        *engine_guard = Some(engine);
        Ok(())
    }

    /// Audit day's Jev decisions.
    async fn audit_decisions(&self) -> Result<usize> {
        let engine_guard = self.engine.lock().await;
        let engine = engine_guard.as_ref().ok_or_else(|| anyhow::anyhow!("Engine not available"))?;

        // Get recent Jev decisions from database
        let decisions = self.db.get_recent_jev_decisions(50)?;

        let mut audited = 0;
        for decision in decisions {
            // Skip low-confidence decisions (already flagged)
            if decision.confidence.unwrap_or(0.0) < 0.5 {
                continue;
            }

            let _audit = engine.audit_decision(
                &decision.input,
                &decision.output,
                decision.confidence.unwrap_or(0.0) as f32,
            )?;

            audited += 1;
        }

        Ok(audited)
    }

    /// Consolidate related memories.
    async fn consolidate_memories(&self) -> Result<usize> {
        let engine_guard = self.engine.lock().await;
        let engine = engine_guard.as_ref().ok_or_else(|| anyhow::anyhow!("Engine not available"))?;

        // Get memories ready for consolidation
        let memories = self.db.get_memories_for_consolidation(100)?;

        if memories.is_empty() {
            return Ok(0);
        }

        // Convert to MemoryBatch
        let batch: Vec<_> = memories
            .iter()
            .map(|m| crate::night::llm::MemoryBatch {
                content: m.content.clone(),
                memory_type: m.memory_type.clone(),
                importance: m.importance,
            })
            .collect();

        let synthesis = engine.synthesize(&batch)?;

        // Store synthesis result
        self.db.store_synthesis(&synthesis.raw_response, memories.len())?;

        Ok(memories.len())
    }

    /// Build mental models from patterns.
    async fn build_mental_models(&self) -> Result<usize> {
        // TODO: Implement mental model building
        // This would:
        // 1. Get all observations and experiences
        // 2. Identify recurring patterns
        // 3. Create/update mental model entries
        Ok(0)
    }

    /// Repair memories (resolve contradictions, decay stale).
    async fn repair_memories(&self) -> Result<usize> {
        // TODO: Implement repair
        // This would:
        // 1. Detect contradictions between memories
        // 2. Mark stale memories for decay
        // 3. Resolve conflicts using night shift reasoning
        Ok(0)
    }
}

/// Report of night shift execution.
#[derive(Debug, Default)]
pub struct NightShiftReport {
    pub audited: usize,
    pub consolidated: usize,
    pub models_built: usize,
    pub repaired: usize,
    pub errors: Vec<String>,
}

/// Find llama-server binary.
fn find_llama_server() -> Option<std::path::PathBuf> {
    let candidates = [
        "/home/glitch/llama.cpp-official/build/bin/llama-server",
        "/home/glitch/llama.cpp-turboquant/build/bin/llama-server",
        "/usr/local/bin/llama-server",
        "/usr/bin/llama-server",
    ];

    for path in &candidates {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            let p = std::path::PathBuf::from(dir).join("llama-server");
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}
