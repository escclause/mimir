// Mock rule-based decision engine
// Fallback when Kev-0.8B is not available

use anyhow::Result;

use super::{Classification, Contradiction, DecisionEngine, ImportanceScore, MemoryType};

pub struct MockEngine;

impl MockEngine {
    pub fn new() -> Self {
        Self
    }
}

impl DecisionEngine for MockEngine {
    fn classify(&self, content: &str) -> Result<Classification> {
        let content_lower = content.to_lowercase();

        // Simple heuristics
        let memory_type = if content_lower.contains("i ") && content_lower.contains("remember") {
            MemoryType::Experience
        } else if content_lower.contains("preference") || content_lower.contains("always") || content_lower.contains("never") {
            MemoryType::MentalModel
        } else if content_lower.contains("observed") || content_lower.contains("noticed") {
            MemoryType::Observation
        } else {
            MemoryType::WorldFact
        };

        Ok(Classification {
            memory_type,
            confidence: 0.7,
        })
    }

    fn score_importance(&self, content: &str) -> Result<ImportanceScore> {
        let content_lower = content.to_lowercase();

        // Simple heuristics for importance
        let level = if content_lower.contains("critical") || content_lower.contains("important") || content_lower.contains("always") {
            5
        } else if content_lower.contains("preference") || content_lower.contains("usually") {
            4
        } else if content_lower.contains("sometimes") || content_lower.contains("maybe") {
            3
        } else {
            2
        };

        Ok(ImportanceScore {
            level,
            confidence: 0.6,
        })
    }

    fn detect_contradiction(&self, new: &str, existing: &[&str]) -> Result<Contradiction> {
        let new_lower = new.to_lowercase();

        // Simple contradiction detection
        for (i, old) in existing.iter().enumerate() {
            let old_lower = old.to_lowercase();

            // Check for negation patterns
            if (new_lower.contains("is") && old_lower.contains("is not"))
                || (new_lower.contains("not") && !old_lower.contains("not"))
            {
                return Ok(Contradiction {
                    exists: true,
                    confidence: 0.5,
                    conflicting_ids: vec![i.to_string()],
                });
            }
        }

        Ok(Contradiction {
            exists: false,
            confidence: 0.8,
            conflicting_ids: vec![],
        })
    }
}
