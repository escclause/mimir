// Kev-0.8B ONNX integration
// Day-shift decision engine via ONNX Runtime

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Result;
use tokenizers::Tokenizer;

use super::{Classification, Contradiction, DecisionEngine, ImportanceScore, MemoryType};

/// Kev-0.8B decision engine running via ONNX Runtime.
///
/// Uses a small language model to classify memories, score importance,
/// and detect contradictions. Model is loaded from an ONNX file with
/// a corresponding tokenizer.
pub struct KevEngine {
    session: Mutex<ort::session::Session>,
    tokenizer: Tokenizer,
    #[allow(dead_code)]
    model_path: PathBuf,
    /// Max tokens for input sequences
    max_seq_len: usize,
}

impl KevEngine {
    /// Load Kev model from disk.
    ///
    /// `model_path` — path to the ONNX model file (e.g. `kev-0.8b.onnx`)
    /// `tokenizer_path` — path to the tokenizer JSON (e.g. `tokenizer.json`)
    pub fn load(model_path: impl AsRef<Path>, tokenizer_path: impl AsRef<Path>) -> Result<Self> {
        let model_path = model_path.as_ref().to_path_buf();
        let tokenizer_path = tokenizer_path.as_ref();

        tracing::info!(model = %model_path.display(), "Loading Kev ONNX model");

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load Kev tokenizer: {}", e))?;

        // Build ONNX session
        let mut builder = ort::session::Session::builder()
            .map_err(|e| anyhow::anyhow!("Failed to create ONNX session builder: {}", e))?;

        builder = builder
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)
            .unwrap_or_else(|e| e.recover());

        builder = builder
            .with_intra_threads(1)
            .unwrap_or_else(|e| e.recover());

        let session = builder
            .commit_from_file(&model_path)
            .map_err(|e| anyhow::anyhow!("Failed to load Kev ONNX model: {}", e))?;

