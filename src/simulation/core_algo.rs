// src/simulation/core_algo.rs
use super::numerical_types::{
    NumericalLearnerProfile, NumericalProcessedSentence, PriceAndCost,
};
use crate::profile::LemmaState;
use crate::simulation::dictionary::GlobalLemmaDictionary;
use std::collections::HashSet;

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
    Hybrid {
        base_english_phrase: String,
        substitution: DiglotSubstitution,
    },
    English(String),
}

#[derive(Debug, Clone)]
pub struct DiglotSubstitution {
    pub eng_word_to_replace: String,
    pub spa_form_to_insert: String,
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
    _dictionary: &GlobalLemmaDictionary,
) -> ChosenLevelOutput {
    n_sentence.l0_upgrade_pc = Default::default();
    n_sentence.l1_segment_upgrade_pcs = Default::default();

    let mut overall_l0_pc = PriceAndCost::default();
    let mut l0_can_be_fully_constructed = true;

    if !n_sentence.adv_segment_bundles_numerical.is_empty() {
        let mut l0_collected_lemma_ids: Vec<u32> = Vec::new();
        let mut l0_segment_choices: Vec<L0SegmentChoice> = Vec::new();
        // --- FIX: The flawed flag is completely removed. ---

        for bundle in &n_sentence.adv_segment_bundles_numerical {
            let adv_inactive_lemmas: HashSet<u32> = bundle
                .adv_lemma_ids
                .iter()
                .filter(|&&id| profile.get_lemma_info(id).map_or(true, |i| i.state == LemmaState::New))
                .cloned()
                .collect();

            if adv_inactive_lemmas.is_empty() {
                l0_segment_choices.push(L0SegmentChoice::Adv(bundle.adv_text_original.clone()));
                l0_collected_lemma_ids.extend(&bundle.adv_lemma_ids);
                // --- FIX: The flawed check that used the flag is also removed. ---
            } else {
                let simpler_inactive_lemmas: HashSet<u32> = bundle
                    .simpler_lemma_ids
                    .iter()
                    .filter(|&&id| profile.get_lemma_info(id).map_or(true, |i| i.state == LemmaState::New))
                    .cloned()
                    .collect();

                if simpler_inactive_lemmas.is_empty() {
                    l0_segment_choices.push(L0SegmentChoice::SimplerAdv(bundle.simpler_text_original.clone()));
                    l0_collected_lemma_ids.extend(&bundle.simpler_lemma_ids);
                    
                    n_sentence.l1_segment_upgrade_pcs.insert(bundle.a_id_str.clone(), PriceAndCost {
                        price: adv_inactive_lemmas.len() as u32,
                        cost: adv_inactive_lemmas,
                    });
                } else {
                    l0_can_be_fully_constructed = false;
                    break;
                }
            }
        }

        // --- FIX: The final condition now only depends on whether the level could be built. ---
        if l0_can_be_fully_constructed {
            let min_price = n_sentence.l1_segment_upgrade_pcs.values().map(|pc| pc.price).min().unwrap_or(u32::MAX);
            if min_price != u32::MAX {
                overall_l0_pc.price = min_price;
                for pc in n_sentence.l1_segment_upgrade_pcs.values() {
                    if pc.price == min_price {
                        overall_l0_pc.cost.extend(&pc.cost);
                    }
                }
            }
            n_sentence.l0_upgrade_pc = overall_l0_pc;

            return ChosenLevelOutput {
                level: OutputLevel::AdvancedWeave,
                lemma_ids: l0_collected_lemma_ids,
                english_word_count: 0,
                l0_segment_choices: Some(l0_segment_choices),
                l1_part_choices: None,
            };
        }
    }

    // The rest of the function for Level 1 logic remains unchanged.
    let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
    let mut l1_part_choices: Vec<L1PartChoice> = Vec::new();
    let mut l1_english_word_count: usize = 0;

    for s_seg_data_num in &n_sentence.sims_l3_segments_numerical {
        let seg_lemmas_obj_num = n_sentence
            .l3_simsl_per_segment_numerical
            .iter()
            .find(|sl_num| sl_num.segment_id_str == s_seg_data_num.id_str);

        let inactive_ss_lemmas: HashSet<u32> = seg_lemmas_obj_num.map_or(HashSet::new(), |lemmas| {
            lemmas.lemma_ids.iter().filter(|&&id| profile.get_lemma_info(id).map_or(true, |i| i.state == LemmaState::New)).cloned().collect()
        });

        if inactive_ss_lemmas.is_empty() {
            l1_part_choices.push(L1PartChoice::Spanish(s_seg_data_num.text_original.clone()));
            if let Some(lemmas) = seg_lemmas_obj_num {
                l1_collected_lemma_ids.extend(&lemmas.lemma_ids);
            }
        } else {
            if let Some(alignment) = n_sentence.phrase_alignments_l3_to_eng_numerical.iter().find(|pa| pa.s_segment_id_str == s_seg_data_num.id_str) {
                n_sentence.l1_segment_upgrade_pcs.insert(s_seg_data_num.id_str.clone(), PriceAndCost {
                    price: inactive_ss_lemmas.len() as u32,
                    cost: inactive_ss_lemmas,
                });
                
                let mut substitution_found: Option<(DiglotSubstitution, u32)> = None;
                if let Some(diglot_map_for_segment) = n_sentence.diglot_map_numerical.iter().find(|dm| dm.s_segment_id_str == s_seg_data_num.id_str) {
                    let mut best_sub_candidate: Option<(u32, &str, &str, u32)> = None;
                    for entry in &diglot_map_for_segment.entries {
                        if entry.viable {
                            if let Some(info) = profile.get_lemma_info(entry.spa_lemma_id) {
                                if info.state == LemmaState::Active || info.state == LemmaState::Known {
                                    let current_exposure = info.exposure_count;
                                    if best_sub_candidate.is_none() || current_exposure < best_sub_candidate.unwrap().0 {
                                        best_sub_candidate = Some((current_exposure, &entry.eng_word_original, &entry.exact_spa_form_original, entry.spa_lemma_id));
                                    }
                                }
                            }
                        }
                    }
                    if let Some((_, eng_word, spa_form, lemma_id)) = best_sub_candidate {
                        substitution_found = Some((DiglotSubstitution {
                            eng_word_to_replace: eng_word.to_string(),
                            spa_form_to_insert: spa_form.to_string(),
                        }, lemma_id));
                    }
                }

                if let Some((sub, lemma_id)) = substitution_found {
                    l1_part_choices.push(L1PartChoice::Hybrid {
                        base_english_phrase: alignment.eng_span_text_original.clone(),
                        substitution: sub,
                    });
                    l1_english_word_count += alignment.eng_span_word_count.saturating_sub(1);
                    l1_collected_lemma_ids.push(lemma_id);
                } else {
                    l1_part_choices.push(L1PartChoice::English(alignment.eng_span_text_original.clone()));
                    l1_english_word_count += alignment.eng_span_word_count;
                }
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
