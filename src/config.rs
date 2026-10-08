// Configuration management
// All config stored in SQLite, editable via Web UI

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub port: u16,
    pub storage_path: String,
    pub kev_model: String,
    pub kev_model_path: PathBuf,
    pub kev_tokenizer_path: PathBuf,
    pub night_model: String,
    pub night_schedule: String,
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
        let mimir_dir = home.join(".mimir");

        Self {
            port: 8080,
            storage_path: mimir_dir.to_string_lossy().to_string(),
            kev_model: "kev-0.8b".to_string(),
            kev_model_path: mimir_dir.join("models").join("kev-0.8b.onnx"),
            kev_tokenizer_path: mimir_dir.join("models").join("tokenizer.json"),
            night_model: "qwen2.5-7b".to_string(),
            night_schedule: "0 2 * * *".to_string(),
        }
    }
}
