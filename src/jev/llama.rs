// Kev-0.8B llama.cpp integration
// Day-shift decision engine via llama-server subprocess

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{Classification, Contradiction, DecisionEngine, ImportanceScore, MemoryType};

/// Kev-0.8B decision engine running via llama-server subprocess.
///
/// Spawns a persistent llama-server process that loads the GGUF model
/// and serves inference via HTTP. Mímir communicates with the server
/// using the OpenAI-compatible chat completions API.
///
/// The engine is designed to be swappable — any GGUF model can be used
/// by changing the model path in config.
pub struct LlamaEngine {
    /// Path to the GGUF model file
    model_path: PathBuf,
    /// Path to llama-server binary
    server_binary: PathBuf,
    /// HTTP port for llama-server
    port: u16,
    /// Persistent server process (kept alive for engine lifetime)
    #[allow(dead_code)]
    server_process: Arc<Mutex<Option<Child>>>,
    /// HTTP client for API calls
    client: reqwest::blocking::Client,
    /// Maximum tokens for input sequences
    max_seq_len: usize,
}

/// llama-server chat completion request
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
    stop: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// llama-server chat completion response
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

/// Server readiness check response
#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
}

impl LlamaEngine {
    /// Start llama-server with the given model.
    ///
    /// `model_path` — path to GGUF model file
    /// `server_binary` — path to llama-server binary
    /// `port` — HTTP port for the server (default: 8081)
    pub fn start(
        model_path: impl AsRef<Path>,
        server_binary: impl AsRef<Path>,
        port: u16,
    ) -> Result<Self> {
        let model_path = model_path.as_ref().to_path_buf();
        let server_binary = server_binary.as_ref().to_path_buf();

        tracing::info!(
            model = %model_path.display(),
            binary = %server_binary.display(),
            port = port,
            "Starting llama-server for Kev"
        );

        // Verify model exists
        if !model_path.exists() {
            return Err(anyhow::anyhow!(
                "Model file not found: {}",
                model_path.display()
            ));
        }

        // Verify server binary exists
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
                "--port",
                &port.to_string(),
                "--host",
                "127.0.0.1",
                "--ctx-size",
                "512",
                "--threads",
                "4",
                "--batch-size",
                "32",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn llama-server: {}", e))?;

        // Wait for server to be ready
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

        let health_url = format!("http://127.0.0.1:{}/health", port);
        let mut ready = false;

        for _ in 0..60 {
            // Check if process died
            if let Ok(Some(_)) = child.try_wait() {
                let stderr = child
                    .stderr
                    .take()
                    .map(|s| BufReader::new(s).lines().take(20).map(|l| l.unwrap_or_default()).collect::<Vec<_>>().join("\n"))
                    .unwrap_or_default();
                return Err(anyhow::anyhow!(
                    "llama-server exited during startup:\n{}",
                    stderr
                ));
            }

            // Check health endpoint
            if let Ok(resp) = client.get(&health_url).send() {
                if resp.status().is_success() {
                    if let Ok(health) = resp.json::<HealthResponse>() {
                        if health.status == "ok" {
                            ready = true;
                            break;
                        }
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(500));
        }

        if !ready {
            let _ = child.kill();
            return Err(anyhow::anyhow!(
                "llama-server failed to become ready within 30 seconds"
            ));
        }

        tracing::info!("llama-server ready on port {}", port);

        Ok(Self {
            model_path,
            server_binary,
            port,
            server_process: Arc::new(Mutex::new(Some(child))),
            client,
            max_seq_len: 512,
        })
    }

    /// Check whether a model file exists at the given path.
    pub fn model_exists(model_path: impl AsRef<Path>) -> bool {
        model_path.as_ref().exists()
    }

    /// Send a chat completion request to llama-server.
    fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        let url = format!("http://127.0.0.1:{}/v1/chat/completions", self.port);

        let request = ChatRequest {
            model: "kev".to_string(), // llama-server ignores this, uses loaded model
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            max_tokens: 32,
            temperature: 0.1, // Low temperature for deterministic classification
            stop: vec!["\n".to_string()],
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .map_err(|e| anyhow::anyhow!("llama-server request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(anyhow::anyhow!(
                "llama-server returned {}: {}",
                status,
                body
            ));
        }

        let chat_response: ChatResponse = response
            .json()
            .map_err(|e| anyhow::anyhow!("Failed to parse llama-server response: {}", e))?;

        let content = chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(content.trim().to_string())
    }

    /// Classify content using Kev model.
    fn classify_with_model(&self, content: &str) -> Result<Classification> {
        let system = "You are a memory classifier. Classify the given content as exactly one of: world_fact, experience, observation, or mental_model. Reply with only the classification label.";
        let user = format!("Classify: {}", content);

        let response = self.chat(system, &user)?;

        // Parse the response
        let memory_type = match response.to_lowercase().as_str() {
            s if s.contains("world_fact") || s.contains("world fact") => MemoryType::WorldFact,
            s if s.contains("experience") => MemoryType::Experience,
            s if s.contains("observation") => MemoryType::Observation,
            s if s.contains("mental_model") || s.contains("mental model") => MemoryType::MentalModel,
            _ => self.heuristic_classify(content), // Fallback
        };

        Ok(Classification {
            memory_type,
            confidence: 0.85,
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

    /// Score importance using the model.
    fn score_with_model(&self, content: &str) -> Result<ImportanceScore> {
        let system = "You are an importance scorer. Rate the importance of the given memory from 1 (trivial) to 5 (critical). Reply with only the number.";
        let user = format!("Rate importance: {}", content);

        let response = self.chat(system, &user)?;

        // Parse the number from response
        let level: u8 = response
            .chars()
            .find(|c| c.is_ascii_digit())
            .and_then(|c| c.to_digit(10))
            .map(|d| (d as u8).clamp(1, 5))
            .unwrap_or_else(|| self.heuristic_importance(content));

        Ok(ImportanceScore {
            level,
            confidence: 0.80,
        })
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

    /// Detect contradiction using the model.
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
            let system = "You are a contradiction detector. Determine if statement B contradicts statement A. Reply with only 'yes' or 'no'.";
            let user = format!("A: {}\nB: {}\nDo they contradict?", existing_item, new);

            let response = self.chat(system, &user)?;

            if response.to_lowercase().contains("yes") {
                conflicting.push(idx.to_string());
                max_confidence = max_confidence.max(0.75);
            }
        }

        Ok(Contradiction {
            exists: !conflicting.is_empty(),
            confidence: max_confidence,
            conflicting_ids: conflicting,
        })
    }
}

impl DecisionEngine for LlamaEngine {
    fn classify(&self, content: &str) -> Result<Classification> {
        match self.classify_with_model(content) {
            Ok(result) => Ok(result),
            Err(e) => {
                tracing::warn!(error = %e, "Kev llama-server classification failed, using heuristic fallback");
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
                tracing::warn!(error = %e, "Kev llama-server importance scoring failed, using heuristic fallback");
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
                tracing::warn!(error = %e, "Kev llama-server contradiction detection failed, using heuristic fallback");
                Ok(Contradiction {
                    exists: false,
                    confidence: 0.0,
                    conflicting_ids: vec![],
                })
            }
        }
    }
}

impl Drop for LlamaEngine {
    fn drop(&mut self) {
        tracing::info!("Shutting down llama-server");
        if let Ok(mut child_opt) = self.server_process.lock() {
            if let Some(mut child) = child_opt.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
