// D1 decision engine via llama-server subprocess
// Day-shift cognition using Liquid AI's d1-omni-600M

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{Classification, Contradiction, DecisionEngine, ImportanceScore, MemoryType};

/// D1 decision engine running via llama-server subprocess.
///
/// Spawns a persistent llama-server process that loads the D1 GGUF model
/// and serves typed decisions via the /v1/systemone endpoint. Mímir
/// communicates with the server using HTTP POST requests.
///
/// D1 is a "System One" decision model — it answers named, typed questions
/// over a state in one forward pass, with no generated tokens. This makes
/// it ideal for fast, calibrated day-shift memory judgments.
pub struct D1Engine {
    /// Path to the D1 GGUF model file
    model_path: PathBuf,
    /// Path to the multimodal projector (mmproj) file
    mmproj_path: PathBuf,
    /// Path to llama-server binary
    server_binary: PathBuf,
    /// HTTP port for llama-server
    port: u16,
    /// Persistent server process (kept alive for engine lifetime)
    #[allow(dead_code)]
    server_process: Arc<Mutex<Option<Child>>>,
    /// HTTP client for API calls
    client: reqwest::blocking::Client,
}

/// D1 systemone request
#[derive(Debug, Serialize)]
struct SystemOneRequest {
    state: String,
    questions: serde_json::Value,
}

/// D1 systemone response
#[derive(Debug, Deserialize)]
struct SystemOneResponse {
    answers: serde_json::Value,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    input_tokens: Option<u32>,
}

/// Server readiness check response
#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
}

