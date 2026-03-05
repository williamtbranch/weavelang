// src/global_settings.rs
//
// App-level (per-user) settings that persist across workspaces.
// Lives in the OS config directory: e.g.
//   Windows: C:\Users\<user>\AppData\Roaming\weavelang\settings.toml
//   Linux:   ~/.config/weavelang/settings.toml
//   macOS:   ~/Library/Application Support/weavelang/settings.toml

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct GlobalSettings {
    /// Path to the last opened workspace directory.
    pub last_workspace: Option<String>,
}

/// Returns the path to the global settings file.
pub fn settings_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    });
    base.join("weavelang").join("settings.toml")
}

impl GlobalSettings {
    /// Load from disk, returning defaults if the file doesn't exist or is malformed.
    pub fn load() -> Self {
        let path = settings_path();
        if let Ok(contents) = fs::read_to_string(&path) {
            toml::from_str(&contents).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Persist to disk, creating the directory if needed.
    pub fn save(&self) -> Result<(), String> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Cannot create settings dir: {e}"))?;
        }
        let contents = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, contents).map_err(|e| format!("Cannot write settings: {e}"))
    }

    /// Update the stored workspace path and save immediately.
    pub fn set_workspace(&mut self, path: &str) {
        self.last_workspace = Some(path.to_string());
    }
}
