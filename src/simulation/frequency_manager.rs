// src/simulation/frequency_manager.rs

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Mutex;

// --- Data Structures ---

// Holds the master frequency data, loaded once.
struct FrequencyData {
    lemma_to_rank: HashMap<String, u32>,
    rank_to_lemma: Vec<String>,
}

// --- Global Static Variable ---

// Use a Mutex to allow for safe, one-time initialization.
static FREQUENCY_DATA: Lazy<Mutex<Option<FrequencyData>>> = Lazy::new(|| Mutex::new(None));

// --- Public Functions ---

/// Loads the master frequency list from the specified asset path.
/// This MUST be called once at the start of the application.
pub fn load_master_frequency_list(asset_path: &Path) -> Result<(), String> {
    let mut guard = FREQUENCY_DATA.lock().unwrap();
    if guard.is_some() {
        return Ok(()); // Already loaded
    }

    println!("[INFO] Loading master frequency list from: {}", asset_path.display());
    let file = File::open(asset_path)
        .map_err(|e| format!("Failed to open frequency list at '{}': {}", asset_path.display(), e))?;
    let reader = BufReader::new(file);

    let mut temp_rank_to_lemma: Vec<(u32, String)> = Vec::new();

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| format!("Failed to read line from frequency list: {}", e))?;
        
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let lemma_str = parts[0].trim().to_lowercase();
            if let Ok(rank) = parts[1].parse::<u32>() {
                if !lemma_str.is_empty() {
                    temp_rank_to_lemma.push((rank, lemma_str));
                }
            }
        }
    }
    
    // Sort by the rank column to ensure correct order, just in case
    temp_rank_to_lemma.sort_by_key(|k| k.0);

    let rank_to_lemma: Vec<String> = temp_rank_to_lemma.into_iter().map(|(_, s)| s).collect();

    let mut lemma_to_rank = HashMap::new();
    for (i, lemma) in rank_to_lemma.iter().enumerate() {
        lemma_to_rank.insert(lemma.clone(), (i + 1) as u32);
    }
    
    if rank_to_lemma.is_empty() {
        return Err("Frequency list is empty or could not be parsed.".to_string());
    }

    println!("[INFO] Loaded {} unique lemmas into frequency manager.", rank_to_lemma.len());
    *guard = Some(FrequencyData { lemma_to_rank, rank_to_lemma });
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
    guard.as_ref().expect("Master frequency list has not been loaded.").lemma_to_rank.get(lemma).copied()
}