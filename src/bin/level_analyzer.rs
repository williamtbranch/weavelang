use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

// --- Configuration ---
const FILE_PATH: &str = "assets/frequency_lists/es_master_frequency_list.txt";
const TARGET_PERCENTAGE_INCREASE: f64 = 2.0;
const TOTAL_LEVELS: u32 = 50; // This is now a suggestion, we might get fewer levels.
const MINIMUM_NEW_WORDS_PER_LEVEL: u32 = 3;

// --- New Tapering Configuration ---
const TAPERING_THRESHOLD: u32 = 1000; // Switch to tapering when a level adds this many words.
const TAPERING_MULTIPLIER: u32 = 2; // Multiplier for word count increase during tapering.

fn main() -> Result<(), Box<dyn Error>> {
    // --- Pass 1: Get totals for occurrences and total number of lemmas ---
    println!("Pass 1: Calculating totals...");
    // We now get both total occurrences and total lines (lemmas)
    let (total_occurrences, total_lemmas) = get_file_totals(FILE_PATH)?;
    println!(
        "Total occurrences found: {}",
        format_number(total_occurrences)
    );
    println!(
        "Total lemmas found: {}\n",
        format_number(total_lemmas as u64)
    );

    // --- Pass 2: Determine level cutoffs with the full 3-phase logic ---
    println!("Pass 2: Determining level cutoffs...");
    println!("{:-<100}", "");
    println!(
        "{:<5} | {:<20} | {:<18} | {:<15} | {:<30}",
        "Level", "Vocab Size (Rank)", "New Words Added", "Cumulative %", "Threshold-crossing Lemma"
    );
    println!("{:-<100}", "");

    let file = File::open(FILE_PATH)?;
    let reader = BufReader::new(file);

    let mut cumulative_occurrences: u64 = 0;
    let mut current_level: u32 = 1;
    let mut last_level_rank: u32 = 0;
    let mut last_level_percentage: f64 = 0.0;

    // --- State variables for the tapering logic ---
    let mut is_tapering_active = false;
    let mut last_taper_increase: u32 = 0;

    for line_result in reader.lines().skip(1) {
        if current_level > TOTAL_LEVELS {
            break;
        }

        let line = line_result?;
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let lemma = parts[0];
        let rank: u32 = parts[1].parse()?;
        let occurrences: u64 = parts[2].parse()?;
        cumulative_occurrences += occurrences;

        if !is_tapering_active {
            // --- PHASE 1: Standard Hybrid Logic ---
            let current_percentage =
                (cumulative_occurrences as f64 / total_occurrences as f64) * 100.0;
            let new_words = rank - last_level_rank;
            let percentage_increase = current_percentage - last_level_percentage;

            if new_words >= MINIMUM_NEW_WORDS_PER_LEVEL
                && percentage_increase >= TARGET_PERCENTAGE_INCREASE
            {
                print_level(current_level, rank, new_words, current_percentage, lemma);

                // Update state
                last_level_rank = rank;
                last_level_percentage = current_percentage;
                current_level += 1;

                // --- PHASE 2: Check if we should TRIGGER tapering for the *next* level ---
                if new_words >= TAPERING_THRESHOLD {
                    is_tapering_active = true;
                    last_taper_increase = new_words;
                }
            }
        } else {
            // --- PHASE 3: Tapering Logic is Active ---
            let next_increase = last_taper_increase * TAPERING_MULTIPLIER;
            let mut target_rank = last_level_rank + next_increase;

            // Look-ahead: If the *next* step would overshoot, make this step the final one.
            let next_next_increase = next_increase * TAPERING_MULTIPLIER;
            if target_rank + next_next_increase > total_lemmas && rank < total_lemmas {
                target_rank = total_lemmas;
            }

            if rank >= target_rank {
                let current_percentage =
                    (cumulative_occurrences as f64 / total_occurrences as f64) * 100.0;
                let new_words = rank - last_level_rank;
                print_level(current_level, rank, new_words, current_percentage, lemma);

                // Update state
                last_level_rank = rank;
                current_level += 1;
                // Important: The *next* increase is based on the *calculated* target, not the actual.
                last_taper_increase = if target_rank == total_lemmas {
                    new_words
                } else {
                    next_increase
                };

                // If this was the final level, stop processing
                if rank >= total_lemmas {
                    break;
                }
            }
        }
    }

    println!("{:-<100}", "");
    println!("Analysis complete.");
    Ok(())
}

/// Helper to print a formatted level row.
fn print_level(level: u32, rank: u32, new_words: u32, percentage: f64, lemma: &str) {
    println!(
        "{:<5} | {:<20} | {:<18} | {:<15.2}% | {:<30}",
        level,
        format_number(rank as u64),
        format_number(new_words as u64),
        percentage,
        lemma
    );
}

/// Reads the file once to get total occurrences and total number of lines (lemmas).
fn get_file_totals<P: AsRef<Path>>(path: P) -> io::Result<(u64, u32)> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut total_occurrences = 0;
    let mut total_lines = 0;

    for line_result in reader.lines().skip(1) {
        // Skip header
        let line = line_result?;
        total_lines += 1;
        if let Some(occurrences_str) = line.split('\t').nth(2) {
            if let Ok(occurrences) = occurrences_str.parse::<u64>() {
                total_occurrences += occurrences;
            }
        }
    }
    Ok((total_occurrences, total_lines))
}

/// Helper to format large numbers with commas.
fn format_number(n: u64) -> String {
    n.to_string()
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(std::str::from_utf8)
        .collect::<Result<Vec<&str>, _>>()
        .unwrap()
        .join(",")
}
