use rand::prelude::SliceRandom;
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct FrontierConfig {
    pub target_pct: f32,
    pub expected_unknown_pct: f32,
    pub total_tokens: usize,
    pub seed: u64,
}

#[derive(Debug, Clone)]
pub struct FrontierEngine {
    rng: StdRng,
    deck: Vec<bool>,
    cursor: usize,
    deck_size: usize,
    pass_count: usize,
    desired_unknown_pass_rate: f32,
    target_frontier_tokens: usize,
    emitted_frontier_tokens: usize,
    seen_unknown_tokens: usize,
    steering_adjustment_count: usize,
}

impl FrontierEngine {
    pub fn new(cfg: FrontierConfig) -> Self {
        let expected_unknown_pct = cfg.expected_unknown_pct.max(0.0);
        let desired_unknown_pass_rate = if expected_unknown_pct <= 0.0 {
            0.0
        } else {
            (cfg.target_pct / expected_unknown_pct).clamp(0.0, 1.0)
        };

        let expected_unknown_pct_int = expected_unknown_pct.round().max(1.0) as usize;
        let deck_size = (expected_unknown_pct_int.saturating_mul(3)).max(300);
        let pass_count = ((deck_size as f32) * desired_unknown_pass_rate).round() as usize;
        let target_frontier_tokens =
            (((cfg.total_tokens as f32) * (cfg.target_pct / 100.0)).round()).max(0.0) as usize;

        let mut engine = Self {
            rng: StdRng::seed_from_u64(cfg.seed),
            deck: vec![false; deck_size],
            cursor: 0,
            deck_size,
            pass_count: pass_count.min(deck_size),
            desired_unknown_pass_rate,
            target_frontier_tokens,
            emitted_frontier_tokens: 0,
            seen_unknown_tokens: 0,
            steering_adjustment_count: 0,
        };

        engine.reshuffle_deck();
        engine
    }

    fn reshuffle_deck(&mut self) {
        self.deck.fill(false);
        let mut indices: Vec<usize> = (0..self.deck_size).collect();
        indices.shuffle(&mut self.rng);
        for idx in indices.into_iter().take(self.pass_count) {
            self.deck[idx] = true;
        }
        self.cursor = 0;
    }

    fn next_deck_decision(&mut self) -> bool {
        if self.cursor >= self.deck.len() {
            self.reshuffle_deck();
        }
        let decision = self.deck[self.cursor];
        self.cursor += 1;
        decision
    }

    pub fn pick_sentence_frontier_lemmas(
        &mut self,
        unknown_lemma_token_weights: &HashMap<u32, usize>,
    ) -> HashSet<u32> {
        let mut passed = HashSet::new();
        if unknown_lemma_token_weights.is_empty() {
            return passed;
        }

        let mut ordered: Vec<(u32, usize)> = unknown_lemma_token_weights
            .iter()
            .map(|(lemma_id, weight)| (*lemma_id, (*weight).max(1)))
            .collect();
        ordered.sort_unstable_by_key(|(lemma_id, _)| *lemma_id);

        for (lemma_id, token_weight) in ordered {
            if self.should_pass_weight(token_weight) {
                passed.insert(lemma_id);
            }
        }

        passed
    }

    fn should_pass_weight(&mut self, token_weight: usize) -> bool {
        if token_weight == 0 {
            return false;
        }

        self.seen_unknown_tokens = self.seen_unknown_tokens.saturating_add(token_weight);

        let ideal_frontier_so_far =
            ((self.seen_unknown_tokens as f32) * self.desired_unknown_pass_rate).round() as usize;

        let mut pass = self.next_deck_decision();

        // Steering toward target ratio over token mass, while keeping randomness from deck.
        if pass {
            let projected = self.emitted_frontier_tokens.saturating_add(token_weight);
            if projected > ideal_frontier_so_far.saturating_add(token_weight)
                || projected > self.target_frontier_tokens.saturating_add(token_weight)
            {
                pass = false;
                self.steering_adjustment_count = self.steering_adjustment_count.saturating_add(1);
            }
        } else if self.emitted_frontier_tokens.saturating_add(token_weight) <= ideal_frontier_so_far
        {
            pass = true;
            self.steering_adjustment_count = self.steering_adjustment_count.saturating_add(1);
        }

        if pass {
            self.emitted_frontier_tokens = self.emitted_frontier_tokens.saturating_add(token_weight);
        }

        pass
    }

    pub fn emitted_frontier_tokens(&self) -> usize {
        self.emitted_frontier_tokens
    }

    pub fn target_frontier_tokens(&self) -> usize {
        self.target_frontier_tokens
    }

    pub fn steering_adjustment_count(&self) -> usize {
        self.steering_adjustment_count
    }

    pub fn deck_size(&self) -> usize {
        self.deck_size
    }

    pub fn pass_count(&self) -> usize {
        self.pass_count
    }
}
