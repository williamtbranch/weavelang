// In src/config.rs

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::collections::HashMap;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Config {
    pub content_project_dir: String,
    pub custom_frequency_list_path: Option<String>,
    pub models: HashMap<String, ModelConfig>,
    pub pipeline: PipelineConfig,
    pub stages: HashMap<String, StageConfig>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ModelConfig {
    pub provider: String,
    pub name: String,
    pub max_input_tokens: usize,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PipelineConfig {
    pub max_api_retries: u32,
    pub max_validation_retries: u32,
    pub retry_delay: u32,
    pub thinking_budget_tokens: Option<u32>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct StageConfig {
    pub primary_model: String,
    pub fallback_model: Option<String>,
    pub batch_size_in_items: usize,
    pub thinking_budget_tokens: Option<u32>,
    pub thinking_on_first_attempt: Option<bool>,
}

// --- NEW HELPER METHOD ---
impl Config {
    pub fn content_project_dir_path(&self) -> PathBuf {
        PathBuf::from(&self.content_project_dir)
    }

    pub fn get_stage_config(&self, stage_name: &str) -> Option<&StageConfig> {
        self.stages.get(stage_name)
    }

    pub fn get_model_config(&self, model_key: &str) -> Option<&ModelConfig> {
        self.models.get(model_key)
    }
}
// --- END NEW HELPER METHOD ---

pub fn load_config_from_file(file_path: &str) -> Result<Config, String> {
    match fs::read_to_string(file_path) {
        Ok(contents) => match toml::from_str::<Config>(&contents) {
            Ok(loaded_config) => {
                let path = PathBuf::from(&loaded_config.content_project_dir);
                if path.is_dir() {
                    Ok(loaded_config)
                } else {
                    Err(format!(
                        "Error: content_project_dir specified in {} ('{}') is not a valid directory.",
                        file_path,
                        loaded_config.content_project_dir
                    ))
                }
            }
            Err(e) => Err(format!("Failed to parse {file_path}: {e}")),
        },
        Err(e) => Err(format!(
            "Failed to read {file_path}: {e}. Please ensure it exists."
        )),
    }
}
