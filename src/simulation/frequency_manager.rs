// src/simulation/frequency_manager.rs

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// --- Data Structures ---

struct FrequencyData {
    lemma_to_rank: HashMap<String, u32>,
    rank_to_lemma: Vec<String>,
}

// --- Global Static Variables ---

static FREQUENCY_DATA: Lazy<Mutex<Option<FrequencyData>>> = Lazy::new(|| Mutex::new(None));
// This separate static variable tracks the path of the file that was loaded.
static LOADED_PATH: Lazy<Mutex<Option<PathBuf>>> = Lazy::new(|| Mutex::new(None));


// --- Public Functions ---

/// Loads the master frequency list from the specified asset path.
/// This MUST be called once at the start of the application.
/// It is now robust against test runs polluting the global state.
pub fn load_master_frequency_list(asset_path: &Path) -> Result<(), String> {
    let mut guard = FREQUENCY_DATA.lock().unwrap();
    let mut path_guard = LOADED_PATH.lock().unwrap();

    // If data is already loaded, check if it's from the SAME file path.
    if let Some(loaded_path) = path_guard.as_ref() {
        if loaded_path == asset_path && guard.is_some() {
            return Ok(()); // Already loaded from the correct path, so we can skip.
        }
        // If the path is different (e.g., test vs. real), we proceed to reload.
    }

    println!("[INFO] Loading master frequency list from: {}", asset_path.display());
    let file = File::open(asset_path)
        .map_err(|e| format!("Failed to open frequency list at '{}': {}", asset_path.display(), e))?;
    let reader = BufReader::new(file);

    let mut temp_data: Vec<(String, u32)> = Vec::new();
    let mut lines_read = 0;
    let mut valid_lines_parsed = 0;

    // Skip the header line and iterate through the rest
    for (i, line_result) in reader.lines().skip(1).enumerate() {
        lines_read += 1;
        let line = match line_result {
            Ok(l) => l,
            Err(_) => {
                eprintln!("[WARN] Failed to read line {} of frequency list.", i + 2);
                continue;
            }
        };

        let parts: Vec<&str> = line.split('\t').collect();
        
        if parts.len() >= 2 {
            let lemma = parts[0].trim().to_string();
            if let Ok(rank) = parts[1].parse::<u32>() {
                if !lemma.is_empty() {
                    temp_data.push((lemma, rank));
                    valid_lines_parsed += 1;
                }
            }
        }
    }
    
    println!("[DEBUG] Frequency List Parser: Read {} data lines, successfully parsed {} valid entries.", lines_read, valid_lines_parsed);

    if temp_data.is_empty() {
        return Err("Frequency list is empty or could not be parsed.".to_string());
    }

    // Now, build the final structures
    let mut lemma_to_rank = HashMap::new();
    let mut rank_to_lemma_temp: Vec<(u32, String)> = Vec::new();

    for (lemma, rank) in temp_data {
        lemma_to_rank.insert(lemma.clone(), rank);
        rank_to_lemma_temp.push((rank, lemma));
    }

    rank_to_lemma_temp.sort_by_key(|k| k.0);
    let rank_to_lemma: Vec<String> = rank_to_lemma_temp.into_iter().map(|(_, s)| s).collect();
    
    println!("[INFO] Loaded {} unique lemmas into frequency manager.", lemma_to_rank.len());
    *guard = Some(FrequencyData { lemma_to_rank, rank_to_lemma });
    // Store the path of the file we just successfully loaded.
    *path_guard = Some(asset_path.to_path_buf());
    
    Ok(())
}

/// Retrieves the master frequency list ordered by rank.
/// Panics if the list has not been loaded.
pub fn get_ordered_lemmas() -> Vec<String> {
    let guard = FREQUENCY_DATA.lock().unwrap();
    guard.as_ref().expect("Master frequency list has not been loaded.").rank_to_lemma.clone()
}

/// Gets the rank for a given lemma string.
/// Panics if the list has not been loaded.
pub fn get_rank_for_lemma(lemma: &str) -> Option<u32> {
    let guard = FREQUENCY_DATA.lock().unwrap();
    // We trim the input lemma here as a final safety measure.
    guard.as_ref().expect("Master frequency list has not been loaded.").lemma_to_rank.get(lemma.trim()).copied()
}