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
    /// This now includes a normalization step to cap high-frequency outliers.
    pub fn new(lemma_instances: &[String], english_word_count: usize) -> Self {
        let mut lemma_tallies: HashMap<&String, u32> = HashMap::new();
        for lemma in lemma_instances {
            *lemma_tallies.entry(lemma).or_insert(0) += 1;
        }

        // --- START OF "GREGOR EFFECT" FIX ---
        let total_word_count_float = (lemma_instances.len() + english_word_count) as f64;
        // Cap is 0.2% of the total word count.
        let tally_cap = (total_word_count_float * 0.002).ceil().max(1.0) as u32;

        let mut capped_tallies = HashMap::new();
        for (lemma, tally) in lemma_tallies {
            // Treat unranked words (like proper nouns) as having a very high rank.
            let rank = frequency_manager::rank_of_lemma_string(lemma).unwrap_or(u32::MAX);

            // Apply the cap if the word is rare (rank > 400) and appears too frequently.
            if rank > 400 && tally > tally_cap {
                capped_tallies.insert(lemma.clone(), tally_cap);
            } else {
                capped_tallies.insert(lemma.clone(), tally);
            }
        }
        // --- END OF "GREGOR EFFECT" FIX ---

        let mut ranked_tallies: Vec<(u32, u32)> = capped_tallies
            .into_iter()
            .filter_map(|(lemma, tally)| {
                // Use the capped tally for the final structure.
                frequency_manager::rank_of_lemma_string(&lemma).map(|rank| (rank, tally))
            })
            .collect();

        if english_word_count > 0 {
            ranked_tallies.push((0, english_word_count as u32));
        }

        let total_word_count = ranked_tallies
            .iter()
            .map(|(_, tally)| *tally as u64)
            .sum::<u64>();
        ranked_tallies.sort_unstable_by_key(|k| k.0);

        Self {
            total_word_count,
            ranked_tallies,
        }
    }

    /// Creates a new TextMetrics instance WITHOUT the 0.2% tally cap.
    /// Used for corrected AVD scoring that doesn't flatten the tail.
    pub fn new_v2(lemma_instances: &[String], english_word_count: usize) -> Self {
        let mut lemma_tallies: HashMap<&String, u32> = HashMap::new();
        for lemma in lemma_instances {
            *lemma_tallies.entry(lemma).or_insert(0) += 1;
        }

        // NO CAP — just count naturally.
        let mut ranked_tallies: Vec<(u32, u32)> = lemma_tallies
            .into_iter()
            .filter_map(|(lemma, tally)| {
                // Treat unranked words as having a very high rank.
                frequency_manager::rank_of_lemma_string(lemma).map(|rank| (rank, tally))
            })
            .collect();

        if english_word_count > 0 {
            ranked_tallies.push((0, english_word_count as u32));
        }

        let total_word_count = ranked_tallies
            .iter()
            .map(|(_, tally)| *tally as u64)
            .sum::<u64>();
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

    /// Returns the raw percentile rank components used by AVD.
    pub fn calculate_avd_components(&self) -> (f64, f64) {
        if self.total_word_count == 0 {
            return (0.0, 0.0);
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

        (p85_rank, p95_rank)
    }

    /// Percentage of token instances with rank >= min_rank.
    pub fn tail_share_pct(&self, min_rank: u32) -> f64 {
        if self.total_word_count == 0 {
            return 0.0;
        }

        let tail_tally: u64 = self
            .ranked_tallies
            .iter()
            .filter(|(rank, _)| *rank >= min_rank)
            .map(|(_, tally)| *tally as u64)
            .sum();

        (tail_tally as f64 / self.total_word_count as f64) * 100.0
    }

    /// Lonsdale-style vocabulary coverage: percentage of *target-language*
    /// (non-anchor) token instances whose lemma rank is within the first
    /// `max_rank` lemmas of the frequency list.  Rank=0 English anchor tokens
    /// are excluded from both numerator and denominator so the result reflects
    /// pure Spanish lexical coverage.
    pub fn head_share_pct(&self, max_rank: u32) -> f64 {
        let target_total: u64 = self
            .ranked_tallies
            .iter()
            .filter(|(rank, _)| *rank > 0)
            .map(|(_, tally)| *tally as u64)
            .sum();

        if target_total == 0 {
            return 0.0;
        }

        let head_tally: u64 = self
            .ranked_tallies
            .iter()
            .filter(|(rank, _)| *rank > 0 && *rank <= max_rank)
            .map(|(_, tally)| *tally as u64)
            .sum();

        (head_tally as f64 / target_total as f64) * 100.0
    }

    /// Weighted average of log10(rank), ignoring rank=0 anchor tokens.
    pub fn weighted_log_rank_mean(&self) -> f64 {
        let mut weighted_sum = 0.0;
        let mut tally_sum: u64 = 0;

        for (rank, tally) in &self.ranked_tallies {
            if *rank == 0 {
                continue;
            }
            weighted_sum += (*tally as f64) * (*rank as f64).log10();
            tally_sum += *tally as u64;
        }

        if tally_sum == 0 {
            0.0
        } else {
            weighted_sum / tally_sum as f64
        }
    }

    /// Average rank within a percentile band [start_pct, end_pct].
    ///
    /// Example: 99-100 band means the top 1% of token instances by cumulative
    /// rank position in this text.
    pub fn average_rank_in_percentile_band(&self, start_pct: f64, end_pct: f64) -> f64 {
        if self.total_word_count == 0 {
            return 0.0;
        }

        let total = self.total_word_count;
        let start_pos = ((total as f64) * (start_pct / 100.0)).floor() as u64;
        let mut end_pos = ((total as f64) * (end_pct / 100.0)).floor() as u64;

        if end_pct >= 100.0 {
            end_pos = total;
        }
        if end_pos <= start_pos {
            end_pos = (start_pos + 1).min(total);
        }

        let mut cumulative_before: u64 = 0;
        let mut rank_sum: f64 = 0.0;
        let mut token_count: u64 = 0;

        for (rank, tally) in &self.ranked_tallies {
            let cumulative_after = cumulative_before + (*tally as u64);

            // This rank bucket covers token positions (cumulative_before, cumulative_after].
            // Band covers token positions (start_pos, end_pos].
            let overlap_start = cumulative_before.max(start_pos);
            let overlap_end = cumulative_after.min(end_pos);

            if overlap_end > overlap_start {
                let overlap = overlap_end - overlap_start;
                rank_sum += (overlap as f64) * (*rank as f64);
                token_count += overlap;
            }

            cumulative_before = cumulative_after;
            if cumulative_before >= end_pos {
                break;
            }
        }

        if token_count == 0 {
            0.0
        } else {
            rank_sum / token_count as f64
        }
    }

    /// Calculates the density of new lemmas compared to a previous vocabulary level.
    pub fn calculate_new_lemma_density(&self, previous_v_level: u32) -> f64 {
        if self.total_word_count == 0 {
            return 0.0;
        }

        let new_word_tally: u64 = self
            .ranked_tallies
            .iter()
            .filter(|(rank, _)| *rank > previous_v_level)
            .map(|(_, tally)| *tally as u64)
            .sum();

        new_word_tally as f64 / self.total_word_count as f64
    }
}
