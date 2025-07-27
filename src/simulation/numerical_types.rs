// src/simulation/numerical_types.rs
use crate::profile::{LearnerLemmaInfo, LemmaState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- This struct is NEW. It will hold the "word" part of a token. ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordToken {
    pub text: String,
    // The index of the corresponding entry in the relevant diglot map.
    // This provides a direct link to the substitution data.
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
            self.vocabulary.insert(lemma_id, LearnerLemmaInfo::default());
        }
    }
    pub fn get_lemma_info(&self, lemma_id: u32) -> Option<&LearnerLemmaInfo> {
        self.vocabulary.get(&lemma_id)
    }
    pub fn get_lemma_info_mut(&mut self, lemma_id: u32) -> &mut LearnerLemmaInfo {
        self.vocabulary.entry(lemma_id).or_insert_with(LearnerLemmaInfo::default)
    }
    pub fn set_lemma_state(&mut self, lemma_id: u32, new_state: LemmaState) {
        if lemma_id != u32::MAX {
            let info = self.get_lemma_info_mut(lemma_id);
            info.state = new_state;
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalSegmentData {
    pub id_str: String,
    pub text_original: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalSegmentLemmas {
    pub segment_id_str: String,
    pub lemma_ids: Vec<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalPhraseAlignmentToEng {
    pub s_segment_id_str: String,
    pub sims_l3_segment_text_original: String,
    pub eng_span_text_original: String,
    pub eng_span_word_count: usize,
    // --- NEW FIELDS to hold the tokenized English phrase ---
    pub eng_span_words: Vec<WordToken>,
    pub eng_span_backgrounds: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalDiglotEntry {
    pub eng_word_original: String,
    pub spa_lemma_id: u32,
    pub exact_spa_form_original: String,
    pub viable: bool,
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
    pub simpler_text_original: String,
    pub simpler_lemma_ids: Vec<u32>,
    pub inverse_diglot_map_numerical: Vec<(String, u32, String)>,
    // --- NEW FIELDS to hold the tokenized Simpler Spanish phrase for inverse diglot ---
    pub simpler_text_words: Vec<WordToken>,
    pub simpler_text_backgrounds: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalProcessedSentence {
    pub source_file_name_original: String,
    pub sentence_id_str: String,
    pub eng_text_original: String,
    pub eng_text_word_count: usize,
    pub adv_s_text_original: String,
    pub adv_sl_overall_lemma_ids: Vec<u32>,
    pub adv_segment_bundles_numerical: Vec<NumericalAdvSegmentBundle>,
    pub simpler_adv_s_text_original: String,
    pub simpler_adv_sl_overall_lemma_ids: Vec<u32>,
    pub l3_sim_s_text_original: String,
    pub l3_sim_sl_overall_lemma_ids: Vec<u32>,
    pub sims_l3_segments_numerical: Vec<NumericalSegmentData>,
    pub phrase_alignments_l3_to_eng_numerical: Vec<NumericalPhraseAlignmentToEng>,
    pub l3_simsl_per_segment_numerical: Vec<NumericalSegmentLemmas>,
    pub diglot_map_numerical: Vec<NumericalDiglotSegmentMap>,
}

#[derive(Debug, Clone, Default)]
pub struct NumericalChapter {
    pub source_file_name_original: String,
    pub sentences_numerical: Vec<NumericalProcessedSentence>,
}