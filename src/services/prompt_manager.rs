// src/services/prompt_manager.rs

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
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
    ///
    /// `base_code`/`target_code` are the *per-step* input/output languages of
    /// the operation (e.g. `es`-`es` for segmentation/simplification, `en`-`es`
    /// for a translation), not necessarily the project's master language pair.
    /// See `tier_graph::prompt_pair_for_stage`.
    pub fn get_prompt(
        &self,
        prompt_name: &str,
        base_code: &str,
        target_code: &str,
    ) -> Result<String, String> {
        // Cache key includes language pair to avoid collisions if defaults differ
        let cache_key = format!("{prompt_name}:{base_code}-{target_code}");

        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| "Failed to lock prompt cache")?;
            if let Some(content) = cache.get(&cache_key) {
                return Ok(content.clone());
            }
        }

        // 1. Construct pair-specific path: e.g., "assets/prompts/es-es/moderate.txt"
        let pair_dir = format!("{base_code}-{target_code}");
        let pair_path = self
            .root_dir
            .join("assets")
            .join("prompts")
            .join(&pair_dir)
            .join(format!("{prompt_name}.txt"));

        if pair_path.exists() {
            let content = fs::read_to_string(&pair_path)
                .map_err(|e| format!("Failed to read pair prompt {pair_path:?}: {e}"))?;

            let mut cache = self
                .cache
                .lock()
                .map_err(|_| "Failed to lock prompt cache")?;
            cache.insert(cache_key, content.clone());
            println!("[PromptManager] Loaded '{prompt_name}' from {pair_dir}");
            return Ok(content);
        }

        // 2. Fallback to _defaults
        let default_path = self
            .root_dir
            .join("assets")
            .join("prompts")
            .join("_defaults")
            .join(format!("{prompt_name}.txt"));

        if default_path.exists() {
            let content = fs::read_to_string(&default_path)
                .map_err(|e| format!("Failed to read default prompt {default_path:?}: {e}"))?;

            let mut cache = self
                .cache
                .lock()
                .map_err(|_| "Failed to lock prompt cache")?;
            cache.insert(cache_key, content.clone());
            println!("[PromptManager] Loaded '{prompt_name}' from _defaults (Fallback)");
            return Ok(content);
        }

        Err(format!(
            "Prompt '{prompt_name}' not found in '{pair_dir}' or '_defaults'"
        ))
    }

    /// Renders a prompt by replacing `{{KEY}}` placeholders with values from `vars`.
    pub fn render_prompt(
        &self,
        prompt_name: &str,
        base_code: &str,
        target_code: &str,
        vars: &HashMap<String, String>,
    ) -> Result<String, String> {
        let mut template = self.get_prompt(prompt_name, base_code, target_code)?;

        for (key, val) in vars {
            // Use strict replacement for now: {{KEY}}
            let placeholder = format!("{{{{{}}}}}", key);
            template = template.replace(&placeholder, val);
        }

        Ok(template)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_get_prompt_fallback_defaults() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let tmp = env::temp_dir().join(format!("weavelang_prompt_test_{now}"));
        let defaults_dir = tmp.join("assets").join("prompts").join("_defaults");
        fs::create_dir_all(&defaults_dir).expect("create defaults dir");

        let prompt_name = "test_default_prompt";
        let content = "This is the default prompt.";
        fs::write(defaults_dir.join(format!("{prompt_name}.txt")), content).expect("write prompt");

        let pm = PromptManager::new(tmp.clone());
        let got = pm.get_prompt(prompt_name, "en", "es").expect("should load");
        assert_eq!(got, content.to_string());
    }
}