impl D1Engine {
    /// Start llama-server with the D1 model.
    ///
    /// `model_path` — path to D1 GGUF model file
    /// `mmproj_path` — path to multimodal projector GGUF
    /// `server_binary` — path to llama-server binary
    /// `port` — HTTP port for the server (default: 8081)
    pub fn start(
        model_path: impl AsRef<Path>,
        mmproj_path: impl AsRef<Path>,
        server_binary: impl AsRef<Path>,
        port: u16,
    ) -> Result<Self> {
        let model_path = model_path.as_ref().to_path_buf();
        let mmproj_path = mmproj_path.as_ref().to_path_buf();
        let server_binary = server_binary.as_ref().to_path_buf();

        tracing::info!(
            model = %model_path.display(),
            mmproj = %mmproj_path.display(),
            binary = %server_binary.display(),
            port = port,
            "Starting llama-server for D1"
        );

        // Verify files exist
        if !model_path.exists() {
            return Err(anyhow::anyhow!(
                "D1 model file not found: {}",
                model_path.display()
            ));
        }
        if !mmproj_path.exists() {
            return Err(anyhow::anyhow!(
                "D1 mmproj file not found: {}",
                mmproj_path.display()
            ));
        }
        if !server_binary.exists() {
            return Err(anyhow::anyhow!(
                "llama-server binary not found: {}",
                server_binary.display()
            ));
        }

        // Spawn llama-server process
        let mut child = Command::new(&server_binary)
            .args([
                "--model",
                model_path.to_str().unwrap(),
                "--mmproj",
                mmproj_path.to_str().unwrap(),
                "--port",
                &port.to_string(),
                "--host",
                "127.0.0.1",
                "-b",
                "4096",
                "-ub",
                "4096",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn llama-server: {}", e))?;

        // Wait for server to be ready
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

        let health_url = format!("http://127.0.0.1:{}/health", port);
        let mut ready = false;
        let mut last_error = String::new();

        for i in 0..60 {
            // Check if process died
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process exited — capture stderr
                    let stderr = child
                        .stderr
                        .take()
                        .map(|s| {
                            std::io::BufRead::lines(std::io::BufReader::new(s))
                                .take(50)
                                .map(|l| l.unwrap_or_default())
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    return Err(anyhow::anyhow!(
                        "llama-server exited during startup (status: {}):\n{}",
                        status,
                        stderr
                    ));
                }
                Ok(None) => {
                    // Still running, check health
                    match client.get(&health_url).send() {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                if let Ok(health) = resp.json::<HealthResponse>() {
                                    if health.status == "ok" {
                                        ready = true;
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            last_error = e.to_string();
                            if i % 10 == 0 {
                                tracing::debug!(
                                    attempt = i,
                                    error = %e,
                                    "Waiting for llama-server..."
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    last_error = e.to_string();
                }
            }

            std::thread::sleep(Duration::from_millis(500));
        }

        if !ready {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::anyhow!(
                "llama-server failed to become ready within 30 seconds. Last error: {}",
                last_error
            ));
        }

        tracing::info!("llama-server ready on port {}", port);

        Ok(Self {
            model_path,
            mmproj_path,
            server_binary,
            port,
            server_process: Arc::new(Mutex::new(Some(child))),
            client,
        })
    }

    /// Check whether D1 model files exist at the given paths.
    pub fn model_exists(model_path: impl AsRef<Path>, mmproj_path: impl AsRef<Path>) -> bool {
        model_path.as_ref().exists() && mmproj_path.as_ref().exists()
    }

    /// Send a systemone request to llama-server.
    fn systemone(&self, state: &str, questions: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("http://127.0.0.1:{}/v1/systemone", self.port);

        let request = SystemOneRequest {
            state: state.to_string(),
            questions,
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .map_err(|e| anyhow::anyhow!("D1 systemone request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(anyhow::anyhow!(
                "D1 systemone returned {}: {}",
                status,
                body
            ));
        }

        let systemone_response: SystemOneResponse = response
            .json()
            .map_err(|e| anyhow::anyhow!("Failed to parse D1 response: {}", e))?;

        Ok(systemone_response.answers)
    }

    /// Classify content using D1's typed decision API.
    fn classify_with_model(&self, content: &str) -> Result<Classification> {
        let questions = serde_json::json!({
            "memory_type": {
                "type": "choice",
                "instructions": "What type of memory is this content?",
                "criteria": {
                    "world_fact": "Objective facts about the world, systems, or external reality",
                    "experience": "Personal experiences, events, or things that happened to the agent or user",
                    "observation": "Inferences, observations, or synthesized insights about patterns",
                    "mental_model": "Preferences, habits, beliefs, goals, or mental models of the user"
                }
            }
        });

        let answers = self.systemone(content, questions)?;

        // Parse the choice answer
        let memory_type_str = answers["memory_type"]["choice"]
            .as_str()
            .unwrap_or("world_fact");

        let confidence = answers["memory_type"]["confidence"]
            .as_f64()
            .unwrap_or(0.5) as f32;

        let memory_type = match memory_type_str {
            "world_fact" => MemoryType::WorldFact,
            "experience" => MemoryType::Experience,
            "observation" => MemoryType::Observation,
            "mental_model" => MemoryType::MentalModel,
            _ => self.heuristic_classify(content),
        };

        Ok(Classification {
            memory_type,
            confidence,
        })
    }

    /// Score importance using D1's score type.
    fn score_with_model(&self, content: &str) -> Result<ImportanceScore> {
        let questions = serde_json::json!({
            "importance": {
                "type": "score",
                "instructions": "How important is this memory to retain long-term? Consider: Does it affect future behavior? Is it reusable? Would losing it matter?",
                "criteria": ["trivial - forget immediately", "minor - probably not needed", "moderate - useful context", "important - actively reference", "critical - always relevant"]
            }
        });

        let answers = self.systemone(content, questions)?;

        // D1 score is 0-4, we map to 1-5
        let score_0_to_4 = answers["importance"]["score"]
            .as_f64()
            .unwrap_or(1.0);

        let level = ((score_0_to_4 + 1.0).round() as u8).clamp(1, 5);

        let confidence = answers["importance"]["confidence"]
            .as_f64()
            .unwrap_or(0.5) as f32;

        Ok(ImportanceScore { level, confidence })
    }

    /// Detect contradiction using D1's noul (yes/no probability) type.
    fn detect_with_model(&self, new: &str, existing: &[&str]) -> Result<Contradiction> {
        if existing.is_empty() {
            return Ok(Contradiction {
                exists: false,
                confidence: 1.0,
                conflicting_ids: vec![],
            });
        }

        let mut conflicting = Vec::new();
        let mut max_confidence = 0.0f32;

        for (idx, existing_item) in existing.iter().enumerate() {
            // Combine both statements into a single state
            let state = format!(
                "Statement A: {}\nStatement B: {}",
                existing_item, new
            );

            let questions = serde_json::json!({
                "contradicts": {
                    "type": "noul",
                    "instructions": "Do these two statements contradict each other? Consider: Do they assert incompatible facts? Is one the negation of the other?"
                }
            });

            let answers = self.systemone(&state, questions)?;

            // noul gives probability of "yes"
            let contradiction_prob = answers["contradicts"]["noul"]
                .as_f64()
                .unwrap_or(0.0) as f32;

            // Threshold: >0.7 means likely contradiction
            if contradiction_prob > 0.7 {
                conflicting.push(idx.to_string());
                max_confidence = max_confidence.max(contradiction_prob);
            }
        }

        Ok(Contradiction {
            exists: !conflicting.is_empty(),
            confidence: max_confidence,
            conflicting_ids: conflicting,
        })
    }

    /// Rule-based fallback classification.
    fn heuristic_classify(&self, content: &str) -> MemoryType {
        let lower = content.to_lowercase();

        let exp_markers = [
            "i ", "my ", "me ", "we ", "our ", "i'm", "i've", "i'll", "myself",
        ];
        if exp_markers
            .iter()
            .any(|m| lower.starts_with(m) || lower.contains(m))
        {
            return MemoryType::Experience;
        }

        let model_markers = [
            "prefer", "always", "never", "usually", "tends to", "pattern", "habit",
        ];
        if model_markers.iter().any(|m| lower.contains(m)) {
            return MemoryType::MentalModel;
        }

        let obs_markers = [
            "seems", "appears", "likely", "probably", "infer", "conclude", "suggest",
        ];
        if obs_markers.iter().any(|m| lower.contains(m)) {
            return MemoryType::Observation;
        }

        MemoryType::WorldFact
    }

    fn heuristic_importance(&self, content: &str) -> u8 {
        let len = content.len();
        if len > 500 {
            4
        } else if len > 200 {
            3
        } else if len > 50 {
            2
        } else {
            1
        }
    }
}

impl DecisionEngine for D1Engine {
    fn classify(&self, content: &str) -> Result<Classification> {
        match self.classify_with_model(content) {
            Ok(result) => Ok(result),
            Err(e) => {
                tracing::warn!(error = %e, "D1 classification failed, using heuristic fallback");
                Ok(Classification {
                    memory_type: self.heuristic_classify(content),
                    confidence: 0.60,
                })
            }
        }
    }

    fn score_importance(&self, content: &str) -> Result<ImportanceScore> {
        match self.score_with_model(content) {
            Ok(result) => Ok(result),
            Err(e) => {
                tracing::warn!(error = %e, "D1 importance scoring failed, using heuristic fallback");
                Ok(ImportanceScore {
                    level: self.heuristic_importance(content),
                    confidence: 0.50,
                })
            }
        }
    }

    fn detect_contradiction(&self, new: &str, existing: &[&str]) -> Result<Contradiction> {
        match self.detect_with_model(new, existing) {
            Ok(result) => Ok(result),
            Err(e) => {
                tracing::warn!(error = %e, "D1 contradiction detection failed");
                Ok(Contradiction {
                    exists: false,
                    confidence: 0.0,
                    conflicting_ids: vec![],
                })
            }
        }
    }
}

impl Drop for D1Engine {
    fn drop(&mut self) {
        tracing::info!("Shutting down D1 llama-server");
        if let Ok(mut child_opt) = self.server_process.lock() {
            if let Some(mut child) = child_opt.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
