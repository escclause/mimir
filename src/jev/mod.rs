// Jev decision engine
// Trait + implementations (D1, mock)

pub mod d1;
pub mod mock;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classification {
    pub memory_type: MemoryType,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MemoryType {
    WorldFact,
    Experience,
    Observation,
    MentalModel,
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::WorldFact => "world_fact",
            MemoryType::Experience => "experience",
            MemoryType::Observation => "observation",
            MemoryType::MentalModel => "mental_model",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "world_fact" => MemoryType::WorldFact,
            "experience" => MemoryType::Experience,
            "observation" => MemoryType::Observation,
            "mental_model" => MemoryType::MentalModel,
            _ => MemoryType::WorldFact,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportanceScore {
    pub level: u8, // 1-5
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contradiction {
    pub exists: bool,
    pub confidence: f32,
    pub conflicting_ids: Vec<String>,
}

pub trait DecisionEngine: Send + Sync {
    fn classify(&self, content: &str) -> Result<Classification>;
    fn score_importance(&self, content: &str) -> Result<ImportanceScore>;
    fn detect_contradiction(&self, new: &str, existing: &[&str]) -> Result<Contradiction>;
}
