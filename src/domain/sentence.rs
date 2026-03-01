// src/domain/sentence.rs

use crate::domain::mapping::TierMapping;
use crate::domain::primitives::WordId;
use crate::domain::segment::Segment;
use crate::domain::tier::{Tier, TierState};
use crate::domain::token_stream::TokenStream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sentence {
    pub id: String,
    pub tiers: HashMap<String, Tier>,
    pub mappings: Vec<TierMapping>,
}

impl Sentence {
    pub fn new(id: String) -> Self {
        Self {
            id,
            tiers: HashMap::new(),
            mappings: Vec::new(),
        }
    }

    pub fn add_tier(&mut self, tier: Tier) {
        self.tiers.insert(tier.id.clone(), tier);
    }

    pub fn add_mapping(&mut self, mapping: TierMapping) {
        self.mappings.push(mapping);
    }

    pub fn get_tier(&self, tier_id: &str) -> Option<&Tier> {
        self.tiers.get(tier_id)
    }

    pub fn get_tier_mut(&mut self, tier_id: &str) -> Option<&mut Tier> {
        self.tiers.get_mut(tier_id)
    }

    pub fn mappings(&self) -> &[TierMapping] {
        &self.mappings
    }

    pub fn update_tier_text(&mut self, tier_id: &str, new_text: String) {
        // Default to Dirty for manual edits
        self.update_tier_text_internal(tier_id, new_text, TierState::Dirty)
    }

    pub fn update_tier_text_as_clean(&mut self, tier_id: &str, new_text: String) {
        // LLM updates are clean (Valid)
        self.update_tier_text_internal(tier_id, new_text, TierState::Valid)
    }

    /// Replace a tier's segments with pre-built segments (from tier_processor).
    /// This is the preferred path for LLM-generated text that has been properly
    /// segmented and tokenized via SpaCy.
    pub fn update_tier_with_segments(&mut self, tier_id: &str, segments: Vec<Segment>) {
        let tier = self
            .tiers
            .entry(tier_id.to_string())
            .or_insert_with(|| Tier::new(tier_id.to_string()));

        tier.state = TierState::Valid;
        tier.segments = segments;

        // Propagate staleness to dependents (same logic as update_tier_text_internal)
        self.propagate_stale(tier_id);
    }

    fn update_tier_text_internal(&mut self, tier_id: &str, new_text: String, new_state: TierState) {
        // 1. Update the target tier
        let tier = self
            .tiers
            .entry(tier_id.to_string())
            .or_insert_with(|| Tier::new(tier_id.to_string()));

        tier.state = new_state;

        tier.segments.clear();
        tier.segments.push(Segment::from_stream(
            "S1".to_string(),
            TokenStream::new(&new_text),
            vec![],
        ));

        // 2. Propagate staleness
        self.propagate_stale(tier_id);
    }

    /// Propagate "Stale" state to dependents based on the tier graph:
    /// Path A: Base -> Advanced -> Moderate -> Basic Target
    /// Path B: Base -> Basic Base
    fn propagate_stale(&mut self, tier_id: &str) {
        match tier_id {
            "base" => {
                self.mark_tier_stale("advanced_target");
                self.mark_tier_stale("basic_base");
            }
            "advanced_target" => {
                self.mark_tier_stale("moderate_target");
            }
            "moderate_target" => {
                self.mark_tier_stale("basic_target");
            }
            _ => {}
        }
    }

    fn mark_tier_stale(&mut self, tier_id: &str) {
        if let Some(tier) = self.tiers.get_mut(tier_id) {
            // Only mark as Stale if it was clean (Valid)
            if tier.state == TierState::Valid {
                tier.state = TierState::Stale;

                // Recurse: if this tier becomes stale, its children also become stale
                match tier_id {
                    "advanced_target" => self.mark_tier_stale("moderate_target"),
                    "moderate_target" => self.mark_tier_stale("basic_target"),
                    _ => {}
                }
            }
        }
    }

    pub fn modify_word_text(
        &mut self,
        tier_id: &str,
        word_id: WordId,
        new_text: String,
    ) -> Result<(), String> {
        let tier = self
            .tiers
            .get_mut(tier_id)
            .ok_or_else(|| format!("Tier '{tier_id}' not found."))?;
        for segment in &mut tier.segments {
            if segment
                .stream
                .modify_word_text(word_id, new_text.clone())
                .is_ok()
            {
                return Ok(());
            }
        }
        Err(format!("WordId {word_id:?} not found in any segment."))
    }

    pub fn delete_word(&mut self, tier_id: &str, word_id: WordId) -> Result<(), String> {
        let tier = self
            .tiers
            .get_mut(tier_id)
            .ok_or_else(|| format!("Tier '{tier_id}' not found."))?;
        let mut found = false;
        for segment in &mut tier.segments {
            if segment.stream.delete_word(word_id).is_ok() {
                found = true;
                break;
            }
        }
        if !found {
            return Err(format!("WordId {word_id:?} not found."));
        }
        for mapping in &mut self.mappings {
            if mapping.from_tier_id == tier_id {
                mapping.remove_entries_for_word(word_id);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::token_stream::TokenStream;

    #[test]
    fn test_sequential_ids_generation() {
        let mut sentence = Sentence::new("S1".into());
        sentence.update_tier_text("base", "A b c".into());

        let tier = sentence.get_tier("base").unwrap();
        let segment = &tier.segments[0];

        let ids: Vec<u64> = segment
            .stream
            .tokens()
            .iter()
            .filter_map(|t| match t {
                crate::domain::token_stream::Token::Word(w) => Some(w.id.0),
                _ => None,
            })
            .collect();

        assert_eq!(ids, vec![0, 1, 2], "IDs must be sequential starting at 0");
    }
}
