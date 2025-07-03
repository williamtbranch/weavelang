// src/simulation/core_algo.rs
use super::numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager; // <-- NEW IMPORT
use crate::simulation::global_settings::{ForceLevel, FORCE_LEVEL_OVERRIDE};
use std::collections::HashMap;

// --- Helper function to check if a phrase is comprehensible ---
fn are_lemmas_active(
    lemma_ids: &[u32],
    profile: &NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
) -> bool {
    lemma_ids.iter().all(|&id| {
        // A lemma is "known" if it's in the profile...
        if profile.is_lemma_active(id) {
            return true;
        }
        // ...OR if it's not a "learnable" word (e.g., a number, proper noun).
        // We determine this by checking if it exists in our master frequency list.
        if let Some(lemma_str) = dictionary.get_str(id) {
            if frequency_manager::get_rank_for_lemma(lemma_str).is_none() {
                return true; // Not in frequency list, so it's a "free" word.
            }
        }
        // Otherwise, it's a learnable word that is not yet active.
        false
    })
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OutputLevel {
    AdvancedWeave,
    SimpleHybrid,
}

#[derive(Debug, Clone)]
pub enum L0SegmentChoice {
    Adv(String),
    SimplerAdv(String),
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
) -> ChosenLevelOutput {
    if let Some(forced_level) = FORCE_LEVEL_OVERRIDE.get() {
        if *forced_level == ForceLevel::Advanced {
            let l0_segment_choices: Vec<L0SegmentChoice> = n_sentence
                .adv_segment_bundles_numerical
                .iter()
                .map(|bundle| L0SegmentChoice::Adv(bundle.adv_text_original.clone()))
                .collect();

            return ChosenLevelOutput {
                level: OutputLevel::AdvancedWeave,
                lemma_ids: n_sentence.adv_sl_overall_lemma_ids.clone(),
                english_word_count: 0,
                l0_segment_choices: Some(l0_segment_choices),
                l1_part_choices: None,
            };
        }
    }

    // L0: Advanced Weave Attempt
    let mut l0_can_be_fully_constructed = true;
    if !n_sentence.adv_segment_bundles_numerical.is_empty() {
        let mut l0_collected_lemma_ids: Vec<u32> = Vec::new();
        let mut l0_segment_choices: Vec<L0SegmentChoice> = Vec::new();

        for bundle in &n_sentence.adv_segment_bundles_numerical {
            // --- USE NEW HELPER FUNCTION ---
            if are_lemmas_active(&bundle.adv_lemma_ids, profile, dictionary) {
                l0_segment_choices.push(L0SegmentChoice::Adv(bundle.adv_text_original.clone()));
                l0_collected_lemma_ids.extend(&bundle.adv_lemma_ids);
            } else if are_lemmas_active(&bundle.simpler_lemma_ids, profile, dictionary) {
                l0_segment_choices.push(L0SegmentChoice::SimplerAdv(bundle.simpler_text_original.clone()));
                l0_collected_lemma_ids.extend(&bundle.simpler_lemma_ids);
            } else {
                l0_can_be_fully_constructed = false;
                break;
            }
        }

        if l0_can_be_fully_constructed {
            return ChosenLevelOutput {
                level: OutputLevel::AdvancedWeave,
                lemma_ids: l0_collected_lemma_ids,
                english_word_count: 0,
                l0_segment_choices: Some(l0_segment_choices),
                l1_part_choices: None,
            };
        }
    }

    // L1: Simple Hybrid (The Fallback)
    let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut l1_part_choices: Vec<L1PartChoice> = Vec::new();
    let mut l1_english_word_count: usize = 0;

    for s_seg_data_num in &n_sentence.sims_l3_segments_numerical {
        let seg_lemmas_obj_num = n_sentence
            .l3_simsl_per_segment_numerical
            .iter()
            .find(|sl_num| sl_num.segment_id_str == s_seg_data_num.id_str);

        // --- USE NEW HELPER FUNCTION ---
        let ss_is_active = seg_lemmas_obj_num.map_or(false, |lemmas| {
            are_lemmas_active(&lemmas.lemma_ids, profile, dictionary)
        });

        if ss_is_active {
            l1_part_choices.push(L1PartChoice::Spanish(s_seg_data_num.text_original.clone()));
            if let Some(lemmas) = seg_lemmas_obj_num {
                l1_collected_lemma_ids.extend(&lemmas.lemma_ids);
            }
        } else {
            if let Some(alignment) = n_sentence.phrase_alignments_l3_to_eng_numerical.iter().find(|pa| pa.s_segment_id_str == s_seg_data_num.id_str) {
                
                let mut woven_phrase_parts: Vec<String> = Vec::new();
                let mut substitutions_made = 0;

                if let Some(diglot_map) = n_sentence.diglot_map_numerical.iter().find(|dm| dm.s_segment_id_str == s_seg_data_num.id_str) {
                    let diglot_lookup: HashMap<_, _> = diglot_map.entries.iter().map(|e| (e.eng_word_original.to_lowercase(), e)).collect();
                    
                    for eng_word in alignment.eng_span_text_original.split_whitespace() {
                        let lower_eng_word = eng_word.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
                        let mut substituted = false;
                        
                        if let Some(diglot_entry) = diglot_lookup.get(&lower_eng_word) {
                            if diglot_entry.viable && profile.is_lemma_active(diglot_entry.spa_lemma_id) {
                                let is_capitalized = eng_word.chars().next().map_or(false, |c| c.is_uppercase());
                                let spa_form = &diglot_entry.exact_spa_form_original;
                                let final_form = if is_capitalized && !spa_form.is_empty() {
                                    let mut c = spa_form.chars();
                                    c.next().unwrap().to_uppercase().to_string() + c.as_str()
                                } else {
                                    spa_form.clone()
                                };
                                woven_phrase_parts.push(final_form);
                                l1_collected_lemma_ids.push(diglot_entry.spa_lemma_id);
                                substituted = true;
                                substitutions_made += 1;
                            }
                        }

                        if !substituted {
                            woven_phrase_parts.push(eng_word.to_string());
                            l1_english_word_count += 1;
                        }
                    }
                } else {
                    woven_phrase_parts.push(alignment.eng_span_text_original.clone());
                    l1_english_word_count += alignment.eng_span_word_count;
                }

                if substitutions_made > 0 {
                    let contains_english = substitutions_made < alignment.eng_span_text_original.split_whitespace().count();
                    l1_part_choices.push(L1PartChoice::Woven(woven_phrase_parts.join(" "), contains_english));
                } else {
                    l1_part_choices.push(L1PartChoice::English(alignment.eng_span_text_original.clone()));
                }
            } else {
                l1_part_choices.push(L1PartChoice::Spanish(s_seg_data_num.text_original.clone()));
            }
        }
    }

    ChosenLevelOutput {
        level: OutputLevel::SimpleHybrid,
        lemma_ids: l1_collected_lemma_ids,
        english_word_count: l1_english_word_count,
        l0_segment_choices: None,
        l1_part_choices: Some(l1_part_choices),
    }
}