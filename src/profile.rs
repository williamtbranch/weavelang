use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LemmaState { New, Active, Known }

// Added PartialEq here to allow HashMaps of LearnerLemmaInfo to be compared
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)] 
pub struct LearnerLemmaInfo { 
    pub state: LemmaState, 
    pub exposure_count: u32, 
    pub required_exposure_threshold: u32 
}

impl Default for LearnerLemmaInfo { 
    fn default() -> Self { 
        Self { 
            state: LemmaState::New, 
            exposure_count: 0, 
            // Default threshold for a word to become "Known" after being "Active"
            // This can be overridden per lemma if adaptive thresholds are implemented later.
            required_exposure_threshold: 20 
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearnerProfile { 
    // Made vocabulary public to allow direct comparison in main.rs for the saturation check.
    // This is acceptable for this prototype's internal logic.
    pub vocabulary: HashMap<String, LearnerLemmaInfo> 
}

impl LearnerProfile {
    pub fn new() -> Self { 
        Self::default() 
    }

    // Helper to consistently use lowercase keys for lemmas
    fn get_key(lemma_str: &str) -> String { 
        lemma_str.to_lowercase() 
    }

    // Gets a mutable reference to a lemma's info, creating a default if it doesn't exist.
    pub fn get_lemma_info_mut(&mut self, lemma_str: &str) -> &mut LearnerLemmaInfo { 
        self.vocabulary.entry(Self::get_key(lemma_str)).or_default() 
    }

    // Gets an immutable reference to a lemma's info.
    pub fn get_lemma_info(&self, lemma_str: &str) -> Option<&LearnerLemmaInfo> { 
        self.vocabulary.get(&Self::get_key(lemma_str)) 
    }

    // Checks if a lemma is considered "Known" or "Active".
    pub fn is_lemma_known_or_active(&self, lemma_str: &str) -> bool { 
        match self.get_lemma_info(lemma_str) { 
            Some(info) => info.state == LemmaState::Known || info.state == LemmaState::Active, 
            None => false // If not in profile, it's implicitly "New" and thus not known/active.
        } 
    }

    // Records exposures to a list of lemmas and updates their states.
    pub fn record_exposures(&mut self, lemmas: &[String]) {
        for lemma_s in lemmas {
            if lemma_s.trim().is_empty() { continue; } // Ignore empty lemma strings

            let info = self.get_lemma_info_mut(lemma_s);
            info.exposure_count += 1;

            // Transition New -> Active on first meaningful exposure.
            // L4 might also directly set a word to Active when introducing it.
            if info.state == LemmaState::New && info.exposure_count > 0 { 
                info.state = LemmaState::Active; 
                // Optional: Reset exposure_count to 1 if 'Active' state means "just introduced".
                // info.exposure_count = 1; 
                // Current logic: exposure accumulates from 0.
            }

            // Transition Active -> Known when exposure threshold is met.
            if info.state == LemmaState::Active && info.exposure_count >= info.required_exposure_threshold { 
                info.state = LemmaState::Known; 
            }
        }
    }

    // Counts lemmas that are either "Known" or "Active".
    pub fn count_total_known_or_active(&self) -> usize { 
        self.vocabulary.values().filter(|info| info.state == LemmaState::Known || info.state == LemmaState::Active).count() 
    }

    // Returns the total number of unique lemmas in the profile (vocabulary size).
    pub fn vocabulary_size(&self) -> usize { 
        self.vocabulary.len() 
    }

    // Counts lemmas that are strictly "Known".
    pub fn count_known(&self) -> usize {
        self.vocabulary.values().filter(|info| info.state == LemmaState::Known).count()
    }

    // Counts lemmas that are strictly "Active" (not including "Known").
    pub fn count_active_only(&self) -> usize { 
        self.vocabulary.values().filter(|info| info.state == LemmaState::Active).count()
    }
    
    // Calculates the sum of all exposure counts across all lemmas in the profile.
    pub fn total_exposure_count(&self) -> u32 {
        self.vocabulary.values().map(|info| info.exposure_count).sum()
    }
}