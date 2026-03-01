// In your Cargo.toml, add this line under [dependencies]:
// rand = "0.8"

use rand::distributions::WeightedIndex;
use rand::prelude::*;
use std::collections::{HashMap, HashSet};

// --- SIMULATION PARAMETERS ---
const VOCAB_SIZE: usize = 60_222;
const EXPOSURE_THRESHOLD: u32 = 20;
const ZIPF_ALPHA: f64 = 1.0;
const NUM_SIMULATION_RUNS: usize = 5;
const WORD_PROCESSING_BATCH_SIZE: u64 = 10_000;
const WORDS_PER_HOUR: u64 = 9000;

fn generate_zipf_frequencies(num_lemmas: usize, alpha: f64) -> Vec<f64> {
    let mut freqs: Vec<f64> = (1..=num_lemmas)
        .map(|i| 1.0 / (i as f64).powf(alpha))
        .collect();
    let sum: f64 = freqs.iter().sum();
    for f in &mut freqs {
        *f /= sum;
    }
    freqs
}
mod production {
    use super::*;
    #[derive(Debug, Clone)]
    pub struct Checkpoint {
        pub ka_vocab_percent: f64,
        pub known_words: usize,
        pub active_words: usize,
        pub hours_elapsed: f64,
        pub learning_rate: f64,
        pub measured_comprehension: f64,
    }
    // MODIFIED: Removed the unused `frequencies` field from the struct.
    pub struct ProductionSim {
        rate_curve: Vec<(f64, u64)>,
        global_dist: WeightedIndex<f64>,
        rng: ThreadRng,
        active_lemmas: HashMap<u32, u32>,
        known_lemmas: HashSet<u32>,
        words_processed: u64,
        words_processed_since_last_intro: u64,
        next_lemma_to_introduce_idx: u32,
    }
    impl ProductionSim {
        // MODIFIED: Updated the constructor to match the struct change.
        pub fn new(rate_curve: Vec<(f64, u64)>, frequencies: Vec<f64>) -> Self {
            let global_dist = WeightedIndex::new(&frequencies).unwrap();
            let mut active_lemmas = HashMap::new();
            active_lemmas.insert(0, 0);
            Self {
                rate_curve,
                global_dist,
                rng: rand::thread_rng(),
                active_lemmas,
                known_lemmas: HashSet::new(),
                words_processed: 0,
                words_processed_since_last_intro: 0,
                next_lemma_to_introduce_idx: 1,
            }
        }

        fn get_rate_from_curve(&self, current_percent: f64) -> u64 {
            if self.rate_curve.is_empty() {
                return 900;
            }
            for window in self.rate_curve.windows(2) {
                let (p1, r1) = window[0];
                let (p2, r2) = window[1];
                if current_percent >= p1 && current_percent < p2 {
                    let factor = (current_percent - p1) / (p2 - p1);
                    return r1 + (factor * (r2 as f64 - r1 as f64)) as u64;
                }
            }
            self.rate_curve.last().map_or(900, |(_, r)| *r)
        }

