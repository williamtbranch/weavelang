// src/services/llm_logger.rs

use chrono::Local;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone)]
pub struct LlmLogger {
    log_path: PathBuf,
}

impl LlmLogger {
    // UPDATED: Now accepts the specific destination directory
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            log_path: output_dir.join("studio_llm.log"),
        }
    }

    /// Returns the path to the LLM log file.
    pub fn log_file_path(&self) -> &PathBuf {
        &self.log_path
    }

    pub fn log_interaction(&self, context: &str, system: &str, user: &str, response: &str) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");

        let log_entry = format!(
            "\n========================================\n\
             [{timestamp}] CONTEXT: {context}\n\
             ========================================\n\
             --- SYSTEM PROMPT ---\n\
             {system}\n\
             \n\
             --- USER PROMPT ---\n\
             {user}\n\
             \n\
             --- LLM RESPONSE ---\n\
             {response}\n\
             \n"
        );

        // Ensure directory exists (basic check)
        if let Some(parent) = self.log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = file.write_all(log_entry.as_bytes());
        } else {
            eprintln!(
                "[LlmLogger] Failed to write to log file: {:?}",
                self.log_path
            );
        }
    }
}
