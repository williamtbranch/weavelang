//*** START FILE: src/simulation/numerical_types.rs ***//
use std::collections::HashMap;
use crate::profile::{LearnerLemmaInfo, LemmaState}; // Using existing profile structs

// --- Numerical Learner Profile ---
#[derive(Debug, Clone, Default)]
pub struct NumericalLearnerProfile {
    pub vocabulary: HashMap<u32, LearnerLemmaInfo>, // Key is lemma_id (u32)
}

impl NumericalLearnerProfile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_lemma_info(&self, lemma_id: u32) -> Option<&LearnerLemmaInfo> {
        self.vocabulary.get(&lemma_id)
    }

    pub fn get_lemma_info_mut(&mut self, lemma_id: u32) -> &mut LearnerLemmaInfo {
        self.vocabulary.entry(lemma_id).or_insert_with(LearnerLemmaInfo::default)
    }

    pub fn is_lemma_known_or_active(&self, lemma_id: u32) -> bool {
        match self.get_lemma_info(lemma_id) {
            Some(info) => info.state == LemmaState::Known || info.state == LemmaState::Active,
            None => false, // If a lemma_id isn't in the profile, it's effectively "New"
        }
    }
    
    pub fn record_exposures(&mut self, lemma_ids: &[u32]) {
        for &lemma_id in lemma_ids {
            // It's assumed lemma_id 0 (or any specific ID) could be reserved if empty strings were an issue,
            // but dictionary now tries to avoid adding empty strings.
            // If an ID representing an "empty" or "invalid" lemma somehow gets here,
            // it would be processed like any other ID.
            let info = self.get_lemma_info_mut(lemma_id);
            info.exposure_count += 1;

            // Note: The default LearnerLemmaInfo has required_exposure_threshold = 20.
            // This logic correctly transitions states.
            if info.state == LemmaState::New && info.exposure_count > 0 {
                info.state = LemmaState::Active;
            }
            if info.state == LemmaState::Active && info.exposure_count >= info.required_exposure_threshold {
                info.state = LemmaState::Known;
            }
        }
    }

    // --- Counting methods ---
    pub fn count_known(&self) -> usize {
        self.vocabulary.values().filter(|info| info.state == LemmaState::Known).count()
    }
    
    pub fn count_active_only(&self) -> usize {
        self.vocabulary.values().filter(|info| info.state == LemmaState::Active).count()
    }

    pub fn count_total_known_or_active(&self) -> usize {
        self.vocabulary.values().filter(|info| info.state == LemmaState::Known || info.state == LemmaState::Active).count()
    }
    
    pub fn vocabulary_size(&self) -> usize {
        self.vocabulary.len() // Number of unique lemma IDs tracked in the profile
    }

    pub fn total_exposure_count(&self) -> u32 {
        self.vocabulary.values().map(|info| info.exposure_count).sum()
    }

    // Helper to set a lemma's state directly, e.g., when activating "New" words
    pub fn set_lemma_state(&mut self, lemma_id: u32, new_state: LemmaState) {
        let info = self.get_lemma_info_mut(lemma_id);
        info.state = new_state;
        // Optionally, if transitioning to Active from New, reset exposure count if desired
        // if old_state == LemmaState::New && new_state == LemmaState::Active {
        //     info.exposure_count = 1; // Or 0, depending on convention
        // }
    }
}

// --- Numerical representations of LLM data structures ---
// These structs remain largely the same as before (definitions only)

#[derive(Debug, Clone, Default)]
pub struct NumericalSegmentData {
    pub id_str: String, 
    pub text_original: String, 
}

#[derive(Debug, Clone, Default)]
pub struct NumericalPhraseAlignment {
    pub segment_id_str: String, 
    pub adv_s_span_original: String, 
    pub sim_e_span_original: String,
}

#[derive(Debug, Clone, Default)]
pub struct NumericalSegmentLemmas {
    pub segment_id_str: String, 
    pub lemma_ids: Vec<u32>,   
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
    pub segment_id_str: String, 
    pub entries: Vec<NumericalDiglotEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct NumericalProcessedSentence {
    pub sentence_id_str: String, 
    pub adv_s_original: String,
    pub sim_s_original: String,
    pub sim_e_original: String,
    pub sim_s_segments_numerical: Vec<NumericalSegmentData>, 
    pub phrase_alignments_numerical: Vec<NumericalPhraseAlignment>, 
    pub sim_s_lemmas_numerical: Vec<NumericalSegmentLemmas>, 
    pub adv_s_lemma_ids: Vec<u32>,
    pub diglot_map_numerical: Vec<NumericalDiglotSegmentMap>, 
    pub locked_phrase_segment_id_strs: Option<Vec<String>>, 
}

#[derive(Debug, Clone, Default)]
pub struct NumericalChapter {
    pub source_file_name_original: String,
    pub sentences_numerical: Vec<NumericalProcessedSentence>,
}
//*** END FILE: src/simulation/numerical_types.rs ***//