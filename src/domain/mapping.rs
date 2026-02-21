// src/domain/mapping.rs

use crate::domain::primitives::WordId;
use serde::{Deserialize, Serialize};

/// Represents a specific link from a word in a source tier to a concept in a target tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MappingEntry {
    /// The stable identifier of the word in the source Tier.
    pub source_word_id: WordId,

    /// The text content this word maps to (e.g., the Spanish translation "gato").
    pub target_text: String,

    /// The lemmas associated with the target text (e.g., ["gato"]).
    pub target_lemmas: Vec<String>,

    /// Whether this mapping is "viable" (grammatically suitable for substitution).
    pub is_viable: bool,

    /// Whether this mapping represents a proper noun (which often bypasses learning checks).
    pub is_proper_noun: bool,
}

impl MappingEntry {
    pub fn new(source_id: WordId, target_text: String, lemmas: Vec<String>) -> Self {
        Self {
            source_word_id: source_id,
            target_text,
            target_lemmas: lemmas,
            is_viable: true, // Default to true, can be toggled
            is_proper_noun: false,
        }
    }
}

/// A collection of mappings between two specific Tiers (e.g., "basic_base" -> "basic_target").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierMapping {
    pub from_tier_id: String,
    pub to_tier_id: String,
    pub entries: Vec<MappingEntry>,
}

impl TierMapping {
    pub fn new(from_tier: String, to_tier: String) -> Self {
        Self {
            from_tier_id: from_tier,
            to_tier_id: to_tier,
            entries: Vec::new(),
        }
    }

    /// Adds a mapping entry.
    pub fn add_entry(&mut self, entry: MappingEntry) {
        // Logic could be added here to ensure no duplicate mappings for the same WordId
        // if strict 1:1 mapping is required, though currently 1:1 is the norm.
        self.entries.push(entry);
    }

    /// Removes mappings associated with a specific WordId.
    /// This is called by the Sentence aggregate when a word is deleted from a Tier.
    pub fn remove_entries_for_word(&mut self, id: WordId) {
        self.entries.retain(|e| e.source_word_id != id);
    }
}