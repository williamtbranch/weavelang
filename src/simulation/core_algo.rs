use super::numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use regex::{Captures, Regex};
use once_cell::sync::Lazy;

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

static WORD_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[\w'-]+\b").unwrap());

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
                    let mut substitution_count = 0;
                    let total_words = bundle.simpler_text_original.split_whitespace().count();
                    let mut temp_collected_lemmas = Vec::new();
                    let mut map_iter = inverse_map_entries.iter();

                    let final_text = WORD_REGEX.replace_all(&bundle.simpler_text_original, |caps: &Captures| {
                        if let Some((_word, lemma_id, eng_sub)) = map_iter.next() {
                            if profile.is_lemma_active(*lemma_id) || eng_sub == "PROPER_NOUN" {
                                temp_collected_lemmas.push(*lemma_id);
                                caps[0].to_string() // Keep original Spanish word
                            } else {
                                substitution_count += 1;
                                eng_sub.clone() // Substitute with English
                            }
                        } else {
                            caps[0].to_string() // Fallback if map is exhausted
                        }
                    }).to_string();

                    let substitution_limit_exceeded = if total_words > 1 {
                        (substitution_count as f32 / total_words as f32) > 0.5
                    } else {
                        false
                    };

                    if substitution_limit_exceeded {
                        l0_is_viable = false;
                        break;
                    } else {
                        l0_candidate_choices.push(L0SegmentChoice::InverseDiglot { final_words: vec![final_text] });
                        l0_collected_lemma_ids.extend(temp_collected_lemmas);
                    }
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

    // --- L1 FALLBACK LOGIC ---
    let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut l1_part_choices: Vec<L1PartChoice> = Vec::new();
    let mut l1_english_word_count: usize = 0;

    for s_seg_data_num in &n_sentence.sims_l3_segments_numerical {
        let seg_lemmas_obj_num = n_sentence.l3_simsl_per_segment_numerical.iter().find(|sl_num| sl_num.segment_id_str == s_seg_data_num.id_str);

        let ss_is_active = seg_lemmas_obj_num.map_or(false, |lemmas| are_lemmas_active(&lemmas.lemma_ids, profile, dictionary));

        if ss_is_active {
            l1_part_choices.push(L1PartChoice::Spanish(s_seg_data_num.text_original.clone()));
            if let Some(lemmas) = seg_lemmas_obj_num { l1_collected_lemma_ids.extend(&lemmas.lemma_ids); }
        } else {
            if let Some(alignment) = n_sentence.phrase_alignments_l3_to_eng_numerical.iter().find(|pa| pa.s_segment_id_str == s_seg_data_num.id_str) {
                if let Some(diglot_map) = n_sentence.diglot_map_numerical.iter().find(|dm| dm.s_segment_id_str == s_seg_data_num.id_str) {
                    let mut diglot_iter = diglot_map.entries.iter();
                    let mut substitutions_made = 0;
                    
                    let woven_text = WORD_REGEX.replace_all(&alignment.eng_span_text_original, |caps: &Captures| {
                        if let Some(diglot_entry) = diglot_iter.next() {
                            let spa_form = &diglot_entry.exact_spa_form_original;
                            
                            // *** FINAL FIX: Add resilience against bad data. ***
                            // A substitution is only valid if all conditions are met.
                            let should_substitute = diglot_entry.viable
                                && profile.is_lemma_active(diglot_entry.spa_lemma_id)
                                && spa_form != "PROPER_NOUN"
                                && spa_form != "NO_SUB";

                            if should_substitute {
                                let is_capitalized = caps[0].chars().next().map_or(false, |c| c.is_uppercase());
                                substitutions_made += 1;
                                l1_collected_lemma_ids.push(diglot_entry.spa_lemma_id);
                                if is_capitalized && !spa_form.is_empty() {
                                    let mut c = spa_form.chars();
                                    c.next().unwrap().to_uppercase().to_string() + c.as_str()
                                } else {
                                    spa_form.clone()
                                }
                            } else {
                                l1_english_word_count += 1;
                                caps[0].to_string()
                            }
                        } else {
                            l1_english_word_count += 1;
                            caps[0].to_string()
                        }
                    }).to_string();

                    if substitutions_made > 0 {
                        let contains_english = substitutions_made < alignment.eng_span_word_count;
                        l1_part_choices.push(L1PartChoice::Woven(woven_text, contains_english));
                    } else {
                        // If no substitutions happened, the word count is just the original span's word count.
                        l1_english_word_count = alignment.eng_span_word_count;
                        l1_part_choices.push(L1PartChoice::English(alignment.eng_span_text_original.clone()));
                    }
                } else {
                    l1_english_word_count += alignment.eng_span_word_count;
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