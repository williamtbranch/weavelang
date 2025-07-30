//*** START FILE: src/simulation/core_algo.rs ***//
// In file: src/simulation/core_algo.rs

use super::numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;

/// Helper function to check if a slice of lemma IDs are all known to the learner.
fn are_lemmas_active(
    lemma_ids: &[u32],
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
) -> bool {
    if lemma_ids.is_empty() {
        return true;
    }
    lemma_ids.iter().all(|&id| {
        if profile.is_lemma_active(id) {
            return true;
        }
        if let Some(lemma_str) = dictionary.get_str(id) {
            if frequency_manager::get_rank_for_lemma(lemma_str).is_none() {
                return true; // "Rare word" rule
            }
        }
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
fn try_build_advanced_weave(
    n_sentence: &NumericalProcessedSentence,
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
) -> Option<ChosenLevelOutput> {
    // If there are no advanced segments defined at all, L0 is not possible.
    if n_sentence.adv_segment_bundles_numerical.is_empty() {
        return None;
    }

    let mut l0_candidate_choices: Vec<L0SegmentChoice> = Vec::new();
    let mut l0_collected_lemma_ids: Vec<u32> = Vec::new();

    // Iterate through each segment "column". If any column fails, this whole function will return None.
    for bundle in &n_sentence.adv_segment_bundles_numerical {
        // Path 1: Try Advanced Spanish
        if are_lemmas_active(&bundle.adv_lemma_ids, profile, dictionary) {
            l0_candidate_choices.push(L0SegmentChoice::Adv(bundle.adv_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.adv_lemma_ids);
            continue; // Success for this column, move to the next.
        }

        // Path 2: Try Simpler Advanced Spanish
        if are_lemmas_active(&bundle.simpler_lemma_ids, profile, dictionary) {
            l0_candidate_choices.push(L0SegmentChoice::SimplerAdv(bundle.simpler_text_original.clone()));
            l0_collected_lemma_ids.extend(&bundle.simpler_lemma_ids);
            continue; // Success for this column, move to the next.
        }

        // Path 3: Try Inverse Diglot with the 50% rule
        let total_words = bundle.simpler_text_words.len();
        if total_words == 0 {
            // Empty segment is trivially successful
            l0_candidate_choices.push(L0SegmentChoice::InverseDiglot("".to_string()));
            continue;
        }

        let mut final_parts: Vec<String> = Vec::new();
        let mut temp_collected_lemmas: Vec<u32> = Vec::new();
        let mut substitutions = 0;
        let mut inverse_diglot_is_viable = true;

        for (i, word_token) in bundle.simpler_text_words.iter().enumerate() {
            final_parts.push(bundle.simpler_text_backgrounds[i].clone());

            let diglot_entry = bundle.inverse_diglot_map_numerical.iter()
                .find(|(original_word, _, _)| original_word == &word_token.text);

            if let Some((_, lemma_id, eng_sub)) = diglot_entry {
                // *** L0 PROPER_NOUN LOGIC ***
                if eng_sub == "PROPER_NOUN" {
                    // Always keep the Spanish word for proper nouns in L0. Do not count as a substitution.
                    final_parts.push(word_token.text.clone());
                } else if profile.is_lemma_active(*lemma_id) {
                    temp_collected_lemmas.push(*lemma_id);
                    final_parts.push(word_token.text.clone());
                } else if eng_sub == "NO_SUB" {
                    inverse_diglot_is_viable = false; // Cannot substitute, so path is blocked
                    break;
                } else {
                    substitutions += 1;
                    final_parts.push(eng_sub.clone());
                }
            } else {
                final_parts.push(word_token.text.clone()); // Not in map, assume punctuation/etc.
            }
        }
        final_parts.push(bundle.simpler_text_backgrounds.last().unwrap().clone());

        // Check viability based on the loop results
        if !inverse_diglot_is_viable {
            return None; // This segment is un-expressible, so the entire L0 attempt fails.
        }

        // Apply 50% rule, but waive for single-word segments
        if total_words > 1 {
            let substitution_ratio = substitutions as f32 / total_words as f32;
            if substitution_ratio > 0.5 {
                return None; // Fails the 50% rule. This entire L0 attempt fails.
            }
        }
        
        // If we get here, the inverse diglot path is viable for this segment.
        l0_candidate_choices.push(L0SegmentChoice::InverseDiglot(final_parts.concat()));
        l0_collected_lemma_ids.extend(temp_collected_lemmas);
    }
    
    // If we successfully processed all segments without returning None, the L0 attempt is a success.
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
    // This argument is now unused due to the per-segment 50% rule,
    // but is kept for signature compatibility with the caller.
    _inverse_diglot_threshold: f32,
) -> ChosenLevelOutput {
    
    // --- L0: Advanced Weave Attempt ---
    if let Some(l0_output) = try_build_advanced_weave(n_sentence, profile, dictionary) {
        return l0_output;
    }

    // --- L1: Simple Hybrid Fallback (This logic runs only if the L0 attempt fails) ---
    let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut l1_part_choices: Vec<L1PartChoice> = Vec::new();
    let mut l1_total_english_word_count: usize = 0;

    for l3_segment in &n_sentence.sims_l3_segments_numerical {
        let segment_id = &l3_segment.id_str;
        let segment_lemmas = n_sentence.l3_simsl_per_segment_numerical.iter().find(|sl| &sl.segment_id_str == segment_id);

        if segment_lemmas.map_or(false, |l| are_lemmas_active(&l.lemma_ids, profile, dictionary)) {
            l1_part_choices.push(L1PartChoice::Spanish(l3_segment.text_original.clone()));
            if let Some(lemmas) = segment_lemmas {
                l1_collected_lemma_ids.extend(&lemmas.lemma_ids);
            }
        } else {
            if let Some(alignment) = n_sentence.phrase_alignments_l3_to_eng_numerical.iter().find(|pa| &pa.s_segment_id_str == segment_id) {
                let mut final_parts: Vec<String> = Vec::new();
                let mut substitutions_made = 0;
                let diglot_map_opt = n_sentence.diglot_map_numerical.iter().find(|dm| &dm.s_segment_id_str == segment_id);

                for (i, word_token) in alignment.eng_span_words.iter().enumerate() {
                    final_parts.push(alignment.eng_span_backgrounds[i].clone());
                    let mut substituted = false;
                    
                    if let Some(diglot_map) = diglot_map_opt {
                        if let Some(entry) = diglot_map.entries.get(word_token.diglot_index) {
                            // *** L1 METADATA LOGIC (PROPER_NOUN & NO_SUB) ***
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
            } else {
                l1_part_choices.push(L1PartChoice::Spanish(l3_segment.text_original.clone()));
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
//*** END FILE: src/simulation/core_algo.rs ***//