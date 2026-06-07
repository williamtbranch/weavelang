// src/domain/sentence.rs

use crate::domain::mapping::TierMapping;
use crate::domain::normalization::normalize_spanish_lemma;
use crate::domain::primitives::WordId;
use crate::domain::segment::Segment;
use crate::domain::tier::{Tier, TierState};
use crate::domain::token_stream::{Token, TokenStream};
use crate::services::python_bridge::BridgeService;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Completeness status for a sentence (or a single tier within it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completeness {
    /// All required data present and valid.
    Complete,
    /// Some data present but not everything needed for weave.
    Incomplete,
    /// No data present (tier missing or empty).
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sentence {
    pub id: String,
    #[serde(default)]
    pub tiers: HashMap<String, Tier>,
    #[serde(default)]
    pub mappings: Vec<TierMapping>,
    /// Lemma strings that the forward diglot map flagged as proper nouns
    /// (via `{{…}}` braces).  These are persisted per-sentence so that
    /// the weave algorithm can treat them as always-known, even after
    /// tier re-generation reintroduces the lemmas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proper_noun_lemmas: Vec<String>,
    /// In-memory only: when true, this sentence belongs to a project where
    /// source language == target language (e.g. Spanish-source Spanish-target).
    /// The basic-branch dependency direction reverses in that mode:
    ///   English-source: basic_base → basic_target
    ///   Spanish-source: basic_target → basic_base
    /// Engine/loader is responsible for setting this after import or load.
    #[serde(skip)]
    pub source_is_target: bool,
    /// In-memory only: when true, the project is in *simple mode*. Weave
    /// completeness only requires the basic branch (`base`, `basic_base`,
    /// `basic_target`) plus both diglot mappings — `advanced_target` and
    /// `moderate_target` are never produced and must not block weave-ready.
    #[serde(skip)]
    pub simple_mode: bool,
    /// In-memory only: when true, the project is in *simple-triple* output
    /// mode. Only `base` + `basic_target` are required; `basic_base` is OFF
    /// (no forward diglot mapping). Weave-readiness needs the inverse diglot
    /// mapping (for the diglot output). See `Simple_Triple_Mode_Plan.md`.
    #[serde(skip)]
    pub simple_triple: bool,
}

impl Sentence {
    pub fn new(id: String) -> Self {
        Self {
            id,
            tiers: HashMap::new(),
            mappings: Vec::new(),
            proper_noun_lemmas: Vec::new(),
            source_is_target: false,
            simple_mode: false,
            simple_triple: false,
        }
    }

    /// Set the in-memory `source_is_target` flag. Engine calls this after
    /// import/load to propagate the project-level mode into each sentence
    /// so dependency-aware staleness propagation works correctly.
    pub fn set_source_is_target(&mut self, v: bool) {
        self.source_is_target = v;
    }

    /// Set the in-memory `simple_mode` flag. Engine calls this after
    /// import/load and on flag toggles so weave-completeness skips the
    /// unused advanced/moderate tiers.
    pub fn set_simple_mode(&mut self, v: bool) {
        self.simple_mode = v;
    }

    /// Set the in-memory `simple_triple` flag. Engine calls this after
    /// import/load and on flag toggles so weave-completeness drops the
    /// `basic_base` tier and its forward mapping from the requirements.
    pub fn set_simple_triple(&mut self, v: bool) {
        self.simple_triple = v;
    }

    pub fn add_tier(&mut self, tier: Tier) {
        self.tiers.insert(tier.id.clone(), tier);
    }

    pub fn add_mapping(&mut self, mapping: TierMapping) {
        // When a forward diglot mapping (basic_base → basic_target) is added,
        // extract proper-noun lemmas so the weave algorithm can ignore them.
        let is_forward_diglot =
            mapping.from_tier_id == "basic_base" && mapping.to_tier_id == "basic_target";

        // Replace any existing mapping with the same direction to prevent
        // stale data.  Display and coverage-check must always see the same
        // (most-recent) mapping.
        self.mappings.retain(|m| {
            !(m.from_tier_id == mapping.from_tier_id && m.to_tier_id == mapping.to_tier_id)
        });

        self.mappings.push(mapping);

        if is_forward_diglot {
            self.rebuild_proper_noun_lemmas();
        }
    }

    /// Rebuild the `proper_noun_lemmas` list from the current forward diglot
    /// mapping entries.  Called automatically when a forward diglot mapping
    /// is added, or can be invoked manually after editing mapping entries.
    pub fn rebuild_proper_noun_lemmas(&mut self) {
        let mut pn_lemmas: Vec<String> = Vec::new();

        for mapping in &self.mappings {
            if mapping.from_tier_id == "basic_base" && mapping.to_tier_id == "basic_target" {
                for entry in &mapping.entries {
                    if entry.is_proper_noun {
                        // Use the target lemmas if available, otherwise
                        // fall back to the lowercased target text.
                        if entry.target_lemmas.is_empty() {
                            let lemma = entry.target_text.to_lowercase();
                            if !pn_lemmas.contains(&lemma) {
                                pn_lemmas.push(lemma);
                            }
                        } else {
                            for l in &entry.target_lemmas {
                                let lc = l.to_lowercase();
                                if !pn_lemmas.contains(&lc) {
                                    pn_lemmas.push(lc);
                                }
                            }
                        }
                    }
                }
            }
        }

        self.proper_noun_lemmas = pn_lemmas;
    }

    /// Populate the `target_lemmas` field on all mapping entries, mirroring
    /// the Python pipeline's `FinalizeMappings` stage.
    ///
    /// **Forward diglot** (basic_base → basic_target): each entry's
    /// `target_text` (the Spanish phrase) is sent to SpaCy for
    /// lemmatization.  The resulting lemmas are normalized with
    /// `normalize_spanish_lemma` and stored.
    ///
    /// **Inverse diglot** (basic_target → basic_base): each entry's
    /// lemmas are looked up from the `basic_target` tier's token stream
    /// by matching on `source_word_id`.
    pub fn finalize_mapping_lemmas(
        &mut self,
        bridge: Option<&BridgeService>,
        target_lang_code: &str,
    ) {
        // --- 1. Forward diglot: lemmatize target_text via SpaCy ---
        for mapping in &mut self.mappings {
            if mapping.from_tier_id == "basic_base" && mapping.to_tier_id == "basic_target" {
                for entry in &mut mapping.entries {
                    if !entry.is_viable || entry.target_text.is_empty() {
                        entry.target_lemmas = Vec::new();
                        continue;
                    }
                    if let Some(br) = bridge {
                        match br.tokenize(&entry.target_text, target_lang_code) {
                            Ok(raw_tokens) => {
                                let mut lemmas: Vec<String> = raw_tokens
                                    .iter()
                                    .filter(|t| !t.is_punct && !t.is_space)
                                    .filter_map(|t| {
                                        let norm = normalize_spanish_lemma(&t.lemma);
                                        if norm.is_empty() { None } else { Some(norm) }
                                    })
                                    .collect();
                                lemmas.sort();
                                entry.target_lemmas = lemmas;
                            }
                            Err(e) => {
                                eprintln!(
                                    "[FinalizeMappings] SpaCy lemmatization failed for {:?}: {}",
                                    entry.target_text, e
                                );
                                // Fallback: normalized lowercase of target_text
                                let fallback = normalize_spanish_lemma(&entry.target_text);
                                entry.target_lemmas = if fallback.is_empty() {
                                    Vec::new()
                                } else {
                                    vec![fallback]
                                };
                            }
                        }
                    } else {
                        // No bridge available — best-effort fallback
                        let fallback = normalize_spanish_lemma(&entry.target_text);
                        entry.target_lemmas = if fallback.is_empty() {
                            Vec::new()
                        } else {
                            vec![fallback]
                        };
                    }
                }
            }
        }

        // --- 2. Inverse diglot: pull lemmas from basic_target token stream ---
        // Pre-build a WordId → lemmas lookup from the basic_target tier.
        let target_lemma_by_id: HashMap<WordId, Vec<String>> = self
            .get_tier("basic_target")
            .map(|tier| {
                let mut map = HashMap::new();
                for seg in &tier.segments {
                    for token in seg.stream.tokens() {
                        if let Token::Word(wd) = token {
                            map.insert(wd.id, wd.lemmas.clone());
                        }
                    }
                }
                map
            })
            .unwrap_or_default();

        for mapping in &mut self.mappings {
            if mapping.from_tier_id == "basic_target" && mapping.to_tier_id == "basic_base" {
                for entry in &mut mapping.entries {
                    entry.target_lemmas = target_lemma_by_id
                        .get(&entry.source_word_id)
                        .cloned()
                        .unwrap_or_default();
                }
            }
        }

        // Proper-noun lemmas may have changed — rebuild.
        self.rebuild_proper_noun_lemmas();
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

    /// Public entry point for propagating staleness from a given tier.
    pub fn propagate_stale_from(&mut self, tier_id: &str) {
        self.propagate_stale(tier_id);
    }

    /// Propagate "Stale" state to dependents based on the tier graph.
    ///
    /// English-source (default):
    ///   base → advanced_target → moderate_target
    ///   base → basic_base       → basic_target
    ///
    /// Spanish-source (`source_is_target == true`):
    ///   base → advanced_target → moderate_target
    ///   base → basic_target    → basic_base
    fn propagate_stale(&mut self, tier_id: &str) {
        if self.source_is_target {
            match tier_id {
                "base" => {
                    self.mark_tier_stale("advanced_target");
                    self.mark_tier_stale("basic_target");
                }
                "advanced_target" => {
                    self.mark_tier_stale("moderate_target");
                }
                "basic_target" => {
                    self.mark_tier_stale("basic_base");
                }
                _ => {}
            }
        } else {
            match tier_id {
                "base" => {
                    self.mark_tier_stale("advanced_target");
                    self.mark_tier_stale("basic_base");
                }
                "advanced_target" => {
                    self.mark_tier_stale("moderate_target");
                }
                "basic_base" => {
                    self.mark_tier_stale("basic_target");
                }
                _ => {}
            }
        }
    }

    fn mark_tier_stale(&mut self, tier_id: &str) {
        if let Some(tier) = self.tiers.get_mut(tier_id) {
            // Only mark as Stale if it was clean (Valid or Pending)
            if tier.state == TierState::Valid || tier.state == TierState::Pending {
                tier.state = TierState::Stale;

                // Recurse: if this tier becomes stale, its children also become stale
                if self.source_is_target {
                    match tier_id {
                        "advanced_target" => self.mark_tier_stale("moderate_target"),
                        "basic_target" => self.mark_tier_stale("basic_base"),
                        _ => {}
                    }
                } else {
                    match tier_id {
                        "advanced_target" => self.mark_tier_stale("moderate_target"),
                        "basic_base" => self.mark_tier_stale("basic_target"),
                        _ => {}
                    }
                }
            }
        }
    }

    // ----------------------------------------------------------------
    // Completeness / Weave-readiness queries
    // ----------------------------------------------------------------

    /// The five content tiers required for a complete weave sentence.
    pub const WEAVE_TIERS: &'static [&'static str] = &[
        "base",
        "advanced_target",
        "moderate_target",
        "basic_target",
        "basic_base",
    ];

    /// Check completeness of a single tier.
    pub fn tier_completeness(&self, tier_id: &str) -> Completeness {
        match self.tiers.get(tier_id) {
            None => Completeness::Empty,
            Some(tier) => {
                let text = tier.full_text();
                if text.trim().is_empty() {
                    Completeness::Empty
                } else if tier.state == TierState::Valid {
                    Completeness::Complete
                } else {
                    // Dirty, Stale, or Broken — data exists but isn't clean
                    Completeness::Incomplete
                }
            }
        }
    }

    /// Check whether the forward diglot mapping (basic_base → basic_target) exists.
    pub fn has_diglot_mapping(&self) -> bool {
        self.mappings.iter().any(|m| {
            m.from_tier_id == "basic_base"
                && m.to_tier_id == "basic_target"
                && !m.entries.is_empty()
        })
    }

    /// Check whether the inverse diglot mapping (basic_target → basic_base) exists.
    pub fn has_inverse_diglot_mapping(&self) -> bool {
        self.mappings.iter().any(|m| {
            m.from_tier_id == "basic_target"
                && m.to_tier_id == "basic_base"
                && !m.entries.is_empty()
        })
    }

    /// Check whether every Word in the given tier has a corresponding
    /// MappingEntry in a mapping where `from_tier_id == tier_id`.
    /// Returns `true` if every word is covered (translation or NO_SUB),
    /// `false` if any word lacks a mapping entry.
    pub fn check_mapping_coverage(&self, from_tier_id: &str) -> bool {
        // Collect all WordIds from the tier's token streams
        let tier = match self.get_tier(from_tier_id) {
            Some(t) => t,
            None => return false,
        };
        let word_ids: Vec<WordId> = tier.segments.iter()
            .flat_map(|seg| seg.stream.tokens().iter())
            .filter_map(|tok| match tok {
                Token::Word(wd) => Some(wd.id),
                _ => None,
            })
            .collect();

        if word_ids.is_empty() {
            return true; // no words → vacuously covered
        }

        // Find the most recent mapping from this tier
        let mapping = match self.mappings.iter().rev()
            .find(|m| m.from_tier_id == from_tier_id)
        {
            Some(m) => m,
            None => return false, // no mapping at all
        };

        // Check every word has an entry
        word_ids.iter().all(|wid| {
            mapping.entries.iter().any(|e| e.source_word_id == *wid)
        })
    }

    /// Overall weave-readiness of this sentence.
    ///
    /// `Complete` = all 5 tiers valid + both diglot mappings present.
    /// `Incomplete` = some tiers present but not all requirements met.
    /// `Empty` = only base (or nothing) populated.
    pub fn weave_completeness(&self) -> Completeness {
        let mut has_any_non_base = false;
        let mut all_tiers_complete = true;

        // In simple mode the project never produces advanced/moderate tiers,
        // so they must not gate weave readiness. Limit the required-tier
        // set accordingly. In simple-triple mode `basic_base` is also off, so
        // only `base` + `basic_target` are required.
        let required_tiers: &[&str] = if self.simple_triple {
            &["base", "basic_target"]
        } else if self.simple_mode {
            &["base", "basic_target", "basic_base"]
        } else {
            Self::WEAVE_TIERS
        };

        for &tid in required_tiers {
            match self.tier_completeness(tid) {
                Completeness::Complete => {
                    if tid != "base" {
                        has_any_non_base = true;
                    }
                }
                Completeness::Incomplete => {
                    has_any_non_base = true;
                    all_tiers_complete = false;
                }
                Completeness::Empty => {
                    all_tiers_complete = false;
                }
            }
        }

        if !all_tiers_complete {
            return if has_any_non_base {
                Completeness::Incomplete
            } else {
                Completeness::Empty
            };
        }

        // All tiers complete — now check mappings
        if self.simple_triple {
            // basic_base is off; only the inverse diglot mapping
            // (basic_target → basic_base substitutions) is needed for the
            // diglot output. The forward mapping is not required.
            if self.has_inverse_diglot_mapping() {
                Completeness::Complete
            } else {
                Completeness::Incomplete
            }
        } else if self.has_diglot_mapping() && self.has_inverse_diglot_mapping() {
            Completeness::Complete
        } else if self.has_diglot_mapping() || self.has_inverse_diglot_mapping() {
            Completeness::Incomplete
        } else {
            // All tiers filled but no mappings yet
            Completeness::Incomplete
        }
    }

    /// Returns true if this sentence is ready for weave output.
    pub fn is_weave_ready(&self) -> bool {
        self.weave_completeness() == Completeness::Complete
    }

    /// Human-readable status string for a given tier, including mappings.
    pub fn tier_status_display(&self, tier_id: &str) -> &'static str {
        match self.tier_completeness(tier_id) {
            Completeness::Complete => "valid",
            Completeness::Incomplete => "incomplete",
            Completeness::Empty => "empty",
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

    #[test]
    fn test_weave_completeness_empty() {
        let mut s = Sentence::new("S1".into());
        s.update_tier_text_as_clean("base", "Hello world.".into());
        assert_eq!(s.weave_completeness(), Completeness::Empty);
        assert!(!s.is_weave_ready());
    }

    #[test]
    fn test_weave_completeness_incomplete() {
        let mut s = Sentence::new("S1".into());
        s.update_tier_text_as_clean("base", "Hello world.".into());
        s.update_tier_text_as_clean("basic_base", "Hello simple.".into());
        assert_eq!(s.weave_completeness(), Completeness::Incomplete);
    }

    #[test]
    fn test_weave_completeness_all_tiers_no_mappings() {
        let mut s = Sentence::new("S1".into());
        for tid in Sentence::WEAVE_TIERS {
            s.update_tier_text_as_clean(tid, format!("text for {}", tid));
        }
        // All tiers valid but no mappings — still incomplete
        assert_eq!(s.weave_completeness(), Completeness::Incomplete);
    }

    #[test]
    fn test_weave_completeness_complete() {
        use crate::domain::mapping::{TierMapping, MappingEntry};
        use crate::domain::primitives::WordId;

        let mut s = Sentence::new("S1".into());
        // Set tiers in parent-first order so that propagate_stale finds
        // no existing children to mark Stale.
        // Graph: base → advanced_target → moderate_target
        //        base → basic_base      → basic_target
        for tid in &["base", "advanced_target", "moderate_target", "basic_base", "basic_target"] {
            s.update_tier_text_as_clean(tid, format!("text for {}", tid));
        }

        // Add both diglot mappings
        let mut fwd = TierMapping::new("basic_base".into(), "basic_target".into());
        fwd.add_entry(MappingEntry {
            source_word_id: WordId(0),
            target_text: "texto".into(),
            is_viable: true,
            is_proper_noun: false,
            target_lemmas: vec!["texto".into()],
            target_wlemmas: Vec::new(),
        });
        s.add_mapping(fwd);

        let mut inv = TierMapping::new("basic_target".into(), "basic_base".into());
        inv.add_entry(MappingEntry {
            source_word_id: WordId(0),
            target_text: "text".into(),
            is_viable: true,
            is_proper_noun: false,
            target_lemmas: vec!["text".into()],
            target_wlemmas: Vec::new(),
        });
        s.add_mapping(inv);

        assert_eq!(s.weave_completeness(), Completeness::Complete);
        assert!(s.is_weave_ready());
    }

    #[test]
    fn test_tier_completeness() {
        let mut s = Sentence::new("S1".into());
        assert_eq!(s.tier_completeness("base"), Completeness::Empty);

        s.update_tier_text_as_clean("base", "Hello.".into());
        assert_eq!(s.tier_completeness("base"), Completeness::Complete);

        s.update_tier_text("base", "Edited.".into()); // Dirty state
        assert_eq!(s.tier_completeness("base"), Completeness::Incomplete);
    }

    #[test]
    fn propagate_stale_english_source_default() {
        // English-source: editing basic_base marks basic_target stale,
        // but editing basic_target does NOT mark basic_base stale.
        let mut s = Sentence::new("S1".into());
        assert!(!s.source_is_target);
        for tid in &["base", "basic_base", "basic_target"] {
            s.update_tier_text_as_clean(tid, format!("v0 {}", tid));
        }
        // Edit basic_target → basic_base remains clean.
        s.update_tier_text_as_clean("basic_target", "v1".into());
        assert_eq!(s.get_tier("basic_base").unwrap().state, TierState::Valid);

        // Reset and edit basic_base → basic_target goes stale.
        s.update_tier_text_as_clean("basic_target", "v2".into());
        s.update_tier_text_as_clean("basic_base", "v2".into());
        assert_eq!(s.get_tier("basic_target").unwrap().state, TierState::Stale);
    }

    #[test]
    fn propagate_stale_spanish_source_reverses_basic_branch() {
        // Spanish-source: basic_target is the parent, basic_base the child.
        // Editing basic_target should mark basic_base stale; editing
        // basic_base should NOT mark basic_target stale.
        let mut s = Sentence::new("S1".into());
        s.set_source_is_target(true);
        for tid in &["base", "basic_target", "basic_base"] {
            s.update_tier_text_as_clean(tid, format!("v0 {}", tid));
        }
        // basic_base is now Valid (was set last). Edit basic_target → basic_base stale.
        s.update_tier_text_as_clean("basic_target", "v1".into());
        assert_eq!(
            s.get_tier("basic_base").unwrap().state,
            TierState::Stale,
            "basic_base should be Stale after basic_target re-generation in source_is_target mode"
        );

        // Re-validate basic_base, then edit basic_base — basic_target must stay clean.
        s.update_tier_text_as_clean("basic_base", "v2".into());
        s.update_tier_text_as_clean("basic_base", "v3".into());
        assert_eq!(
            s.get_tier("basic_target").unwrap().state,
            TierState::Valid,
            "basic_target must remain clean when basic_base is regenerated downstream"
        );
    }
}
