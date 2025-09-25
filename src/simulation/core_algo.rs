// In src/simulation/core_algo.rs

use super::numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence, VLevelRecipe};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;
use crate::types::json_types::{JsonTokenType};
use std::collections::HashMap;

// --- NEW HELPER FUNCTION ---
// This function checks if all lemmas in a list are "known" based on a specific V-Level for that tier.
fn are_lemmas_known(
    lemma_ids: &[u32],
    tier_v_level: u32,
    dictionary: &GlobalLemmaDictionary,
) -> bool {
    if lemma_ids.is_empty() {
        return true; // A segment with no lemmas is always "known".
    }
    // A tier is considered "exhausted" if its v-level is MAX.
    if tier_v_level == u32::MAX {
        return true;
    }
    lemma_ids.iter().all(|&id| {
        if let Some(lemma_str) = dictionary.get_str(id) {
            if let Some(rank) = frequency_manager::get_rank_for_lemma(lemma_str) {
                // The core logic: the rank must be within the V-Level for this specific tier.
                return rank <= tier_v_level;
            }
        }
        // Lemmas not found in the frequency list (like proper nouns) are considered "known".
        true
    })
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OutputLevel {
    AdvancedWeave,
    SimpleHybrid,
}

impl Default for OutputLevel {
    fn default() -> Self {
        OutputLevel::AdvancedWeave
    }
}

#[derive(Debug, Clone)]
pub enum L0SegmentChoice {
    Adv(String),
    Mod(String),
    Bas(String),
    Sim(String),
    InverseDiglot(String),
}

#[derive(Debug, Clone)]
pub struct ChosenLevelOutput {
    pub level: OutputLevel,
    pub lemma_ids: Vec<u32>,
    pub l0_segment_choices: Option<Vec<L0SegmentChoice>>,
    pub l1_final_text: Option<String>,
    pub spanish_word_count: usize,
    pub english_word_count: usize,
}

fn try_build_advanced_weave(
    n_sentence: &NumericalProcessedSentence,
    dictionary: &GlobalLemmaDictionary,
    // The four u32 levels are replaced by a single recipe struct.
    v_levels: &VLevelRecipe,
    inverse_diglot_threshold: f32,
) -> Option<ChosenLevelOutput> {
    if n_sentence.adv_segment_bundles_numerical.is_empty() {
        return None;
    }

    // --- THIS IS THE KEY CHANGE ---
    // The inverse diglot level is now derived from the recipe by construction.
    let inv_diglot_v_level = v_levels.inv_diglot_level();
    
    let mut l0_candidate_choices: Vec<L0SegmentChoice> = Vec::new();
    let mut l0_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut spanish_words = 0;
    let mut english_words = 0;

    for bundle in &n_sentence.adv_segment_bundles_numerical {
        // --- The Four-Tier Cascade now uses fields from the struct ---
        if are_lemmas_known(&bundle.adv_lemma_ids, v_levels.adv, dictionary) {
            l0_candidate_choices.push(L0SegmentChoice::Adv(bundle.adv_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.adv_lemma_ids);
            spanish_words += bundle.adv_text_original.split_whitespace().count();
            continue;
        }
        if are_lemmas_known(&bundle.mod_lemma_ids, v_levels.mod_v, dictionary) {
            l0_candidate_choices.push(L0SegmentChoice::Mod(bundle.mod_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.mod_lemma_ids);
            spanish_words += bundle.mod_text_original.split_whitespace().count();
            continue;
        }
        if are_lemmas_known(&bundle.bas_lemma_ids, v_levels.bas, dictionary) {
            l0_candidate_choices.push(L0SegmentChoice::Bas(bundle.bas_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.bas_lemma_ids);
            spanish_words += bundle.bas_text_original.split_whitespace().count();
            continue;
        }
        if are_lemmas_known(&bundle.sim_lemma_ids, v_levels.sim, dictionary) {
            l0_candidate_choices.push(L0SegmentChoice::Sim(bundle.sim_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.sim_lemma_ids);
            spanish_words += bundle.sim_text_original.split_whitespace().count();
            continue;
        }

        // --- Inverse Diglot Fallback (Now uses the derived inv_diglot_v_level) ---
        let total_words_in_sim_seg = bundle.sim_text_words.len();
        if total_words_in_sim_seg == 0 {
            l0_candidate_choices.push(L0SegmentChoice::InverseDiglot("".to_string()));
            continue;
        }
        if bundle.sim_text_words.len() != bundle.inverse_diglot_map_numerical.len() {
            return None; // Data integrity error
        }

        let mut final_parts: Vec<String> = Vec::new();
        let mut temp_collected_lemmas: Vec<u32> = Vec::new();
        let mut substitutions = 0;
        let mut inverse_diglot_is_viable = true;

        for (i, word_token) in bundle.sim_text_words.iter().enumerate() {
            final_parts.push(bundle.sim_text_backgrounds[i].clone());
            let diglot_entry = bundle.inverse_diglot_map_numerical.get(i).unwrap();
            let (_, lemma_ids, eng_sub, eng_wc) = diglot_entry;

            if eng_sub == "PROPER_NOUN" {
                final_parts.push(word_token.text.clone());
                spanish_words += 1;
            // --- THIS LOGIC IS NOW CHANGED TO USE THE DERIVED LEVEL ---
            } else if are_lemmas_known(lemma_ids, inv_diglot_v_level, dictionary) {
                temp_collected_lemmas.extend(lemma_ids);
                final_parts.push(word_token.text.clone());
                spanish_words += 1;
            } else if eng_sub == "NO_SUB" {
                inverse_diglot_is_viable = false;
                break;
            } else {
                substitutions += 1;
                final_parts.push(eng_sub.clone());
                english_words += eng_wc;
            }
        }
        final_parts.push(bundle.sim_text_backgrounds.last().unwrap().clone());
        if !inverse_diglot_is_viable { return None; }
        let substitution_ratio = substitutions as f32 / total_words_in_sim_seg as f32;
        if substitution_ratio > inverse_diglot_threshold && total_words_in_sim_seg > 1 {
            return None;
        }

        l0_candidate_choices.push(L0SegmentChoice::InverseDiglot(final_parts.concat()));
        l0_collected_lemma_ids.extend(temp_collected_lemmas);
    }

    Some(ChosenLevelOutput {
        level: OutputLevel::AdvancedWeave,
        lemma_ids: l0_collected_lemma_ids,
        l0_segment_choices: Some(l0_candidate_choices),
        l1_final_text: None,
        spanish_word_count: spanish_words,
        english_word_count: english_words,
    })
}

pub fn determine_and_annotate_sentence_expression(
    n_sentence: &mut NumericalProcessedSentence,
    profile: &NumericalLearnerProfile, // The real-world profile is now only for L1 fallbacks
    dictionary: &GlobalLemmaDictionary,
    // The function now takes the single recipe struct
    v_levels: &VLevelRecipe,
    inverse_diglot_threshold: f32,
) -> ChosenLevelOutput {
    if let Some(l0_output) =
        try_build_advanced_weave(n_sentence, dictionary, v_levels, inverse_diglot_threshold)
    {
        return l0_output;
    }

    // --- L1 FALLBACK: This logic is unchanged but now uses the learner's real profile ---
    let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut spanish_words = 0;
    let mut english_words = 0;
    let mut final_parts: Vec<String> = Vec::new();

    let diglot_lookup: HashMap<usize, _> = n_sentence
        .diglot_map_numerical
        .iter()
        .flat_map(|seg_map| &seg_map.entries)
        .map(|entry| (entry.base_word_di, entry))
        .collect();

    let base_tokens = n_sentence
        .base_tier_tokenized
        .first()
        .map_or(Vec::new(), |seg| seg.tokenized_text.clone());

    for token in &base_tokens {
        if token.token_type == JsonTokenType::Background {
            final_parts.push(token.value.clone());
            continue;
        }

        let di = token.diglot_index.unwrap_or(usize::MAX);
        let mut substituted = false;

        if let Some(entry) = diglot_lookup.get(&di) {
            let is_metadata_token = entry.exact_spa_form_original == "PROPER_NOUN"
                || entry.exact_spa_form_original == "NO_SUB";
            
            // --- THIS IS THE NEW LOGIC ---
            // The L1 fallback will render the Spanish form ("Alicia") ONLY IF:
            // 1. It's not a metadata token (like NO_SUB).
            // 2. The entry is marked as viable.
            // 3. The learner knows the required lemmas for the Spanish word.
            // 4. AND the original base word was NOT a proper noun.
            if !is_metadata_token && entry.viable && profile.are_lemmas_active(&entry.spa_lemma_ids) && !entry.is_base_token_pn {
                final_parts.push(entry.exact_spa_form_original.clone());
                l1_collected_lemma_ids.extend(&entry.spa_lemma_ids);
                spanish_words += entry.exact_spa_form_original.split_whitespace().count();
                substituted = true;
            }
            // --- END OF NEW LOGIC ---
        }

        if !substituted {
            final_parts.push(token.value.clone());
            english_words += token.value.split_whitespace().count();
        }
    }
    ChosenLevelOutput {
        level: OutputLevel::SimpleHybrid,
        lemma_ids: l1_collected_lemma_ids,
        l0_segment_choices: None,
        l1_final_text: Some(final_parts.concat()),
        spanish_word_count: spanish_words,
        english_word_count: english_words,
    }
}