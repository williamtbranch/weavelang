// src/domain/llm_log.rs
//
// Per-tier LLM call history stored inside the document model.
// Each Tier carries a Vec<LlmCallRecord> so the studio can show exactly
// what the LLM produced (or failed to produce) for every generation attempt
// on that sentence/tier pair, without hunting through log files.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmCallRecord {
    /// Wall-clock time of the call.
    pub timestamp: String,
    /// Pipeline stage name (e.g. "GenerateBasicBase").
    pub stage: String,
    /// Model key as defined in config (e.g. "gemini-pro").
    pub model: String,
    /// Text the LLM produced for THIS sentence/tier — empty on failure.
    #[serde(default)]
    pub generated_text: Option<String>,
    /// Error string if the call (or batch containing this sentence) failed.
    #[serde(default)]
    pub error: Option<String>,
    /// Whether the generated text was actually written to the tier.
    /// False for errors and for calls that produced output but were not
    /// applied (e.g. cancelled mid-job).
    #[serde(default = "default_true")]
    pub applied: bool,
}

pub fn default_true() -> bool {
    true
}

impl LlmCallRecord {
    pub fn success(stage: &str, model: &str, text: &str) -> Self {
        Self {
            timestamp: chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            stage: stage.to_string(),
            model: model.to_string(),
            generated_text: Some(text.to_string()),
            error: None,
            applied: true,
        }
    }

    pub fn failure(stage: &str, model: &str, error: &str) -> Self {
        Self {
            timestamp: chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            stage: stage.to_string(),
            model: model.to_string(),
            generated_text: None,
            error: Some(error.to_string()),
            applied: false,
        }
    }

    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}
