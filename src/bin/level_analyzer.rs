use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::error::Error;

// --- Configuration ---
const FILE_PATH: &str = "assets/frequency_lists/es_master_frequency_list.txt";
const TARGET_PERCENTAGE_INCREASE: f64 = 0.02;
const TOTAL_LEVELS: u32 = 50;

fn main() -> Result<(), Box<dyn Error>> {
    // --- Pass 1: Calculate total occurrences ---
    println!("Pass 1: Calculating total occurrences...");
    let total_occurrences = calculate_total_occurrences(FILE_PATH)?;
    println!("Total occurrences found: {}\n", format_number(total_occurrences));

    // --- Pass 2: Determine level cutoffs ---
    println!("Pass 2: Determining level cutoffs...");
    println!("{:-<80}", "");
    println!(
        "{:<5} | {:<20} | {:<15} | {:<30}",
        "Level", "Vocab Size (Rank)", "Cumulative %", "Threshold-crossing Lemma"
    );
    println!("{:-<80}", "");

    let file = File::open(FILE_PATH)?;
    let reader = BufReader::new(file);

    let mut cumulative_occurrences: u64 = 0;
    let mut current_level: u32 = 1;

    // Skip header and iterate through lines
    for line_result in reader.lines().skip(1) {
        if current_level > TOTAL_LEVELS {
            break;
        }

        let line = line_result?;
        let parts: Vec<&str> = line.split('\t').collect();
        
        if parts.len() < 3 {
            continue; // Skip malformed lines
        }
        
        let lemma = parts[0];
        let rank: u32 = parts[1].parse()?;
        let occurrences: u64 = parts[2].parse()?;

        cumulative_occurrences += occurrences;
        
        // Use a while loop because a single high-frequency word might cross multiple thresholds
        loop {
            let target_coverage = (current_level as f64) * TARGET_PERCENTAGE_INCREASE;
            let target_occurrences = (total_occurrences as f64 * target_coverage) as u64;

            if cumulative_occurrences >= target_occurrences {
                let percentage = (cumulative_occurrences as f64 / total_occurrences as f64) * 100.0;
                println!(
                    "{:<5} | {:<20} | {:<15.2}% | {:<30}",
                    current_level, format_number(rank as u64), percentage, lemma
                );

                current_level += 1;
                if current_level > TOTAL_LEVELS {
                    break;
                }
            } else {
                // We haven't reached the next threshold yet, break the inner loop
                break;
            }
        }
    }

    println!("{:-<80}", "");
    println!("Analysis complete.");

    Ok(())
}

/// Reads the file once to sum all occurrences.
fn calculate_total_occurrences<P: AsRef<Path>>(path: P) -> io::Result<u64> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut total = 0;

    for line_result in reader.lines().skip(1) { // Skip header
        let line = line_result?;
        if let Some(occurrences_str) = line.split('\t').nth(2) {
            if let Ok(occurrences) = occurrences_str.parse::<u64>() {
                total += occurrences;
            }
        }
    }
    Ok(total)
}

/// Helper to format large numbers with commas for readability
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