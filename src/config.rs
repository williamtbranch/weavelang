use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub content_project_dir: String,
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
            Err(e) => Err(format!("Failed to parse {}: {}", file_path, e)),
        },
        Err(e) => Err(format!(
            "Failed to read {}: {}. Please ensure it exists.",
            file_path, e
        )),
    }
}
