// src/simulation/core_algo.rs
use super::numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;

/// Helper function to check if a slice of lemma IDs are all known to the learner.
fn are_lemmas_active(
    lemma_ids: &[u32],
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
) -> bool {
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
    InverseDiglot(String), // Simplified to just hold the final text
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

pub fn determine_and_annotate_sentence_expression(
    n_sentence: &mut NumericalProcessedSentence,
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
    _inverse_diglot_threshold: f32,
) -> ChosenLevelOutput {
    let mut l0_candidate_choices: Vec<L0SegmentChoice> = Vec::new();
    let mut l0_is_viable = true;
    let mut l0_collected_lemma_ids: Vec<u32> = Vec::new();

    if !n_sentence.adv_segment_bundles_numerical.is_empty() {
        for bundle in &n_sentence.adv_segment_bundles_numerical {
            if are_lemmas_active(&bundle.adv_lemma_ids, profile, dictionary) {
                l0_candidate_choices.push(L0SegmentChoice::Adv(bundle.adv_text_original.clone()));
                l0_collected_lemma_ids.extend(&bundle.adv_lemma_ids);
            } else if are_lemmas_active(&bundle.simpler_lemma_ids, profile, dictionary) {
                l0_candidate_choices
                    .push(L0SegmentChoice::SimplerAdv(bundle.simpler_text_original.clone()));
                l0_collected_lemma_ids.extend(&bundle.simpler_lemma_ids);
            } else {
                let mut final_parts: Vec<String> = Vec::new();
                let mut temp_collected_lemmas: Vec<u32> = Vec::new();
                let mut substitutions_made = 0;

                for (i, word_token) in bundle.simpler_text_words.iter().enumerate() {
                    final_parts.push(bundle.simpler_text_backgrounds[i].clone());
                    let (_word, lemma_id, eng_sub) = &bundle.inverse_diglot_map_numerical[word_token.diglot_index];

                    if profile.is_lemma_active(*lemma_id) || eng_sub == "PROPER_NOUN" {
                        temp_collected_lemmas.push(*lemma_id);
                        final_parts.push(word_token.text.clone());
                    } else if eng_sub == "NO_SUB" {
                        l0_is_viable = false; // A NO_SUB for an unknown word fails the weave.
                        break;
                    }
                    else {
                        substitutions_made += 1;
                        final_parts.push(eng_sub.clone());
                    }
                }
                if !l0_is_viable { break; }
                final_parts.push(bundle.simpler_text_backgrounds.last().unwrap().clone());

                let total_words = bundle.simpler_text_words.len();
                let substitution_limit_exceeded = if total_words > 1 {
                    (substitutions_made as f32 / total_words as f32) > 0.5
                } else {
                    false
                };

                if substitution_limit_exceeded || !are_lemmas_active(&temp_collected_lemmas, profile, dictionary) {
                    l0_is_viable = false;
                    break;
                } else {
                    l0_candidate_choices.push(L0SegmentChoice::InverseDiglot(final_parts.join("")));
                    l0_collected_lemma_ids.extend(temp_collected_lemmas);
                }
            }
        }

        if l0_is_viable {
            return ChosenLevelOutput {
                level: OutputLevel::AdvancedWeave,
                lemma_ids: l0_collected_lemma_ids,
                english_word_count: 0,
                l0_segment_choices: Some(l0_candidate_choices),
                l1_part_choices: None,
            };
        }
    }
// --- L1 FALLBACK LOGIC (Version 4 - With Debug Prints) ---
let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
let mut l1_part_choices: Vec<L1PartChoice> = Vec::new();
let mut l1_total_english_word_count: usize = 0;


// Iterate through all the L3 segments that make up the sentence.
for l3_segment in &n_sentence.sims_l3_segments_numerical {
    let segment_id = &l3_segment.id_str;

    let segment_lemmas = n_sentence
        .l3_simsl_per_segment_numerical
        .iter()
        .find(|sl| &sl.segment_id_str == segment_id);

    let ss_is_active = segment_lemmas
        .map_or(false, |lemmas| are_lemmas_active(&lemmas.lemma_ids, profile, dictionary));
    

    if ss_is_active {
        // Case 1: The simple Spanish version of this segment is fully known.
        l1_part_choices.push(L1PartChoice::Spanish(l3_segment.text_original.clone()));
        if let Some(lemmas) = segment_lemmas {
            l1_collected_lemma_ids.extend(&lemmas.lemma_ids);
        }
        continue; // Move to the next segment
    }

    // --- Case 2: Spanish is not fully known. Fallback to English/Diglot. ---
    if let Some(alignment) = n_sentence.phrase_alignments_l3_to_eng_numerical.iter().find(|pa| &pa.s_segment_id_str == segment_id) {
        
        let mut final_parts: Vec<String> = Vec::new();
        let mut substitutions_made = 0;

        if let Some(diglot_map) = n_sentence.diglot_map_numerical.iter().find(|dm| &dm.s_segment_id_str == segment_id) {
            for (i, word_token) in alignment.eng_span_words.iter().enumerate() {
                final_parts.push(alignment.eng_span_backgrounds[i].clone());
                let diglot_entry = diglot_map.entries.get(word_token.diglot_index)
        .unwrap_or_else(|| panic!(
            "\n\nFATAL DATA INTEGRITY ERROR in book '{}', sentence '{}', segment '{}':\n\
             Preprocessor created a WordToken for '{}' with diglot_index '{}', \
             but the diglot_map for this segment only has a length of {}.\n\
             This means the number of words found by the Rust tokenizer does not match the number of diglot map entries from the Python pipeline.\n\
             Please check the JSON data for this sentence.\n",
            n_sentence.source_file_name_original, // Assuming you add this field to NumericalProcessedSentence
            n_sentence.sentence_id_str,
            alignment.s_segment_id_str,
            word_token.text,
            word_token.diglot_index,
            diglot_map.entries.len()
        ));
                let spa_form = &diglot_entry.exact_spa_form_original;
                let should_substitute = diglot_entry.viable
                    && profile.is_lemma_active(diglot_entry.spa_lemma_id)
                    && spa_form != "PROPER_NOUN"
                    && spa_form != "NO_SUB";
                if should_substitute {
                    substitutions_made += 1;
                    l1_collected_lemma_ids.push(diglot_entry.spa_lemma_id);
                    let is_capitalized = word_token.text.chars().next().map_or(false, |c| c.is_uppercase());
                    if is_capitalized && !spa_form.is_empty() {
                        let mut c = spa_form.chars();
                        final_parts.push(c.next().unwrap().to_uppercase().to_string() + c.as_str());
                    } else {
                        final_parts.push(spa_form.clone());
                    }
                } else {
                    final_parts.push(word_token.text.clone());
                }
            }
            final_parts.push(alignment.eng_span_backgrounds.last().unwrap().clone());
        } else {
            for (i, word_token) in alignment.eng_span_words.iter().enumerate() {
                final_parts.push(alignment.eng_span_backgrounds[i].clone());
                final_parts.push(word_token.text.clone());
            }
            final_parts.push(alignment.eng_span_backgrounds.last().unwrap().clone());
        }
        
        let woven_text = final_parts.join("");
        l1_total_english_word_count += alignment.eng_span_words.len() - substitutions_made;

        if substitutions_made > 0 {
            l1_part_choices.push(L1PartChoice::Woven(woven_text, true));
        } else {
            l1_part_choices.push(L1PartChoice::English(alignment.eng_span_text_original.clone()));
        }
    } else {
        l1_part_choices.push(L1PartChoice::Spanish(l3_segment.text_original.clone()));
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