        pub fn run(&mut self) -> Vec<Checkpoint> {
            let mut checkpoints = Vec::new();
            let mut next_checkpoint_percent = 5.0;

            loop {
                let current_intro_percent =
                    (self.next_lemma_to_introduce_idx as f64 / VOCAB_SIZE as f64) * 100.0;
                let target_rate = self.get_rate_from_curve(current_intro_percent);
                while self.words_processed_since_last_intro >= target_rate
                    && self.next_lemma_to_introduce_idx < VOCAB_SIZE as u32
                {
                    self.active_lemmas
                        .insert(self.next_lemma_to_introduce_idx, 0);
                    self.next_lemma_to_introduce_idx += 1;
                    self.words_processed_since_last_intro -= target_rate;
                }

                // --- BATCH PROCESSING (The Core Fix) ---
                // Cache the active_lemmas vector for this batch to avoid rebuilding it
                let active_ids_vec: Vec<u32> = self.active_lemmas.keys().copied().collect();

                for _ in 0..WORD_PROCESSING_BATCH_SIZE {
                    let original_word_id = self.global_dist.sample(&mut self.rng) as u32;

                    let seen_word_id = if self.known_lemmas.contains(&original_word_id)
                        || self.active_lemmas.contains_key(&original_word_id)
                    {
                        original_word_id // The word was in K+A, see it directly.
                    } else {
                        // --- "FAIR PRACTICE" SUBSTITUTION ---
                        // It's an unknown word. Substitute it with a RANDOMLY chosen ACTIVE word.
                        if !active_ids_vec.is_empty() {
                            // Uniformly random choice from the active set.
                            *active_ids_vec.choose(&mut self.rng).unwrap()
                        } else {
                            continue; // No active words to substitute with, so nothing is seen.
                        }
                    };

                    if let Some(count) = self.active_lemmas.get_mut(&seen_word_id) {
                        *count += 1;
                    }
                }
                self.words_processed += WORD_PROCESSING_BATCH_SIZE;
                self.words_processed_since_last_intro += WORD_PROCESSING_BATCH_SIZE;

                let graduated: Vec<u32> = self
                    .active_lemmas
                    .iter()
                    .filter(|(_, &c)| c >= EXPOSURE_THRESHOLD)
                    .map(|(&id, _)| id)
                    .collect();
                if !graduated.is_empty() {
                    for id in graduated {
                        self.active_lemmas.remove(&id);
                        self.known_lemmas.insert(id);
                    }
                }

                let ka_vocab_percent =
                    (self.next_lemma_to_introduce_idx as f64 / VOCAB_SIZE as f64) * 100.0;
                if ka_vocab_percent >= next_checkpoint_percent {
                    let hours = self.words_processed as f64 / WORDS_PER_HOUR as f64;
                    let rate = if hours > 0.0 {
                        self.known_lemmas.len() as f64 / hours
                    } else {
                        0.0
                    };
                    let known_count = self.known_lemmas.len();
                    let active_count = self.active_lemmas.len();
                    let total_vocab = known_count + active_count;
                    let comprehension = if total_vocab > 0 {
                        known_count as f64 / total_vocab as f64
                    } else {
                        1.0
                    };
                    checkpoints.push(Checkpoint {
                        ka_vocab_percent,
                        known_words: known_count,
                        active_words: active_count,
                        hours_elapsed: hours,
                        learning_rate: rate,
                        measured_comprehension: comprehension * 100.0,
                    });
                    next_checkpoint_percent += 5.0;
                }
                if self.next_lemma_to_introduce_idx >= VOCAB_SIZE as u32 {
                    break;
                }
            }
            checkpoints
        }
    }
}

fn main() {
    println!("--- WeaveLang Final Simulation (v10 - Fair Practice Model) ---\n");
    println!(
        "Simulating learner journey with a fixed introduction curve and fair substitution model."
    );

    let rate_curve = vec![
        (0.0, 3000),
        (5.0, 550),
        (10.0, 590),
        (15.0, 620),
        (20.0, 640),
        (25.0, 660),
        (30.0, 670),
        (35.0, 680),
        (40.0, 690),
        (45.0, 700),
        (50.0, 700),
        (55.0, 715),
        (60.0, 720),
        (65.0, 720),
        (70.0, 730),
        (75.0, 730),
        (80.0, 740),
        (85.0, 740),
        (90.0, 745),
        (95.0, 750),
        (100.0, 755),
    ];

    let mut prod_checkpoints: Vec<Vec<production::Checkpoint>> = Vec::new();
    let frequencies = generate_zipf_frequencies(VOCAB_SIZE, ZIPF_ALPHA);
    for _ in 0..NUM_SIMULATION_RUNS {
        let mut sim = production::ProductionSim::new(rate_curve.clone(), frequencies.clone());
        prod_checkpoints.push(sim.run());
        print!(".");
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
    }
    println!(" Done.");

    println!("\n--- The Learner's Journey (Final Model Results) ---");
    println!(
        "{:<8} | {:<8} | {:<8} | {:<8} | {:<9} | {:<12} | {:<12}",
        "K+A (%)", "Known", "Active", "Total", "Hours", "Learn Rate", "Comp. (%)"
    );
    println!("{}", "-".repeat(80));
    if let Some(first_run) = prod_checkpoints.first() {
        for i in 0..first_run.len() {
            let percent = first_run[i].ka_vocab_percent;
            let num_runs = prod_checkpoints
                .iter()
                .filter(|r| r.get(i).is_some())
                .count();
            if num_runs > 0 {
                let avg_known = prod_checkpoints
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|c| c.known_words)
                    .sum::<usize>()
                    / num_runs;
                let avg_active = prod_checkpoints
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|c| c.active_words)
                    .sum::<usize>()
                    / num_runs;
                let avg_hours = prod_checkpoints
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|c| c.hours_elapsed)
                    .sum::<f64>()
                    / num_runs as f64;
                let avg_rate = prod_checkpoints
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|c| c.learning_rate)
                    .sum::<f64>()
                    / num_runs as f64;
                let avg_comp = prod_checkpoints
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|c| c.measured_comprehension)
                    .sum::<f64>()
                    / num_runs as f64;
                println!(
                    "{:<7.1}% | {:<8} | {:<8} | {:<8} | {:<8.0} | {:<11.2} w/h | {:<11.2} %",
                    percent,
                    avg_known,
                    avg_active,
                    avg_known + avg_active,
                    avg_hours,
                    avg_rate,
                    avg_comp
                );
            }
        }
    }
}