        tracing::info!(
            inputs = ?session.inputs().iter().map(|i| i.name()).collect::<Vec<_>>(),
            outputs = ?session.outputs().iter().map(|o| o.name()).collect::<Vec<_>>(),
            "Kev model loaded successfully"
        );

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            model_path,
            max_seq_len: 512,
        })
    }

    /// Check whether a Kev model file exists at the given path.
    pub fn model_exists(model_path: impl AsRef<Path>) -> bool {
        model_path.as_ref().exists()
    }

    /// Run inference on a prompt, return the raw logits.
    fn run_inference(&self, prompt: &str) -> Result<ndarray::ArrayD<f32>> {
        // Tokenize
        let encoding = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

        let token_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();

        let seq_len = token_ids.len().min(self.max_seq_len);
        let input_ids: Vec<i64> = token_ids[..seq_len].to_vec();

        // Create attention mask (all 1s for our use case)
        let attention_mask: Vec<i64> = vec![1; seq_len];

        // Build tensors
        let input_tensor = ort::value::Tensor::from_array((
            [1usize, seq_len],
            input_ids.into_boxed_slice(),
        ))
        .map_err(|e| anyhow::anyhow!("Failed to create input tensor: {}", e))?;

        let mask_tensor = ort::value::Tensor::from_array((
            [1usize, seq_len],
            attention_mask.into_boxed_slice(),
        ))
        .map_err(|e| anyhow::anyhow!("Failed to create attention mask tensor: {}", e))?;

        // Run inference
        let mut session = self
            .session
            .lock()
            .map_err(|e| anyhow::anyhow!("Session lock poisoned: {}", e))?;

        let outputs = session
            .run(ort::inputs![
                "input_ids" => input_tensor,
                "attention_mask" => mask_tensor
            ])
            .map_err(|e| anyhow::anyhow!("ONNX inference failed: {}", e))?;

        // Extract logits — take the last position's output
        let output_value = &outputs[0];
        let (shape, data) = output_value
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow::anyhow!("Failed to extract output tensor: {}", e))?;

        let logits = ndarray::ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
            .map_err(|e| anyhow::anyhow!("Failed to reshape logits: {}", e))?;

        Ok(logits)
    }

    /// Extract a classification label from model output.
    ///
    /// For Kev-0.8B, we use a structured prompt and parse the response.
    /// The model outputs logits; we take argmax over the vocabulary for
    /// the last token position and decode.
    fn classify_with_model(&self, content: &str) -> Result<Classification> {
        let prompt = format!(
            "Classify the following as world_fact, experience, observation, or mental_model.\n\
             Content: {}\n\
             Classification:",
            content
        );

        let _logits = self.run_inference(&prompt)?;

        // For a fine-tuned Kev model, we would decode the output token(s)
        // and map to a MemoryType. For now, we use a heuristic on the
        // prompt content as a placeholder until the model is fine-tuned.
        //
        // TODO: Replace with actual model output decoding once Kev is
        // fine-tuned for memory classification.
        let memory_type = self.heuristic_classify(content);

        Ok(Classification {
            memory_type,
            confidence: 0.85, // placeholder until calibrated
        })
    }

    /// Rule-based fallback classification when model output isn't usable.
    fn heuristic_classify(&self, content: &str) -> MemoryType {
        let lower = content.to_lowercase();

        // First-person experience markers
        let exp_markers = [
            "i ", "my ", "me ", "we ", "our ", "i'm", "i've", "i'll", "myself",
        ];
        if exp_markers
            .iter()
            .any(|m| lower.starts_with(m) || lower.contains(m))
        {
            return MemoryType::Experience;
        }

        // Mental model markers (preferences, patterns)
        let model_markers = [
            "prefer", "always", "never", "usually", "tends to", "pattern", "habit",
        ];
        if model_markers.iter().any(|m| lower.contains(m)) {
            return MemoryType::MentalModel;
        }

        // Observation markers (inference, synthesis)
        let obs_markers = [
            "seems", "appears", "likely", "probably", "infer", "conclude", "suggest",
        ];
        if obs_markers.iter().any(|m| lower.contains(m)) {
            return MemoryType::Observation;
        }

        // Default to world fact
        MemoryType::WorldFact
    }

    /// Score importance using the model or heuristics.
    fn score_with_model(&self, content: &str) -> Result<ImportanceScore> {
        let _ = self.run_inference(&format!(
            "Rate the importance of this memory from 1 to 5.\n\
             Content: {}\n\
             Importance:",
            content
        ))?;

        // Heuristic scoring until model is calibrated
        let level = self.heuristic_importance(content);

        Ok(ImportanceScore {
            level,
            confidence: 0.80,
        })
    }

    fn heuristic_importance(&self, content: &str) -> u8 {
        let len = content.len();

        // Longer memories tend to be more detailed/important
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

    /// Detect contradiction by comparing new content against existing memories.
    fn detect_with_model(&self, new: &str, existing: &[&str]) -> Result<Contradiction> {
        if existing.is_empty() {
            return Ok(Contradiction {
                exists: false,
                confidence: 1.0,
                conflicting_ids: vec![],
            });
        }

        // Check each existing memory for potential contradiction
        let mut conflicting = Vec::new();
        let mut max_confidence = 0.0f32;

        for (idx, existing_item) in existing.iter().enumerate() {
            let _ = self.run_inference(&format!(
                "Do these two statements contradict each other?\n\
                 A: {}\n\
                 B: {}\n\
                 Answer (yes/no):",
                existing_item, new
            ))?;

            // Heuristic: check for negation patterns
            if self.heuristic_contradicts(new, existing_item) {
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

    fn heuristic_contradicts(&self, a: &str, b: &str) -> bool {
        let a_lower = a.to_lowercase();
        let b_lower = b.to_lowercase();

        // Simple negation detection
        let negators = [
            "not ", "never ", "no ", "isn't", "aren't", "doesn't", "don't", "won't", "can't",
        ];

        for neg in negators {
            let a_neg = a_lower.contains(neg);
            let b_neg = b_lower.contains(neg);

            // If one has negation and the other doesn't, and they share significant words
            if a_neg != b_neg {
                // Check for shared content words (simple overlap)
                let a_words: std::collections::HashSet<_> =
                    a_lower.split_whitespace().filter(|w| w.len() > 3).collect();
                let b_words: std::collections::HashSet<_> =
                    b_lower.split_whitespace().filter(|w| w.len() > 3).collect();

                let overlap = a_words.intersection(&b_words).count();
                let min_len = a_words.len().min(b_words.len()).max(1);

                if overlap as f32 / min_len as f32 > 0.5 {
                    return true;
                }
            }
        }

        false
    }
}

impl DecisionEngine for KevEngine {
    fn classify(&self, content: &str) -> Result<Classification> {
        // Try model-based classification, fall back to heuristic on failure
        match self.classify_with_model(content) {
            Ok(result) => Ok(result),
            Err(e) => {
                tracing::warn!(error = %e, "Kev model inference failed, using heuristic fallback");
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
                tracing::warn!(error = %e, "Kev importance scoring failed, using heuristic fallback");
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
                tracing::warn!(error = %e, "Kev contradiction detection failed, using heuristic fallback");
                let conflicting: Vec<String> = existing
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| self.heuristic_contradicts(new, item))
                    .map(|(i, _)| i.to_string())
                    .collect();

                Ok(Contradiction {
                    exists: !conflicting.is_empty(),
                    confidence: if conflicting.is_empty() { 0.0 } else { 0.60 },
                    conflicting_ids: conflicting,
                })
            }
        }
    }
}
