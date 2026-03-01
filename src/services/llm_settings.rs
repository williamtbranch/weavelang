use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StageBatchSettings {
    pub simplify: usize,
    pub mapping: usize,
    pub translate: usize,
}

impl Default for StageBatchSettings {
    fn default() -> Self {
        Self {
            simplify: 10,
            mapping: 3,
            translate: 10,
        }
    }
}

impl StageBatchSettings {
    pub fn load(project_root: &PathBuf) -> Self {
        let cfg_dir = project_root.join(".weavelang");
        let cfg_file = cfg_dir.join("studio_settings.toml");
        if cfg_file.exists() {
            if let Ok(s) = fs::read_to_string(&cfg_file) {
                if let Ok(parsed) = toml::from_str::<StageBatchSettings>(&s) {
                    return parsed;
                }
            }
        }
        Default::default()
    }

    pub fn save(&self, project_root: &PathBuf) -> Result<(), String> {
        let cfg_dir = project_root.join(".weavelang");
        if let Err(e) = fs::create_dir_all(&cfg_dir) {
            return Err(format!("Failed to create config dir: {}", e));
        }
        let cfg_file = cfg_dir.join("studio_settings.toml");
        match toml::to_string(self) {
            Ok(serialized) => fs::write(&cfg_file, serialized).map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_save_load_roundtrip() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let tmp = env::temp_dir().join(format!("weavelang_settings_test_{now}"));
        let _ = fs::create_dir_all(&tmp);

        let settings = StageBatchSettings {
            simplify: 5,
            mapping: 2,
            translate: 7,
        };

        assert!(settings.save(&tmp).is_ok());

        let loaded = StageBatchSettings::load(&tmp);
        assert_eq!(loaded.simplify, 5);
        assert_eq!(loaded.mapping, 2);
        assert_eq!(loaded.translate, 7);
    }
}
