// src/domain/sentence.rs

use crate::domain::mapping::TierMapping;
use crate::domain::primitives::WordId;
use crate::domain::tier::{Tier, TierState};
use crate::domain::segment::Segment;
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

    // --- UPDATED: Handles Creation of Missing Tiers ---
    pub fn update_tier_text(&mut self, tier_id: &str, new_text: String) {
        // 1. Get or Create the target tier
        let tier = self.tiers.entry(tier_id.to_string())
            .or_insert_with(|| Tier::new(tier_id.to_string()));

        tier.state = TierState::Dirty;
        
        // Collapse to single segment to preserve text
        // (TokenStream::new handles basic regex splitting so it's not totally raw)
        tier.segments.clear();
        tier.segments.push(Segment::from_stream(
            "S1".to_string(),
            TokenStream::new(&new_text), 
            vec![]
        ));

        // 2. Propagate "Stale" state to dependents
        let next_tier_id = match tier_id {
            "base" => None, // Base is root, but changes here ripple to Adv and BasBase manually via UI
            "advanced_target" => Some("moderate_target"),
            "moderate_target" => Some("basic_target"),
            "basic_target" => Some("basic_base"),
            _ => None,
        };

        if let Some(dependent_id) = next_tier_id {
            self.mark_tier_stale(dependent_id);
        }
        
        // Note: For Base -> Advanced and Base -> BasicBase, the UI handles 
        // triggering the generation. We could link them here, but explicit 
        // regeneration in the UI is safer for the "Source of Truth".
    }

    fn mark_tier_stale(&mut self, tier_id: &str) {
        if let Some(tier) = self.tiers.get_mut(tier_id) {
            if tier.state == TierState::Valid {
                tier.state = TierState::Stale;
                
                let next_id = match tier_id {
                    "advanced_target" => Some("moderate_target"),
                    "moderate_target" => Some("basic_target"),
                    "basic_target" => Some("basic_base"),
                    _ => None,
                };
                if let Some(n) = next_id {
                    self.mark_tier_stale(n);
                }
            }
        }
    }

    pub fn modify_word_text(&mut self, tier_id: &str, word_id: WordId, new_text: String) -> Result<(), String> {
        let tier = self.tiers.get_mut(tier_id).ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
        for segment in &mut tier.segments {
            if segment.stream.modify_word_text(word_id, new_text.clone()).is_ok() { return Ok(()); }
        }
        Err(format!("WordId {:?} not found in any segment.", word_id))
    }

    pub fn delete_word(&mut self, tier_id: &str, word_id: WordId) -> Result<(), String> {
        let tier = self.tiers.get_mut(tier_id).ok_or_else(|| format!("Tier '{}' not found.", tier_id))?;
        let mut found = false;
        for segment in &mut tier.segments {
            if segment.stream.delete_word(word_id).is_ok() { found = true; break; }
        }
        if !found { return Err(format!("WordId {:?} not found.", word_id)); }
        for mapping in &mut self.mappings {
            if mapping.from_tier_id == tier_id { mapping.remove_entries_for_word(word_id); }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::token_stream::TokenStream;
    use crate::domain::segment::Segment;

    #[test]
    fn test_sequential_ids_generation() {
        // When we create a tier via update_tier_text, IDs should be auto-generated.
        // NOTE: Currently, Sentence::update_tier_text delegates to TokenStream::new,
        // which starts ID counter at 0.
        
        let mut sentence = Sentence::new("S1".into());
        sentence.update_tier_text("base", "A b c".into());

        let tier = sentence.get_tier("base").unwrap();
        let segment = &tier.segments[0];
        
        // Extract IDs
        let ids: Vec<u64> = segment.stream.tokens().iter()
            .filter_map(|t| match t {
                crate::domain::token_stream::Token::Word(w) => Some(w.id.0),
                _ => None
            })
            .collect();

        assert_eq!(ids, vec![0, 1, 2], "IDs must be sequential starting at 0");
    }
}