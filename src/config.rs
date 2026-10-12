// Configuration management
// Layered: defaults → file (~/.mimir/config.toml) → env vars

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
    /// Night shift model name
    pub night_model: String,
    /// Night shift model path (GGUF)
    pub night_model_path: PathBuf,
    /// Night shift llama-server port
    pub night_port: u16,
    /// Night shift schedule (cron expression)
    pub night_schedule: String,
    /// REST API port
    pub rest_port: u16,
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
            night_model: "lfm2.5-2.6b".to_string(),
            night_model_path: mimir_dir
                .join("models")
                .join("lfm2.5-2.6b")
                .join("LFM2.5-2.6B-Q4_0.gguf"),
            night_port: 8082,
            night_schedule: "0 2 * * *".to_string(),
            rest_port: 8083,
        }
    }
}

impl Config {
    /// Load configuration with layered overrides:
    /// 1. Start with defaults
    /// 2. Override from ~/.mimir/config.toml if exists
    /// 3. Override from MIMIR_* environment variables
    pub fn load() -> Self {
        let mut config = Self::default();

        // Layer 2: Load from file
        let config_path = Self::config_file_path();
        if config_path.exists() {
            match Self::load_from_file(&config_path) {
                Ok(file_config) => {
                    tracing::info!(path = %config_path.display(), "Loaded config from file");
                    config = file_config;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load config file, using defaults");
                }
            }
        }

        // Layer 3: Environment variable overrides
        config.apply_env_overrides();

        config
    }

    /// Get the config file path (~/.mimir/config.toml)
    fn config_file_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
        home.join(".mimir").join("config.toml")
    }

    /// Load config from a TOML file
    fn load_from_file(path: &PathBuf) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Save current config to file
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_file_path();
        
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = toml::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;
        
        tracing::info!(path = %path.display(), "Saved config to file");
        Ok(())
    }

    /// Apply environment variable overrides
    /// Format: MIMIR_<FIELD_NAME> in SCREAMING_SNAKE_CASE
    fn apply_env_overrides(&mut self) {
        // Port overrides
        if let Ok(val) = std::env::var("MIMIR_PORT") {
            if let Ok(port) = val.parse() {
                self.port = port;
            }
        }
        if let Ok(val) = std::env::var("MIMIR_REST_PORT") {
            if let Ok(port) = val.parse() {
                self.rest_port = port;
            }
        }
        if let Ok(val) = std::env::var("MIMIR_KEV_PORT") {
            if let Ok(port) = val.parse() {
                self.kev_port = port;
            }
        }
        if let Ok(val) = std::env::var("MIMIR_NIGHT_PORT") {
            if let Ok(port) = val.parse() {
                self.night_port = port;
            }
        }

        // Path overrides
        if let Ok(val) = std::env::var("MIMIR_STORAGE_PATH") {
            self.storage_path = val;
        }
        if let Ok(val) = std::env::var("MIMIR_KEV_MODEL_PATH") {
            self.kev_model_path = PathBuf::from(val);
        }
        if let Ok(val) = std::env::var("MIMIR_NIGHT_MODEL_PATH") {
            self.night_model_path = PathBuf::from(val);
        }

        // Model overrides
        if let Ok(val) = std::env::var("MIMIR_KEV_MODEL") {
            self.kev_model = val;
        }
        if let Ok(val) = std::env::var("MIMIR_NIGHT_MODEL") {
            self.night_model = val;
        }

        // Schedule override
        if let Ok(val) = std::env::var("MIMIR_NIGHT_SCHEDULE") {
            self.night_schedule = val;
        }

        // D1 server URL override
        if let Ok(val) = std::env::var("MIMIR_D1_SERVER_URL") {
            self.d1_server_url = Some(val);
        }
    }

    /// Get a summary of current configuration (for logging)
    pub fn summary(&self) -> String {
        format!(
            "Mímir Config:\n  \
            REST Port: {}\n  \
            D1 Model: {} (port {})\n  \
            Night Model: {} (port {})\n  \
            Night Schedule: {}\n  \
            Storage: {}",
            self.rest_port,
            self.kev_model,
            self.kev_port,
            self.night_model,
            self.night_port,
            self.night_schedule,
            self.storage_path
        )
    }
}
