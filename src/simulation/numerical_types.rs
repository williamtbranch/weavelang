// In src/simulation/numerical_types.rs

use crate::profile::LearnerLemmaInfo;
use crate::types::json_types::JsonSegmentV2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordToken {
    pub text: String,
    pub diglot_index: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalLearnerProfile {
    pub vocabulary: HashMap<u32, LearnerLemmaInfo>,
}

impl NumericalLearnerProfile {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_lemma_active(&self, lemma_id: u32) -> bool {
        self.vocabulary.contains_key(&lemma_id)
    }
    pub fn activate_lemma(&mut self, lemma_id: u32) {
        if lemma_id != u32::MAX {
            self.vocabulary
                .insert(lemma_id, LearnerLemmaInfo::default());
        }
    }
    pub fn are_lemmas_active(&self, lemma_ids: &[u32]) -> bool {
        if lemma_ids.is_empty() {
            return true;
        }
        lemma_ids.iter().all(|&id| self.is_lemma_active(id))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalDiglotEntry {
    pub base_word_di: usize,
    pub eng_word_original: String,
    pub spa_lemma_ids: Vec<u32>,
    pub exact_spa_form_original: String,
    pub viable: bool,
    pub eng_word_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalDiglotSegmentMap {
    pub s_segment_id_str: String,
    pub entries: Vec<NumericalDiglotEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalAdvSegmentBundle {
    pub a_id_str: String,
    // --- NEW FIELDS ADDED ---
    pub adv_text_original: String,
    pub adv_lemma_ids: Vec<u32>,
    pub mod_text_original: String,
    pub mod_lemma_ids: Vec<u32>,
    pub bas_text_original: String,
    pub bas_lemma_ids: Vec<u32>,
    // This is now correctly named "sim" for Simple
    pub sim_text_original: String, 
    pub sim_lemma_ids: Vec<u32>,
    // The inverse map is based on the Simple tier
    pub inverse_diglot_map_numerical: Vec<(String, Vec<u32>, String, usize)>,
    pub sim_text_words: Vec<WordToken>,
    pub sim_text_backgrounds: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalProcessedSentence {
    pub source_file_name_original: String,
    pub sentence_id_str: String,
    pub eng_text_original: String,
    pub eng_text_word_count: usize,
    pub base_tier_tokenized: Vec<JsonSegmentV2>,
    pub adv_s_text_original: String,
    pub adv_sl_overall_lemma_ids: Vec<u32>,
    pub adv_segment_bundles_numerical: Vec<NumericalAdvSegmentBundle>,
    pub diglot_map_numerical: Vec<NumericalDiglotSegmentMap>,
}

#[derive(Debug, Clone, Default)]
pub struct NumericalChapter {
    pub source_file_name_original: String,
    pub sentences_numerical: Vec<NumericalProcessedSentence>,
}