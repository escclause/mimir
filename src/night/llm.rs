// Night shift engine using LFM2.5-2.6B
// Deep synthesis, pattern detection, memory consolidation

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Night shift engine for deep memory synthesis.
///
/// Uses LFM2.5-2.6B (2.6B params, 128K context) for:
/// - Auditing day-shift Jev decisions
/// - Consolidating related memories into insights
/// - Building mental models from patterns
/// - Repairing contradictions and decaying stale memories
pub struct NightEngine {
    /// HTTP URL for llama-server
    server_url: String,
    /// Managed subprocess (Some if we spawned it)
    server_process: Arc<Mutex<Option<Child>>>,
    /// HTTP client
    client: reqwest::blocking::Client,
}

/// Chat message for LFM2.5
#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    total_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
}

impl NightEngine {
    /// Connect to an existing llama-server instance.
    pub fn connect(server_url: impl Into<String>) -> Result<Self> {
        let server_url = server_url.into();

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120)) // Longer timeout for generation
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

        let health_url = format!("{}/health", server_url);
        let response = client
            .get(&health_url)
            .send()
            .map_err(|e| anyhow::anyhow!("Cannot connect to llama-server at {}: {}", server_url, e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "llama-server at {} returned status {}",
                server_url,
                response.status()
            ));
        }

        tracing::info!(url = %server_url, "Connected to external night shift llama-server");

        Ok(Self {
            server_url,
            server_process: Arc::new(Mutex::new(None)),
            client,
        })
    }

    /// Spawn a managed llama-server subprocess.
    pub fn spawn(
        model_path: impl AsRef<Path>,
        server_binary: impl AsRef<Path>,
        port: u16,
    ) -> Result<Self> {
        let model_path = model_path.as_ref().to_path_buf();
        let server_binary = server_binary.as_ref().to_path_buf();
        let server_url = format!("http://127.0.0.1:{}", port);

        tracing::info!(
            model = %model_path.display(),
            port = port,
            "Spawning managed night shift llama-server"
        );

        if !model_path.exists() {
            return Err(anyhow::anyhow!(
                "Night shift model file not found: {}",
                model_path.display()
            ));
        }
        if !server_binary.exists() {
            return Err(anyhow::anyhow!(
                "llama-server binary not found: {}",
                server_binary.display()
            ));
        }

        let mut child = Command::new(&server_binary)
            .args([
                "--model",
                model_path.to_str().unwrap(),
                "--port",
                &port.to_string(),
                "--host",
                "127.0.0.1",
                "-t",
                "4", // 4 threads for CPU inference
                "-c",
                "131072", // 128K context
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn llama-server: {}", e))?;

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

        let health_url = format!("{}/health", server_url);
        let mut ready = false;
        let mut last_error = String::new();

        for _ in 0..120 {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Err(anyhow::anyhow!(
                        "llama-server exited during startup (status: {})",
                        status
                    ));
                }
                Ok(None) => {
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
                "llama-server failed to become ready. Last error: {}",
                last_error
            ));
        }

        tracing::info!(url = %server_url, "Managed night shift llama-server ready");

        Ok(Self {
            server_url,
            server_process: Arc::new(Mutex::new(Some(child))),
            client,
        })
    }

    /// Check whether night shift model exists.
    pub fn model_exists(model_path: impl AsRef<Path>) -> bool {
        model_path.as_ref().exists()
    }

    /// Send a chat completion request.
    pub fn chat(&self, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        let url = format!("{}/v1/chat/completions", self.server_url);

        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user.to_string(),
                },
            ],
            max_tokens,
            temperature: 0.1, // Low temp for consistent synthesis
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .map_err(|e| anyhow::anyhow!("Night shift chat request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Night shift chat returned {}: {}",
                status,
                body
            ));
        }

        let chat_response: ChatResponse = response
            .json()
            .map_err(|e| anyhow::anyhow!("Failed to parse night shift response: {}", e))?;

        let content = chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        Ok(content)
    }

    /// Synthesize insights from a batch of memories.
    pub fn synthesize(&self, memories: &[MemoryBatch]) -> Result<Synthesis> {
        let system = "You are a memory synthesis engine. Analyze the provided memories and identify patterns, contradictions, and insights. Be concise but thorough.";

        let user = format!(
            "Memories to analyze:\n{}\n\nProvide:\n1. Key patterns (3-5 bullet points)\n2. Contradictions or conflicts (if any)\n3. Suggested mental models\n4. Memories that should be consolidated or archived",
            memories
                .iter()
                .enumerate()
                .map(|(i, m)| format!("{}. [{}] {} (importance: {})", i + 1, m.memory_type, m.content, m.importance))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let response = self.chat(system, &user, 2000)?;

        Ok(Synthesis {
            raw_response: response,
            memory_count: memories.len(),
        })
    }

    /// Audit a Jev decision for correctness.
    pub fn audit_decision(&self, content: &str, decision: &str, confidence: f32) -> Result<Audit> {
        let system = "You are auditing a memory classification decision. Determine if the classification is correct and suggest improvements.";

        let user = format!(
            "Content: {}\nClassification: {}\nConfidence: {:.2}\n\nIs this classification correct? If not, what should it be? Answer briefly.",
            content, decision, confidence
        );

        let response = self.chat(system, &user, 500)?;

        Ok(Audit {
            raw_response: response,
            original_decision: decision.to_string(),
        })
    }
}

/// A batch of memories for synthesis
#[derive(Debug, Clone)]
pub struct MemoryBatch {
    pub content: String,
    pub memory_type: String,
    pub importance: u8,
}

/// Result of synthesis
#[derive(Debug, Clone)]
pub struct Synthesis {
    pub raw_response: String,
    pub memory_count: usize,
}

/// Result of auditing a decision
#[derive(Debug, Clone)]
pub struct Audit {
    pub raw_response: String,
    pub original_decision: String,
}

impl Drop for NightEngine {
    fn drop(&mut self) {
        if let Ok(mut child_opt) = self.server_process.lock() {
            if let Some(mut child) = child_opt.take() {
                tracing::info!("Shutting down managed night shift llama-server");
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
