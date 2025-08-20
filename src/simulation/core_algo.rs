use super::numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;

/// Helper function to check if a slice of lemma IDs are all known to the learner.
fn are_lemmas_active(
    lemma_ids: &[u32],
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
) -> bool {
    // An empty list of lemmas is always considered active/known.
    if lemma_ids.is_empty() {
        return true;
    }

    // The .all() iterator ensures that EVERY lemma in the slice passes the test.
    lemma_ids.iter().all(|&id| {
        // Condition 1: The lemma is in the learner's active profile.
        if profile.is_lemma_active(id) {
            return true;
        }

        // Condition 2: The "Rare Word" rule. If the lemma is so rare it's not
        // in our master frequency list, we approve it to avoid getting stuck.
        if let Some(lemma_str) = dictionary.get_str(id) {
            if frequency_manager::get_rank_for_lemma(lemma_str).is_none() {
                // You can add a debug print here if you want to see which words are passing via this rule.
                println!("[DEBUG RARE WORD] Approving unknown lemma: '{}'", lemma_str);
                return true;
            }
        }
        
        // If neither of the above conditions are met, the learner does not know this word.
        false
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
    SimplerAdv(String),
    InverseDiglot(String),
}

#[derive(Debug, Clone)]
pub enum L1PartChoice {
    Spanish(String),
    Woven(String, bool), 
    English(String),
}

#[derive(Debug, Clone)]
pub struct ChosenLevelOutput {
    pub level: OutputLevel,
    pub lemma_ids: Vec<u32>,
    pub english_word_count: usize,
    pub l0_segment_choices: Option<Vec<L0SegmentChoice>>,
    pub l1_part_choices: Option<Vec<L1PartChoice>>,
}


/// Attempts to build a Level 0 (AdvancedWeave) sentence.
/// Returns Some(ChosenLevelOutput) on success, or None on failure.
/// Failure occurs if any single segment is not expressible under L0 rules.
// In src/simulation/core_algo.rs
fn try_build_advanced_weave(
    n_sentence: &NumericalProcessedSentence,
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
) -> Option<ChosenLevelOutput> {
    if n_sentence.adv_segment_bundles_numerical.is_empty() {
        return None;
    }

    let mut l0_candidate_choices: Vec<L0SegmentChoice> = Vec::new();
    let mut l0_collected_lemma_ids: Vec<u32> = Vec::new();

    for bundle in &n_sentence.adv_segment_bundles_numerical {
        // Path 1: Try Advanced Spanish
        if are_lemmas_active(&bundle.adv_lemma_ids, profile, dictionary) {
            l0_candidate_choices.push(L0SegmentChoice::Adv(bundle.adv_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.adv_lemma_ids);
            continue;
        }

        // Path 2: Try Simpler Advanced Spanish
        if are_lemmas_active(&bundle.simpler_lemma_ids, profile, dictionary) {
            l0_candidate_choices.push(L0SegmentChoice::SimplerAdv(bundle.simpler_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.simpler_lemma_ids);
            continue;
        }

        // Path 3: Try Inverse Diglot
        let total_words = bundle.simpler_text_words.len();
        if total_words == 0 {
            l0_candidate_choices.push(L0SegmentChoice::InverseDiglot("".to_string()));
            continue;
        }

        // Guard against cases where the number of words doesn't match the map entries.
        if bundle.simpler_text_words.len() != bundle.inverse_diglot_map_numerical.len() {
            return None; // Data mismatch, L0 fails.
        }

        let mut final_parts: Vec<String> = Vec::new();
        let mut temp_collected_lemmas: Vec<u32> = Vec::new();
        let mut substitutions = 0;
        let mut inverse_diglot_is_viable = true;

        for (i, word_token) in bundle.simpler_text_words.iter().enumerate() {
            final_parts.push(bundle.simpler_text_backgrounds[i].clone());
            
            // We can now safely use .get(i) because of the guard clause above.
            let diglot_entry = bundle.inverse_diglot_map_numerical.get(i).unwrap();
            let (_, lemma_id, eng_sub) = diglot_entry;

            if eng_sub == "PROPER_NOUN" {
                final_parts.push(word_token.text.clone());
            } else if profile.is_lemma_active(*lemma_id) {
                temp_collected_lemmas.push(*lemma_id);
                final_parts.push(word_token.text.clone());
            } else if eng_sub == "NO_SUB" {
                inverse_diglot_is_viable = false;
                break;
            } else {
                substitutions += 1;
                final_parts.push(eng_sub.clone());
            }
        }
        final_parts.push(bundle.simpler_text_backgrounds.last().unwrap().clone());

        if !inverse_diglot_is_viable {
            return None; // L0 attempt fails for this segment.
        }

        let substitution_ratio = substitutions as f32 / total_words as f32;
        if substitution_ratio > 0.5 && total_words > 1 {
            return None; // Fails 50% rule.
        }
        
        l0_candidate_choices.push(L0SegmentChoice::InverseDiglot(final_parts.concat()));
        l0_collected_lemma_ids.extend(temp_collected_lemmas);
    }
    
    Some(ChosenLevelOutput {
        level: OutputLevel::AdvancedWeave,
        lemma_ids: l0_collected_lemma_ids,
        english_word_count: 0,
        l0_segment_choices: Some(l0_candidate_choices),
        l1_part_choices: None,
    })
}

pub fn determine_and_annotate_sentence_expression(
    n_sentence: &mut NumericalProcessedSentence,
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
    _inverse_diglot_threshold: f32,
) -> ChosenLevelOutput {
    
    if let Some(l0_output) = try_build_advanced_weave(n_sentence, profile, dictionary) {
        return l0_output;
    }

    let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut l1_part_choices: Vec<L1PartChoice> = Vec::new();
    let mut l1_total_english_word_count: usize = 0;

    // The main loop is now over the `phrase_alignments` which are derived from the `base_tier`.
    // This is the source of truth for the L1 structure.
    for alignment in &n_sentence.phrase_alignments_l3_to_eng_numerical {
        let segment_id = &alignment.s_segment_id_str;
        
        // Find the corresponding Spanish segment and its lemmas
        let l3_segment = n_sentence.sims_l3_segments_numerical.iter().find(|s| &s.id_str == segment_id);
        let segment_lemmas = n_sentence.l3_simsl_per_segment_numerical.iter().find(|sl| &sl.segment_id_str == segment_id);

        if segment_lemmas.map_or(false, |l| are_lemmas_active(&l.lemma_ids, profile, dictionary)) {
            // Path 1: The Spanish phrase is fully known. Use it.
            l1_part_choices.push(L1PartChoice::Spanish(l3_segment.unwrap().text_original.clone()));
            l1_collected_lemma_ids.extend(&segment_lemmas.unwrap().lemma_ids);

        } else {
            // Path 2: Fallback to English and attempt diglotting.
            let mut final_parts: Vec<String> = Vec::new();
            let mut substitutions_made = 0;
            let diglot_map_for_segment = n_sentence.diglot_map_numerical.iter().find(|dm| &dm.s_segment_id_str == segment_id);

            for (i, word_token) in alignment.eng_span_words.iter().enumerate() {
                final_parts.push(alignment.eng_span_backgrounds[i].clone());
                let mut substituted = false;
                
                if let Some(diglot_map) = diglot_map_for_segment {
                    if let Some(entry) = diglot_map.entries.iter().find(|e| e.base_word_di == word_token.diglot_index) {
                        let is_metadata_token = entry.exact_spa_form_original == "PROPER_NOUN" || entry.exact_spa_form_original == "NO_SUB";
                        
                        if !is_metadata_token && entry.viable && profile.is_lemma_active(entry.spa_lemma_id) {
                            substitutions_made += 1;
                            l1_collected_lemma_ids.push(entry.spa_lemma_id);
                            final_parts.push(entry.exact_spa_form_original.clone());
                            substituted = true;
                        }
                    }
                }
                if !substituted {
                    final_parts.push(word_token.text.clone());
                }
            }
            final_parts.push(alignment.eng_span_backgrounds.last().unwrap().clone());
            
            let final_text = final_parts.concat();
            l1_total_english_word_count += alignment.eng_span_words.len() - substitutions_made;
            
            if substitutions_made > 0 {
                l1_part_choices.push(L1PartChoice::Woven(final_text, true));
            } else {
                l1_part_choices.push(L1PartChoice::English(alignment.eng_span_text_original.clone()));
            }
        }
    }
    
    ChosenLevelOutput {
        level: OutputLevel::SimpleHybrid,
        lemma_ids: l1_collected_lemma_ids,
        english_word_count: l1_total_english_word_count,
        l0_segment_choices: None,
        l1_part_choices: Some(l1_part_choices),
    }
}