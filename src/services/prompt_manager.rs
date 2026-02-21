// src/services/prompt_manager.rs

use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct PromptManager {
    root_dir: PathBuf,
    cache: Arc<Mutex<HashMap<String, String>>>,
}

impl PromptManager {
    pub fn new(root_dir: PathBuf) -> Self {
        Self {
            root_dir,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Loads a prompt. Checks `assets/prompts/{base}-{target}/` first, then `_defaults`.
    pub fn get_prompt(&self, prompt_name: &str, base_code: &str, target_code: &str) -> Result<String, String> {
        // Cache key includes language pair to avoid collisions if defaults differ
        let cache_key = format!("{}:{}-{}", prompt_name, base_code, target_code);
        
        {
            let cache = self.cache.lock().map_err(|_| "Failed to lock prompt cache")?;
            if let Some(content) = cache.get(&cache_key) {
                return Ok(content.clone());
            }
        }

        // 1. Construct pair-specific path: e.g., "assets/prompts/en-es/simplify_segments_moderate.txt"
        let pair_dir = format!("{}-{}", base_code, target_code);
        let pair_path = self.root_dir
            .join("assets")
            .join("prompts")
            .join(&pair_dir)
            .join(format!("{}.txt", prompt_name));

        if pair_path.exists() {
            let content = fs::read_to_string(&pair_path)
                .map_err(|e| format!("Failed to read pair prompt {:?}: {}", pair_path, e))?;
            
            let mut cache = self.cache.lock().map_err(|_| "Failed to lock prompt cache")?;
            cache.insert(cache_key, content.clone());
            println!("[PromptManager] Loaded '{}' from {}", prompt_name, pair_dir);
            return Ok(content);
        }

        // 2. Fallback to _defaults
        let default_path = self.root_dir
            .join("assets")
            .join("prompts")
            .join("_defaults")
            .join(format!("{}.txt", prompt_name));

        if default_path.exists() {
            let content = fs::read_to_string(&default_path)
                .map_err(|e| format!("Failed to read default prompt {:?}: {}", default_path, e))?;
            
            let mut cache = self.cache.lock().map_err(|_| "Failed to lock prompt cache")?;
            cache.insert(cache_key, content.clone());
            println!("[PromptManager] Loaded '{}' from _defaults (Fallback)", prompt_name);
            return Ok(content);
        }

        Err(format!("Prompt '{}' not found in '{}' or '_defaults'", prompt_name, pair_dir))
    }
}