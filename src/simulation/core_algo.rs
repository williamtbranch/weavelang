// src/simulation/core_algo.rs
use super::dictionary::GlobalLemmaDictionary;
use super::numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence};
use crate::profile::LemmaState;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputLevel {
    L0, // AdvS
    L1, // Woven AdvS / SimplerAdvS
    L2, // Simpler AdvS Full
    L3, // Simple Spanish L3 Full
    L4, // Woven SimS L3 / English
    L5, // Diglot
    L6, // English
}

// --- Detailed information for text construction ---
#[derive(Debug, Clone)]
pub enum L1SegmentChoice {
    Adv(String), // The advanced segment text
    SimplerAdv(String), // The simpler advanced segment text
}

#[derive(Debug, Clone)]
pub enum L4PartChoice {
    // The segment is fully known in simple Spanish.
    Spanish(String),
    // The segment is not fully known, but we found a viable diglot word to substitute.
    Hybrid {
        base_english_phrase: String,
        substitution: L5Substitution,
    },
    // The segment is not fully known, and no viable diglot words were found either.
    English(String),
}

#[derive(Debug, Clone)]
pub struct L5Substitution {
    // The English word from the original phrase that will be replaced.
    pub eng_word_to_replace: String,
    // The exact Spanish form (e.g., "la", "abrió") that will replace it.
    pub spa_form_to_insert: String,
}

#[derive(Debug, Clone)]
pub struct ChosenLevelOutput {
    pub level: OutputLevel,
    pub lemma_ids: Vec<u32>, // Spanish lemma IDs used in the chosen output level
    pub l1_segment_choices: Option<Vec<L1SegmentChoice>>,
    pub l4_part_choices: Option<Vec<L4PartChoice>>,
    pub l5_substitutions: Option<Vec<L5Substitution>>,
}

#[derive(Debug, Clone)]
pub struct SimulationBlockResult {
    pub profile_state_for_text_generation: NumericalLearnerProfile,
    pub profile_state_after_block_exposure: NumericalLearnerProfile,
    pub chosen_level_outputs_for_sentences: Vec<ChosenLevelOutput>,
    pub simulation_log_entries: Vec<String>,
    pub final_ct_for_block: f32,
    pub known_and_active_lemmas_in_block: usize,
    pub total_spanish_lemmas_in_block: usize,
    pub activated_lemma_ids_this_block_run: HashSet<u32>,
    pub level_stats: HashMap<OutputLevel, usize>,
}

