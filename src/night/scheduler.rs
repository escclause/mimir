// Night shift scheduler
// Runs tasks at configured time (default: 2am)

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::TimeZone;
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

/// Parsed cron schedule
#[derive(Debug, Clone)]
struct CronSchedule {
    minute: u8,  // 0-59
    hour: u8,    // 0-23
}

impl CronSchedule {
    /// Parse a simple cron expression (only supports "M H * * *" format)
    fn parse(expr: &str) -> Result<Self> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(anyhow::anyhow!(
                "Invalid cron expression: {} (expected 5 fields)",
                expr
            ));
        }

        let minute = parts[0]
            .parse::<u8>()
            .map_err(|_| anyhow::anyhow!("Invalid minute: {}", parts[0]))?;
        let hour = parts[1]
            .parse::<u8>()
            .map_err(|_| anyhow::anyhow!("Invalid hour: {}", parts[1]))?;

        if minute > 59 {
            return Err(anyhow::anyhow!("Minute out of range: {}", minute));
        }
        if hour > 23 {
            return Err(anyhow::anyhow!("Hour out of range: {}", hour));
        }

        // Verify day-of-week, day-of-month, month are "*" (not supported yet)
        if parts[2] != "*" || parts[3] != "*" || parts[4] != "*" {
            return Err(anyhow::anyhow!(
                "Only daily schedules supported (M H * * *), got: {}",
                expr
            ));
        }

        Ok(Self { minute, hour })
    }

    /// Calculate duration until next run time
    fn duration_until_next(&self) -> Duration {
        let now = chrono::Local::now();
        let today = now.date_naive();

        let next = today.and_hms_opt(self.hour as u32, self.minute as u32, 0)
            .unwrap_or_else(|| today.and_hms_opt(0, 0, 0).unwrap());

        let next_dt = if next > now.naive_local() {
            next
        } else {
            // Already passed today, schedule for tomorrow
            (today + chrono::Duration::days(1))
                .and_hms_opt(self.hour as u32, self.minute as u32, 0)
                .unwrap()
        };

        let now_ts = now.timestamp();
        let next_ts = chrono::Local
            .from_local_datetime(&next_dt)
            .single()
            .map(|dt| dt.timestamp())
            .unwrap_or(now_ts + 86400);

        let secs = (next_ts - now_ts).max(0) as u64;
        Duration::from_secs(secs)
    }
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
    /// Parses the cron schedule and sleeps until the next run time.
    /// Then runs night shift tasks and repeats.
    pub async fn start(self: Arc<Self>) -> Result<()> {
        // Parse schedule
        let cron = match CronSchedule::parse(&self.schedule) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    schedule = %self.schedule,
                    "Invalid cron schedule, night shift disabled"
                );
                return Err(e);
            }
        };

        tracing::info!(
            schedule = %self.schedule,
            hour = cron.hour,
            minute = cron.minute,
            "Night shift scheduler started"
        );

        tokio::spawn(async move {
            loop {
                let wait = cron.duration_until_next();
                let wait_hours = wait.as_secs() as f64 / 3600.0;

                tracing::info!(
                    wait_hours = format!("{:.1}", wait_hours),
                    "Night shift sleeping until next run"
                );

                tokio::time::sleep(wait).await;

                tracing::info!("Night shift wake time — starting tasks");

                match self.run_now().await {
                    Ok(report) => {
                        tracing::info!(
                            audited = report.audited,
                            consolidated = report.consolidated,
                            models = report.models_built,
                            repaired = report.repaired,
                            errors = report.errors.len(),
                            "Night shift complete"
                        );

                        // Log to database
                        let _ = self.db.log_night_audit(
                            &chrono::Local::now().format("%Y-%m-%d").to_string(),
                            report.audited as i64,
                            0, // corrections_made — TODO: track from audit
                            report.models_built as i64,
                            Some(&format!(
                                "Consolidated: {}, Repaired: {}, Errors: {}",
                                report.consolidated,
                                report.repaired,
                                report.errors.len()
                            )),
                        );
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Night shift failed");
                    }
                }
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
        let engine_guard = self.engine.lock().await;
        let engine = engine_guard.as_ref().ok_or_else(|| anyhow::anyhow!("Engine not available"))?;

        // Get all memories to analyze for patterns
        let all_banks = self.db.list_banks()?;
        let mut all_memories = Vec::new();

        for bank in &all_banks {
            let memories = self.db.all_memories(&bank.id)?;
            all_memories.extend(memories);
        }

        if all_memories.len() < 5 {
            tracing::debug!(
                count = all_memories.len(),
                "Not enough memories for mental model building"
            );
            return Ok(0);
        }

        // Group by memory type for pattern analysis
        let observations: Vec<_> = all_memories
            .iter()
            .filter(|m| m.memory_type == "observation" || m.memory_type == "experience")
            .collect();

        if observations.is_empty() {
            return Ok(0);
        }

        // Build prompt for mental model generation
        let system = "You are a pattern recognition engine. Analyze observations and identify recurring themes, user preferences, and behavioral patterns. Create concise mental models.";

        let obs_text: Vec<String> = observations
            .iter()
            .enumerate()
            .map(|(i, m)| format!("{}. {}", i + 1, m.content))
            .collect();

        let user = format!(
            "Observations:\n{}\n\nIdentify:\n1. Recurring themes (3-5)\n2. User preferences inferred\n3. Behavioral patterns\n4. Suggested mental models (name + description)",
            obs_text.join("\n")
        );

        let response = engine.chat(system, &user, 1500)?;

        // Store as a special synthesis with type marker
        self.db.store_synthesis(
            &format!("[MENTAL_MODELS]\n{}", response),
            observations.len(),
        )?;

        Ok(observations.len())
    }

    /// Repair memories (resolve contradictions, decay stale).
    async fn repair_memories(&self) -> Result<usize> {
        let engine_guard = self.engine.lock().await;
        let engine = engine_guard.as_ref().ok_or_else(|| anyhow::anyhow!("Engine not available"))?;

        // Get all memories to check for contradictions
        let all_banks = self.db.list_banks()?;
        let mut all_memories = Vec::new();

        for bank in &all_banks {
            let memories = self.db.all_memories(&bank.id)?;
            all_memories.extend(memories);
        }

        if all_memories.len() < 2 {
            return Ok(0);
        }

        // Simple contradiction detection: look for negation patterns
        // In production, this would use embedding similarity + LLM reasoning
        let mut repaired = 0;

        // Check for exact duplicates that slipped through
        let mut seen_content: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for mem in &all_memories {
            let normalized = mem.content.to_lowercase();
            if let Some(count) = seen_content.get(&normalized) {
                if *count > 0 {
                    // Duplicate found — mark for decay by reducing importance
                    let _ = self.db.update_memory_importance(&mem.id, 1);
                    repaired += 1;
                }
                seen_content.insert(normalized, count + 1);
            } else {
                seen_content.insert(normalized, 1);
            }
        }

        // Check for stale memories (not recalled in 90 days)
        let cutoff = chrono::Utc::now() - chrono::Duration::days(90);
        let cutoff_str = cutoff.to_rfc3339();

        for mem in &all_memories {
            if let Some(last_recalled) = &mem.last_recalled_at {
                if last_recalled < &cutoff_str && mem.importance > 1 {
                    // Decay stale memory
                    let _ = self.db.update_memory_importance(
                        &mem.id,
                        mem.importance.saturating_sub(1),
                    );
                    repaired += 1;
                }
            }
        }

        // Use LLM for deeper contradiction analysis if we have enough memories
        if all_memories.len() >= 10 {
            let system = "You are a memory consistency checker. Identify contradictions between memories and suggest resolutions.";

            let mem_text: Vec<String> = all_memories
                .iter()
                .take(20) // Limit to avoid token overflow
                .enumerate()
                .map(|(i, m)| format!("{}. {}", i + 1, m.content))
                .collect();

            let user = format!(
                "Memories:\n{}\n\nIdentify:\n1. Direct contradictions (if any)\n2. Outdated information\n3. Suggested merges or archives",
                mem_text.join("\n")
            );

            let _response = engine.chat(system, &user, 1000)?;
            // TODO: Parse response and apply corrections
        }

        Ok(repaired)
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
