// In src/config.rs

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::collections::HashMap;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub open_last_project: Option<bool>,
    pub last_project_file: Option<String>,
    pub content_project_dir: String,
    pub custom_frequency_list_path: Option<String>,
    pub output_dir: Option<String>,
    pub models: HashMap<String, ModelConfig>,
    pub pipeline: PipelineConfig,
    pub stages: HashMap<String, StageConfig>,
    pub copilot_server_name: Option<String>,
    pub copilot_server_port: Option<u16>,
    pub copilot: Option<CopilotConfig>,
    /// Workspace-wide path to YouTube OAuth client_secret JSON file.
    pub youtube_client_secret_file: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct CopilotConfig {
    /// Model alias from the [models] table. Empty or missing = copilot disabled.
    pub model: Option<String>,
    /// Maximum LLM round-trips per autonomous session (safety cap).
    pub max_turns: Option<u32>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ModelConfig {
    pub provider: String,
    pub name: String,
    pub max_input_tokens: usize,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct PipelineConfig {
    pub max_api_retries: u32,
    pub max_validation_retries: u32,
    pub retry_delay: u32,
    pub thinking_budget_tokens: Option<u32>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
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

impl Default for Config {
    /// Sensible defaults matching config.toml.example — no personal paths, no credentials.
    /// Used when creating a fresh workspace and for schema inspection before a workspace is open.
    fn default() -> Self {
        let mut models = HashMap::new();
        for (key, provider, name, tokens) in &[
            ("gemini-flash-lite", "gemini", "gemini-2.5-flash-lite-preview-06-17", 10000usize),
            ("gemini-pro",        "gemini", "models/gemini-2.5-pro",               20000),
            ("gemini-flash",      "gemini", "gemini-2.5-flash",                    10000),
            ("haiku",             "claude", "claude-3-5-haiku-20241022",            4000),
            ("sonnet",            "claude", "claude-sonnet-4-20250514",            20000),
            ("opus",              "claude", "claude-opus-4-1-20250805",            20000),
        ] {
            models.insert(key.to_string(), ModelConfig {
                provider: provider.to_string(),
                name: name.to_string(),
                max_input_tokens: *tokens,
            });
        }

        let mut stages = HashMap::new();
        for (name, batch) in &[
            ("Segmenter",               1usize),
            ("GenerateBasicBase",       10),
            ("GenerateAdvancedTarget",  20),
            ("GenerateModerateTarget",  10),
            ("GenerateBasicTarget",     20),
            ("GeneratePhraseMap",        5),
            ("GenerateInversePhraseMap", 5),
        ] {
            stages.insert(name.to_string(), StageConfig {
                primary_model: "gemini-pro".to_string(),
                fallback_model: Some("sonnet".to_string()),
                batch_size_in_items: *batch,
                thinking_budget_tokens: None,
                thinking_on_first_attempt: None,
            });
        }

        Config {
            open_last_project: Some(true),
            last_project_file: None,
            content_project_dir: String::new(),
            custom_frequency_list_path: None,
            output_dir: None,
            models,
            pipeline: PipelineConfig {
                max_api_retries: 3,
                max_validation_retries: 3,
                retry_delay: 7,
                thinking_budget_tokens: Some(1024),
            },
            stages,
            copilot_server_name: None,
            copilot_server_port: None,
            copilot: None,
            youtube_client_secret_file: None,
        }
    }
}

/// Load config from a workspace directory (looks for `config.toml` inside).
pub fn load_config_from_workspace_dir(workspace_path: &std::path::Path) -> Result<Config, String> {
    let config_path = workspace_path.join("config.toml");
    load_config_from_file(config_path.to_str().unwrap_or(""))
}

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

pub fn save_config_to_file(config: &Config, file_path: &std::path::Path) -> Result<(), String> {
    let contents = toml::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(file_path, contents).map_err(|e| format!("Failed to write config: {e}"))
}
