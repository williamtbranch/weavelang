// src/simulation/numerical_types.rs
use crate::profile::LearnerLemmaInfo;
use crate::types::json_types::JsonSegmentV2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- NEW STRUCT ADDED HERE ---
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct LLevelRecipe {
    pub bas: f32,
    pub mod_v: f32,
    pub adv: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct VLevelRecipe {
    pub bas: u32,
    pub mod_v: u32,
    pub adv: u32,
}

impl Default for VLevelRecipe {
    fn default() -> Self {
        Self {
            bas: 0,
            mod_v: 0,
            adv: 0,
        }
    }
}

// ... (rest of the file is unchanged) ...

impl VLevelRecipe {
    pub fn inv_diglot_level(&self) -> u32 {
        *([self.mod_v, self.adv].iter().max().unwrap_or(&0))
    }
}

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
    pub fn new() -> Self { Self::default() }
    pub fn is_lemma_active(&self, lemma_id: u32) -> bool { self.vocabulary.contains_key(&lemma_id) }
    pub fn activate_lemma(&mut self, lemma_id: u32) {
        if lemma_id != u32::MAX { self.vocabulary.insert(lemma_id, LearnerLemmaInfo::default()); }
    }
    pub fn are_lemmas_active(&self, lemma_ids: &[u32]) -> bool {
        if lemma_ids.is_empty() { return true; }
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
    pub is_base_token_pn: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalDiglotSegmentMap {
    pub s_segment_id_str: String,
    pub entries: Vec<NumericalDiglotEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalAdvSegmentBundle {
    pub a_id_str: String,
    pub adv_text_original: String,
    pub adv_lemma_ids: Vec<u32>,
    pub mod_text_original: String,
    pub mod_lemma_ids: Vec<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalProcessedSentence {
    pub source_file_name_original: String,
    pub sentence_id_str: String,
    
    pub adv_segment_bundles_numerical: Vec<NumericalAdvSegmentBundle>,
    
    pub basic_base_tier_tokenized: Vec<JsonSegmentV2>,
    pub basic_target_tier_tokenized: Vec<JsonSegmentV2>,
    pub basic_target_lemma_ids: Vec<u32>,
    pub basic_diglot_map_numerical: Vec<NumericalDiglotSegmentMap>,
    pub basic_inverse_diglot_map_numerical: Vec<(String, Vec<u32>, String, usize, usize)>,
    
    pub eng_text_original: String,
    pub eng_text_word_count: usize,
    pub adv_s_text_original: String,
}

#[derive(Debug, Clone, Default)]
pub struct NumericalChapter {
    pub source_file_name_original: String,
    pub sentences_numerical: Vec<NumericalProcessedSentence>,
}