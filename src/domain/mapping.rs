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

    /// Wlemma bucket keys for the target text. See
    /// `documentation/Wlemma_Migration_Plan.md`.
    #[serde(default)]
    pub target_wlemmas: Vec<String>,

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
            target_wlemmas: Vec::new(),
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

#[cfg(test)]
mod wlemma_serde_tests {
    //! TT4: serde round-trip + back-compat for the new `wlemmas` /
    //! `target_wlemmas` / `schema_version` fields. Old payloads (missing
    //! the fields) must still deserialize, with the wlemma fields
    //! defaulting to empty.
    use super::*;
    use crate::domain::primitives::{WordData, WordId};
    use crate::domain::segment::Segment;
    use crate::domain::tier::Tier;

    #[test]
    fn word_data_round_trip_preserves_wlemmas() {
        let mut w = WordData::new(WordId(1), "niños".into(), vec!["niño".into()]);
        w.wlemmas = vec!["niñ".into()];
        let s = serde_json::to_string(&w).unwrap();
        let w2: WordData = serde_json::from_str(&s).unwrap();
        assert_eq!(w, w2);
        assert_eq!(w2.wlemmas, vec!["niñ".to_string()]);
    }

    #[test]
    fn word_data_loads_legacy_payload_with_default_wlemmas() {
        // Pre-wlemma payload: no `wlemmas` field at all.
        let legacy = r#"{"id":7,"text":"niños","lemmas":["niño"]}"#;
        let w: WordData = serde_json::from_str(legacy).unwrap();
        assert_eq!(w.text, "niños");
        assert_eq!(w.lemmas, vec!["niño".to_string()]);
        assert!(w.wlemmas.is_empty(), "missing field defaults to empty");
    }

    #[test]
    fn segment_loads_legacy_payload() {
        // Construct a legacy Segment JSON (no `wlemmas` field). We can't
        // hand-write a valid TokenStream cheaply; instead serialize a real
        // Segment, strip the wlemmas field, and re-deserialize.
        let seg = Segment::new("S1".into(), "hola", vec!["hola".into()]);
        let mut v: serde_json::Value = serde_json::to_value(&seg).unwrap();
        v.as_object_mut().unwrap().remove("wlemmas");
        let seg2: Segment = serde_json::from_value(v).unwrap();
        assert_eq!(seg2.id, "S1");
        assert!(seg2.wlemmas.is_empty());
    }

    #[test]
    fn tier_loads_legacy_payload() {
        let mut tier = Tier::new("basic_target".into());
        tier.lemmas = vec!["el".into()];
        let mut v: serde_json::Value = serde_json::to_value(&tier).unwrap();
        v.as_object_mut().unwrap().remove("wlemmas");
        let tier2: Tier = serde_json::from_value(v).unwrap();
        assert_eq!(tier2.lemmas, vec!["el".to_string()]);
        assert!(tier2.wlemmas.is_empty());
    }

    #[test]
    fn mapping_entry_round_trip_with_wlemmas() {
        let mut m = MappingEntry::new(WordId(2), "gato".into(), vec!["gato".into()]);
        m.target_wlemmas = vec!["gat".into()];
        let s = serde_json::to_string(&m).unwrap();
        let m2: MappingEntry = serde_json::from_str(&s).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn mapping_entry_loads_legacy_payload() {
        let legacy = r#"{
            "source_word_id":3,
            "target_text":"gato",
            "target_lemmas":["gato"],
            "is_viable":true,
            "is_proper_noun":false
        }"#;
        let m: MappingEntry = serde_json::from_str(legacy).unwrap();
        assert_eq!(m.target_text, "gato");
        assert!(m.target_wlemmas.is_empty());
    }
}