fn determine_output_for_sentence(
    n_sentence: &NumericalProcessedSentence,
    profile: &NumericalLearnerProfile,
) -> ChosenLevelOutput {
    // --- L0: Full Advanced Spanish ---
    if !n_sentence.adv_sl_overall_lemma_ids.is_empty()
        && n_sentence
            .adv_sl_overall_lemma_ids
            .iter()
            .all(|&id| profile.is_lemma_known_or_active(id))
    {
        return ChosenLevelOutput {
            level: OutputLevel::L0,
            lemma_ids: n_sentence.adv_sl_overall_lemma_ids.clone(),
            l1_segment_choices: None, l4_part_choices: None, l5_substitutions: None,
        };
    }

    // --- L1: Woven AdvS / SimplerAdvS ---
    if !n_sentence.adv_segment_bundles_numerical.is_empty() {
        let mut l1_collected_lemma_ids: Vec<u32> = Vec::new();
        let mut l1_text_segment_choices: Vec<L1SegmentChoice> = Vec::new();
        let mut l1_can_be_fully_constructed = true;
        let mut l1_produced_any_spanish = false;

        for bundle in &n_sentence.adv_segment_bundles_numerical {
            let adv_part_viable =
                bundle.adv_lemma_ids.is_empty() || bundle.adv_lemma_ids.iter().all(|&id| profile.is_lemma_known_or_active(id));

            if adv_part_viable {
                l1_text_segment_choices.push(L1SegmentChoice::Adv(bundle.adv_text_original.clone()));
                l1_collected_lemma_ids.extend(&bundle.adv_lemma_ids);
                if !bundle.adv_lemma_ids.is_empty() {
                    l1_produced_any_spanish = true;
                }
            } else {
                let simpler_part_viable = bundle.simpler_lemma_ids.is_empty() || bundle.simpler_lemma_ids.iter().all(|&id| profile.is_lemma_known_or_active(id));
                if simpler_part_viable {
                    l1_text_segment_choices.push(L1SegmentChoice::SimplerAdv(bundle.simpler_text_original.clone()));
                    l1_collected_lemma_ids.extend(&bundle.simpler_lemma_ids);
                    if !bundle.simpler_lemma_ids.is_empty() {
                        l1_produced_any_spanish = true;
                    }
                } else {
                    l1_can_be_fully_constructed = false;
                    break;
                }
            }
        }

        if l1_can_be_fully_constructed && l1_produced_any_spanish {
            return ChosenLevelOutput {
                level: OutputLevel::L1,
                lemma_ids: l1_collected_lemma_ids,
                l1_segment_choices: Some(l1_text_segment_choices),
                l4_part_choices: None, l5_substitutions: None,
            };
        }
    }

    // --- L2: Full Simpler Advanced Spanish ---
    if !n_sentence.simpler_adv_sl_overall_lemma_ids.is_empty() && n_sentence
            .simpler_adv_sl_overall_lemma_ids
            .iter()
            .all(|&id| profile.is_lemma_known_or_active(id))
    {
        return ChosenLevelOutput {
            level: OutputLevel::L2,
            lemma_ids: n_sentence.simpler_adv_sl_overall_lemma_ids.clone(),
            l1_segment_choices: None, l4_part_choices: None, l5_substitutions: None,
        };
    }
    
    // --- L3: Full Simple Spanish (L3 structure) ---
    if !n_sentence.l3_sim_sl_overall_lemma_ids.is_empty() && n_sentence
            .l3_sim_sl_overall_lemma_ids
            .iter()
            .all(|&id| profile.is_lemma_known_or_active(id))
    {
        return ChosenLevelOutput {
            level: OutputLevel::L3,
            lemma_ids: n_sentence.l3_sim_sl_overall_lemma_ids.clone(),
            l1_segment_choices: None, l4_part_choices: None, l5_substitutions: None,
        };
    }

    // --- L4: RECURSIVE/HYBRID Woven SimS_L3 / English ---
    if !n_sentence.sims_l3_segments_numerical.is_empty() {
        let mut l4_collected_lemma_ids: Vec<u32> = Vec::new();
        let mut l4_part_choices: Vec<L4PartChoice> = Vec::new();
        let mut l4_can_be_fully_constructed = true;
        let mut l4_produced_any_spanish = false;

        for s_seg_data_num in &n_sentence.sims_l3_segments_numerical {
            if let Some(seg_lemmas_obj_num) = n_sentence
                .l3_simsl_per_segment_numerical
                .iter()
                .find(|sl_num| sl_num.segment_id_str == s_seg_data_num.id_str)
            {
                let use_spanish_part = seg_lemmas_obj_num.lemma_ids.is_empty() || seg_lemmas_obj_num.lemma_ids.iter().all(|&id| profile.is_lemma_known_or_active(id));
                
                if use_spanish_part {
                    l4_part_choices.push(L4PartChoice::Spanish(s_seg_data_num.text_original.clone()));
                    l4_collected_lemma_ids.extend(&seg_lemmas_obj_num.lemma_ids);
                    if !seg_lemmas_obj_num.lemma_ids.is_empty() {
                        l4_produced_any_spanish = true;
                    }
                } else {
                    if let Some(alignment) = n_sentence
                        .phrase_alignments_l3_to_eng_numerical
                        .iter()
                        .find(|pa| pa.s_segment_id_str == s_seg_data_num.id_str)
                    {
                        let mut substitution_found: Option<L5Substitution> = None;
                        if let Some(diglot_map_for_segment) = n_sentence
                            .diglot_map_numerical
                            .iter()
                            .find(|dm| dm.s_segment_id_str == s_seg_data_num.id_str)
                        {
                            for entry in &diglot_map_for_segment.entries {
                                if entry.viable && profile.is_lemma_known_or_active(entry.spa_lemma_id) {
                                    substitution_found = Some(L5Substitution {
                                        eng_word_to_replace: entry.eng_word_original.clone(),
                                        spa_form_to_insert: entry.exact_spa_form_original.clone(),
                                    });
                                    l4_collected_lemma_ids.push(entry.spa_lemma_id);
                                    l4_produced_any_spanish = true;
                                    break;
                                }
                            }
                        }

                        if let Some(sub) = substitution_found {
                            l4_part_choices.push(L4PartChoice::Hybrid {
                                base_english_phrase: alignment.eng_span_text_original.clone(),
                                substitution: sub,
                            });
                        } else {
                            l4_part_choices.push(L4PartChoice::English(
                                alignment.eng_span_text_original.clone(),
                            ));
                        }
                    } else {
                        l4_can_be_fully_constructed = false;
                        break;
                    }
                }
            } else {
                l4_can_be_fully_constructed = false;
                break;
            }
        }

        if l4_can_be_fully_constructed && l4_produced_any_spanish {
            return ChosenLevelOutput {
                level: OutputLevel::L4,
                lemma_ids: l4_collected_lemma_ids,
                l1_segment_choices: None,
                l4_part_choices: Some(l4_part_choices),
                l5_substitutions: None,
            };
        }
    }

    // --- L5: Diglot (one word per sentence) - This is now simpler. ---
    if !n_sentence.diglot_map_numerical.is_empty() {
        let mut l5_collected_lemma_ids: Vec<u32> = Vec::new();
        let mut l5_actual_substitutions: Vec<L5Substitution> = Vec::new();

        'outer: for seg_map_num in &n_sentence.diglot_map_numerical {
            for entry_num in &seg_map_num.entries {
                if entry_num.viable && profile.is_lemma_known_or_active(entry_num.spa_lemma_id) {
                    l5_collected_lemma_ids.push(entry_num.spa_lemma_id);
                    l5_actual_substitutions.push(L5Substitution {
                        eng_word_to_replace: entry_num.eng_word_original.clone(),
                        spa_form_to_insert: entry_num.exact_spa_form_original.clone(),
                    });
                    break 'outer; 
                }
            }
        }

        if !l5_actual_substitutions.is_empty() {
            return ChosenLevelOutput {
                level: OutputLevel::L5,
                lemma_ids: l5_collected_lemma_ids,
                l1_segment_choices: None, 
                l4_part_choices: None,
                l5_substitutions: Some(l5_actual_substitutions),
            };
        }
    }

    // L6: Full Eng (ultimate fallback)
    ChosenLevelOutput {
        level: OutputLevel::L6,
        lemma_ids: Vec::new(),
        l1_segment_choices: None, l4_part_choices: None, l5_substitutions: None,
    }
}

