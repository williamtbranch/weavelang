// In src/simulation/metrics.rs

use crate::simulation::frequency_manager;
use std::collections::HashMap;

/// A unified structure to hold all measurements for a generated text.
/// It is created from a list of all Spanish lemma instances and the total English word count.
#[derive(Debug, Clone)]
pub struct TextMetrics {
    pub total_word_count: u64,
    // A list of (rank, tally) tuples, sorted by rank.
    ranked_tallies: Vec<(u32, u32)>,
}

impl TextMetrics {
    /// Creates a new TextMetrics instance by processing the output of a generation run.
    pub fn new(lemma_instances: &[String], english_word_count: usize) -> Self {
        let mut lemma_tallies: HashMap<&String, u32> = HashMap::new();
        for lemma in lemma_instances {
            *lemma_tallies.entry(lemma).or_insert(0) += 1;
        }

        let mut ranked_tallies: Vec<(u32, u32)> = lemma_tallies
            .into_iter()
            .filter_map(|(lemma, tally)| {
                frequency_manager::get_rank_for_lemma(lemma).map(|rank| (rank, tally))
            })
            .collect();

        if english_word_count > 0 {
            ranked_tallies.push((0, english_word_count as u32));
        }

        let total_word_count = ranked_tallies.iter().map(|(_, tally)| *tally as u64).sum::<u64>();
        ranked_tallies.sort_unstable_by_key(|k| k.0);

        Self {
            total_word_count,
            ranked_tallies,
        }
    }

    /// Calculates the Tail-Weighted AVD Score.
    pub fn calculate_avd_score(&self) -> f64 {
        if self.total_word_count == 0 {
            return 0.0;
        }

        let p85_target_tally = (self.total_word_count as f64 * 0.85).ceil() as u64;
        let p95_target_tally = (self.total_word_count as f64 * 0.95).ceil() as u64;

        let mut cumulative_tally: u64 = 0;
        let mut p85_rank = 0.0;
        let mut p95_rank = 0.0;
        let mut p85_found = false;

        for (rank, tally) in &self.ranked_tallies {
            cumulative_tally += *tally as u64;

            if !p85_found && cumulative_tally >= p85_target_tally {
                p85_rank = *rank as f64;
                p85_found = true;
            }

            if cumulative_tally >= p95_target_tally {
                p95_rank = *rank as f64;
                break;
            }
        }
        (p85_rank + (2.0 * p95_rank)) / 3.0
    }

    /// Calculates the density of new lemmas compared to a previous vocabulary level.
    pub fn calculate_new_lemma_density(&self, previous_v_level: u32) -> f64 {
        if self.total_word_count == 0 {
            return 0.0;
        }

        let new_word_tally: u64 = self.ranked_tallies
            .iter()
            .filter(|(rank, _)| *rank > previous_v_level)
            .map(|(_, tally)| *tally as u64)
            .sum();

        new_word_tally as f64 / self.total_word_count as f64
    }
}