use super::numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use std::collections::HashMap;

/// Helper function to check if a slice of lemma IDs are all known to the learner.
fn are_lemmas_active(
    lemma_ids: &[u32],
    profile: &NumericalLearnerProfile,
    _dictionary: &GlobalLemmaDictionary, // Signature kept for consistency
) -> bool {
    lemma_ids.iter().all(|&id| profile.is_lemma_active(id))
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
    InverseDiglot {
        final_words: Vec<String>,
    },
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
                l0_candidate_choices.push(L0SegmentChoice::SimplerAdv(bundle.simpler_text_original.clone()));
                l0_collected_lemma_ids.extend(&bundle.simpler_lemma_ids);
            } else {
                let mut is_segment_salvageable = true;
                let inverse_map_entries = &bundle.inverse_diglot_map_numerical;

                for (_spanish_word, lemma_id, substitute) in inverse_map_entries.iter() {
                    if substitute == "NO_SUB" && !profile.is_lemma_active(*lemma_id) {
                        is_segment_salvageable = false;
                        break;
                    }
                }

                if is_segment_salvageable {
                    // --- START OF MODIFIED LOGIC ---
                    let mut final_words = Vec::new();
                    let mut temp_collected_lemmas = Vec::new();
                    let mut substitution_count = 0;
                    let total_words = inverse_map_entries.len();

                    for (spanish_word, lemma_id, english_sub) in inverse_map_entries.iter() {
                        if profile.is_lemma_active(*lemma_id) || english_sub == "PROPER_NOUN" {
                            final_words.push(spanish_word.clone());
                            temp_collected_lemmas.push(*lemma_id);
                        } else {
                            final_words.push(english_sub.clone());
                            // This is an English substitution, count it.
                            substitution_count += 1;
                        }
                    }

                    // NEW: Apply the >50% substitution rule.
                    // A single-word phrase can always be substituted (1/1 = 100% is OK).
                    let substitution_limit_exceeded = if total_words > 1 {
                        // If substitution count is strictly greater than half the words, it fails.
                        (substitution_count as f32 / total_words as f32) > 0.5
                    } else {
                        false // Allow single-word segments to be substituted.
                    };

                    if substitution_limit_exceeded {
                        // Too many substitutions, fail this L0 path.
                        l0_is_viable = false;
                        break;
                    } else {
                        // The substitution count is acceptable, add the constructed segment.
                        l0_candidate_choices.push(L0SegmentChoice::InverseDiglot { final_words });
                        l0_collected_lemma_ids.extend(temp_collected_lemmas);
                    }
                    // --- END OF MODIFIED LOGIC ---
                } else {
                    l0_is_viable = false;
                    break;
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

    // --- L1 FALLBACK LOGIC (remains the same) ---
    let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut l1_part_choices: Vec<L1PartChoice> = Vec::new();
    let mut l1_english_word_count: usize = 0;

    for s_seg_data_num in &n_sentence.sims_l3_segments_numerical {
        let seg_lemmas_obj_num = n_sentence
            .l3_simsl_per_segment_numerical
            .iter()
            .find(|sl_num| sl_num.segment_id_str == s_seg_data_num.id_str);

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
                        let lower_eng_word = eng_word.to_lowercase();
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