// NOTE: The run_simulation_numerical function starts below.
// This block provides everything *above* it in the file.
pub fn run_simulation_numerical(
    block_sentences_numerical: &[&NumericalProcessedSentence],
    initial_profile_for_block_run: NumericalLearnerProfile,
    dictionary: &GlobalLemmaDictionary,
    available_new_lemma_ids_for_activation: &[(u32, u32)],
    max_regeneration_attempts_per_block: u32,
    target_ct_comprehensible_threshold: f32,
    max_words_to_activate_per_regen_attempt: usize,
) -> Result<SimulationBlockResult, String> {
    let mut simulation_log_entries: Vec<String> = vec![format!(
        "Core Algo: Block of {} sentences. Max regen: {}. Target CT: {:.2}%. Initial Profile K: {}, A: {}",
        block_sentences_numerical.len(), max_regeneration_attempts_per_block, target_ct_comprehensible_threshold * 100.0,
        initial_profile_for_block_run.count_known(), initial_profile_for_block_run.count_active_only()
    )];

    let mut profile_being_refined_for_block = initial_profile_for_block_run.clone();
    let mut activated_ids_in_this_refinement_cycle: HashSet<u32> = HashSet::new();

    for regen_attempt in 1..=max_regeneration_attempts_per_block {
        simulation_log_entries.push(format!("  Regen Attempt: {}", regen_attempt));
        let profile_for_this_pass = profile_being_refined_for_block.clone();
        
        let mut current_chosen_outputs_for_block: Vec<ChosenLevelOutput> = Vec::new();
        let mut all_lemma_ids_for_pass_output: Vec<u32> = Vec::new();
        let mut level_stats_for_pass: HashMap<OutputLevel, usize> = HashMap::new();

        for n_sentence_ref in block_sentences_numerical.iter() {
            let chosen_output = determine_output_for_sentence(n_sentence_ref, &profile_for_this_pass);
            *level_stats_for_pass.entry(chosen_output.level.clone()).or_insert(0) += 1;
            all_lemma_ids_for_pass_output.extend(&chosen_output.lemma_ids);
            current_chosen_outputs_for_block.push(chosen_output);
        }

        let total_spanish_lemmas_this_pass = all_lemma_ids_for_pass_output.len();
        
        // --- THIS IS THE CORRECTED CT CALCULATION ---
        let known_and_active_lemmas_this_pass = all_lemma_ids_for_pass_output.iter()
            .filter(|&&id| profile_for_this_pass.is_lemma_known_or_active(id))
            .count();
        
        let actual_ct_this_pass = if total_spanish_lemmas_this_pass > 0 {
            known_and_active_lemmas_this_pass as f32 / total_spanish_lemmas_this_pass as f32
        } else { 1.0 }; 

        simulation_log_entries.push(format!(
            "    Pass CT (K+A): {:.2}% ({}K+A / {}Total). Profile for pass: K={}, A={}",
            actual_ct_this_pass * 100.0, known_and_active_lemmas_this_pass, total_spanish_lemmas_this_pass,
            profile_for_this_pass.count_known(), profile_for_this_pass.count_active_only()
        ));

        let block_is_too_easy = actual_ct_this_pass >= target_ct_comprehensible_threshold;
        let is_final_regen_attempt = regen_attempt == max_regeneration_attempts_per_block;
        let no_more_new_words_to_activate_from_source = available_new_lemma_ids_for_activation.iter()
            .all(|(id, _)| !profile_being_refined_for_block.get_lemma_info(*id).map_or(true, |info| info.state == LemmaState::New));

        if !block_is_too_easy || is_final_regen_attempt || no_more_new_words_to_activate_from_source {
            let mut reason_msg = "    Finalizing block: ".to_string();
            if !block_is_too_easy { reason_msg.push_str("CT acceptable. "); }
            if is_final_regen_attempt { reason_msg.push_str("Max regen attempts reached. "); }
            if no_more_new_words_to_activate_from_source { reason_msg.push_str("No new words to activate. "); }
            simulation_log_entries.push(reason_msg);
            
            let mut profile_after_final_exposure = profile_for_this_pass.clone();
            profile_after_final_exposure.record_exposures(&all_lemma_ids_for_pass_output, dictionary); 
            
            return Ok(SimulationBlockResult {
                profile_state_for_text_generation: profile_for_this_pass,
                profile_state_after_block_exposure: profile_after_final_exposure,
                chosen_level_outputs_for_sentences: current_chosen_outputs_for_block,
                simulation_log_entries,
                final_ct_for_block: actual_ct_this_pass,
                known_and_active_lemmas_in_block: known_and_active_lemmas_this_pass,
                total_spanish_lemmas_in_block: total_spanish_lemmas_this_pass,
                activated_lemma_ids_this_block_run: activated_ids_in_this_refinement_cycle,
                level_stats: level_stats_for_pass,
            });
        } else { 
            simulation_log_entries.push("    Activation Triggered.".to_string());
            let mut words_activated_this_attempt = 0;
            for (lemma_id, freq) in available_new_lemma_ids_for_activation.iter() {
                if profile_being_refined_for_block.get_lemma_info(*lemma_id).map_or(true, |info| info.state == LemmaState::New) {
                    profile_being_refined_for_block.set_lemma_state(*lemma_id, LemmaState::Active);
                    activated_ids_in_this_refinement_cycle.insert(*lemma_id);
                    simulation_log_entries.push(format!("      Activated Lemma ID: {} (Freq: {})", lemma_id, freq));
                    words_activated_this_attempt += 1;
                    if words_activated_this_attempt >= max_words_to_activate_per_regen_attempt { break; }
                }
            }
        }
    }
    Err("Core algo: Max regen attempts loop finished unexpectedly.".to_string())
}