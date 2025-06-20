// src/simulation/numerical_types.rs
use crate::profile::{LearnerLemmaInfo, LemmaState};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriceAndCost {
    pub price: u32,
    pub cost: HashSet<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalLearnerProfile {
    pub vocabulary: HashMap<u32, LearnerLemmaInfo>,
}

impl NumericalLearnerProfile {
    pub fn new() -> Self { Self::default() }
    pub fn get_lemma_info(&self, lemma_id: u32) -> Option<&LearnerLemmaInfo> { self.vocabulary.get(&lemma_id) }
    pub fn get_lemma_info_mut(&mut self, lemma_id: u32) -> &mut LearnerLemmaInfo { self.vocabulary.entry(lemma_id).or_insert_with(LearnerLemmaInfo::default) }
    pub fn is_lemma_known_or_active(&self, lemma_id: u32) -> bool { match self.get_lemma_info(lemma_id) { Some(info) => info.state == LemmaState::Known || info.state == LemmaState::Active, None => false, } }
    pub fn record_exposures( &mut self, lemma_ids: &[u32], dictionary: &GlobalLemmaDictionary, ) { for &lemma_id in lemma_ids { if lemma_id == u32::MAX { continue; } let threshold = dictionary.get_threshold(lemma_id).unwrap_or(20); let info = self.get_lemma_info_mut(lemma_id); info.exposure_count += 1; if info.state == LemmaState::New && info.exposure_count > 0 { info.state = LemmaState::Active; } if info.state == LemmaState::Active && info.exposure_count >= threshold { info.state = LemmaState::Known; } } }
    pub fn set_lemma_state(&mut self, lemma_id: u32, new_state: LemmaState) { if lemma_id == u32::MAX { return; } let info = self.get_lemma_info_mut(lemma_id); info.state = new_state; }
    pub fn count_known(&self) -> usize { self.vocabulary.values().filter(|info| info.state == LemmaState::Known).count() }
    pub fn count_active_only(&self) -> usize { self.vocabulary.values().filter(|info| info.state == LemmaState::Active).count() }
    pub fn count_total_known_or_active(&self) -> usize { self.vocabulary.values().filter(|info| info.state == LemmaState::Known || info.state == LemmaState::Active).count() }
}

// --- ADDED SERDE DERIVES TO ALL SUB-STRUCTS ---
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalSegmentData { pub id_str: String, pub text_original: String, }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalSegmentLemmas { pub segment_id_str: String, pub lemma_ids: Vec<u32>, }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalPhraseAlignmentToEng { pub s_segment_id_str: String, pub sims_l3_segment_text_original: String, pub eng_span_text_original: String, pub eng_span_word_count: usize, }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalDiglotEntry { pub eng_word_original: String, pub spa_lemma_id: u32, pub exact_spa_form_original: String, pub viable: bool, }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalDiglotSegmentMap { pub s_segment_id_str: String, pub entries: Vec<NumericalDiglotEntry>, }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalAdvSegmentBundle { pub a_id_str: String, pub adv_text_original: String, pub adv_lemma_ids: Vec<u32>, pub simpler_text_original: String, pub simpler_lemma_ids: Vec<u32>, }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalProcessedSentence {
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
    #[serde(skip)]
    pub l0_upgrade_pc: PriceAndCost,
    #[serde(skip)]
    pub l1_segment_upgrade_pcs: HashMap<String, PriceAndCost>,
}

#[derive(Debug, Clone, Default)]
pub struct NumericalChapter {
    pub source_file_name_original: String,
    pub sentences_numerical: Vec<NumericalProcessedSentence>,
}