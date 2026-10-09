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
    pub d1_mmproj_path: PathBuf,
    pub kev_port: u16,
    /// External llama-server URL for D1 (if already running)
    /// If None, Mímir will try to spawn a managed llama-server
    pub d1_server_url: Option<String>,
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
            kev_model: "d1-omni-600m".to_string(),
            kev_model_path: mimir_dir
                .join("models")
                .join("d1-omni-600m")
                .join("d1-omni-600M-Q8_0.gguf"),
            kev_tokenizer_path: mimir_dir
                .join("models")
                .join("d1-omni-600m")
                .join("tokenizer.json"),
            d1_mmproj_path: mimir_dir
                .join("models")
                .join("d1-omni-600m")
                .join("mmproj-d1-omni-600M-Q8_0.gguf"),
            kev_port: 8081,
            d1_server_url: None, // Will try connect, then spawn
            night_model: "qwen2.5-7b".to_string(),
            night_schedule: "0 2 * * *".to_string(),
        }
    }
}
