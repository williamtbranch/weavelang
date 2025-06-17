// src/simulation/numerical_types.rs
use crate::profile::{LearnerLemmaInfo, LemmaState};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- Numerical Learner Profile ---
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericalLearnerProfile {
    pub vocabulary: HashMap<u32, LearnerLemmaInfo>,
}

impl NumericalLearnerProfile {
    pub fn new() -> Self { Self::default() }

    pub fn get_lemma_info(&self, lemma_id: u32) -> Option<&LearnerLemmaInfo> {
        self.vocabulary.get(&lemma_id)
    }

    /// Gets mutable lemma info. If the lemma is new to the profile, it creates a default entry.
    /// This method does NOT know the lemma's required threshold.
    pub fn get_lemma_info_mut(&mut self, lemma_id: u32) -> &mut LearnerLemmaInfo {
        self.vocabulary.entry(lemma_id).or_insert_with(LearnerLemmaInfo::default)
    }

    /// Checks if a lemma is considered "learned enough" (Active or Known) for generating comprehensible text.
    pub fn is_lemma_known_or_active(&self, lemma_id: u32) -> bool {
        match self.get_lemma_info(lemma_id) {
            Some(info) => info.state == LemmaState::Known || info.state == LemmaState::Active,
            None => false,
        }
    }

    /// Records exposures to a list of lemmas, updating their counts and states.
    /// This is the primary method for "learning".
    pub fn record_exposures(
        &mut self,
        lemma_ids: &[u32],
        dictionary: &GlobalLemmaDictionary, // <-- Now requires the dictionary
    ) {
        for &lemma_id in lemma_ids {
            if lemma_id == u32::MAX { continue; }

            // Get the required threshold from the dictionary, falling back to a default if not found.
            let threshold = dictionary.get_threshold(lemma_id).unwrap_or(20);

            let info = self.get_lemma_info_mut(lemma_id);
            info.exposure_count += 1;

            if info.state == LemmaState::New && info.exposure_count > 0 {
                info.state = LemmaState::Active;
            }
            if info.state == LemmaState::Active && info.exposure_count >= threshold {
                info.state = LemmaState::Known;
            }
        }
    }

    /// Directly sets the state of a lemma, typically used for activating new words during simulation.
    pub fn set_lemma_state(&mut self, lemma_id: u32, new_state: LemmaState) {
        if lemma_id == u32::MAX { return; }
        let info = self.get_lemma_info_mut(lemma_id);
        info.state = new_state;
    }

    // --- Helper/Counting Functions ---
    
    pub fn count_known(&self) -> usize {
        self.vocabulary.values().filter(|info| info.state == LemmaState::Known).count()
    }
    
    pub fn count_active_only(&self) -> usize {
        self.vocabulary.values().filter(|info| info.state == LemmaState::Active).count()
    }

    pub fn count_total_known_or_active(&self) -> usize {
        self.vocabulary.values().filter(|info| info.state == LemmaState::Known || info.state == LemmaState::Active).count()
    }
}

// --- Numerical representations of JSON data structures (These are unchanged) ---
#[derive(Debug, Clone, Default)]
pub struct NumericalSegmentData {
    pub id_str: String,
    pub text_original: String,
}
#[derive(Debug, Clone, Default)]
pub struct NumericalSegmentLemmas {
    pub segment_id_str: String,
    pub lemma_ids: Vec<u32>,
}
#[derive(Debug, Clone, Default)]
pub struct NumericalPhraseAlignmentToEng {
    pub s_segment_id_str: String,
    pub sims_l3_segment_text_original: String,
    pub eng_span_text_original: String,
}
#[derive(Debug, Clone, Default)]
pub struct NumericalDiglotEntry {
    pub eng_word_original: String,
    pub spa_lemma_id: u32,
    pub exact_spa_form_original: String,
    pub viable: bool,
}
#[derive(Debug, Clone, Default)]
pub struct NumericalDiglotSegmentMap {
    pub s_segment_id_str: String,
    pub entries: Vec<NumericalDiglotEntry>,
}
#[derive(Debug, Clone, Default)]
pub struct NumericalAdvSegmentBundle {
    pub a_id_str: String,
    pub adv_text_original: String,
    pub adv_lemma_ids: Vec<u32>,
    pub simpler_text_original: String,
    pub simpler_lemma_ids: Vec<u32>,
}
#[derive(Debug, Clone, Default)]
pub struct NumericalProcessedSentence {
    pub sentence_id_str: String,
    pub eng_text_original: String,
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