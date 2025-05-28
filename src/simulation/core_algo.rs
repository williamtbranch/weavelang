// Current src/simulation/core_algo.rs for context before modification

use super::numerical_types::{
    NumericalLearnerProfile,
    NumericalProcessedSentence, 
};
use crate::profile::LemmaState; 

#[derive(Debug, Clone)]
pub struct SimulationBlockResult {
    pub profile_state_for_text_generation: NumericalLearnerProfile,
    pub profile_state_after_block_exposure: NumericalLearnerProfile,
    pub output_lemma_ids_for_block: Vec<u32>, 
    pub simulation_log_entries: Vec<String>,
    pub final_ct_for_block: f32,
    pub known_lemmas_in_block: usize,
    pub total_spanish_lemmas_in_block: usize,
}

// THIS IS THE FUNCTION WE WILL REFINE:
fn determine_sentence_output_lemma_ids(
    n_sentence: &NumericalProcessedSentence,
    profile: &NumericalLearnerProfile,
) -> Vec<u32> {
    let mut sentence_output_ids: Vec<u32> = Vec::new();
    let mut level_determined = false; // This variable helps structure the L1-L5 fallback

    // L1
    if !n_sentence.adv_s_lemma_ids.is_empty() {
        if n_sentence.adv_s_lemma_ids.iter().all(|&id| profile.is_lemma_known_or_active(id)) {
            sentence_output_ids.extend(&n_sentence.adv_s_lemma_ids);
            level_determined = true;
        }
    }

    // L2
    if !level_determined && !n_sentence.sim_s_original.trim().is_empty() { // SimS text must exist
        let mut can_do_l2 = true;
        // If sim_s_lemmas_numerical is empty, it means all words in SimS are non-trackable or too simple.
        // L2 is possible if all *trackable* lemmas are K/A. If no trackable lemmas, it's vacuously true for L2.
        if n_sentence.sim_s_lemmas_numerical.is_empty() && !n_sentence.sim_s_segments_numerical.is_empty() {
            // This state: segments exist, but no overall lemmas for them based on sim_s_lemmas_numerical.
            // This could happen if all segments are proper nouns, or SimSL was empty for those segments.
            // This implies we cannot verify L2 based on lemmas for these segments.
            can_do_l2 = false; 
        }
        
        if can_do_l2 { // Only proceed if L2 still potentially viable
            for seg_lemmas_num in &n_sentence.sim_s_lemmas_numerical {
                // An empty seg_lemmas_num.lemma_ids means that specific segment has no trackable lemmas.
                // This does not automatically disqualify L2 for the *whole sentence* if other segments are fine.
                for &lemma_id in &seg_lemmas_num.lemma_ids {
                    if !profile.is_lemma_known_or_active(lemma_id) {
                        can_do_l2 = false; 
                        break; // Break from inner lemma loop
                    }
                }
                if !can_do_l2 { 
                    break; // Break from outer segment loop
                }
            }
        }

        if can_do_l2 { // If, after checking all segments, L2 is still viable
            // Collect all lemma IDs from all sim_s_lemmas_numerical segments
            for seg_lemmas_num in &n_sentence.sim_s_lemmas_numerical {
                sentence_output_ids.extend(&seg_lemmas_num.lemma_ids);
            }
            level_determined = true;
        }
    }
    
    // L3
    if !level_determined && !n_sentence.sim_s_segments_numerical.is_empty() {
        let mut temp_l3_ids = Vec::new();
        let mut l3_produced_any_spanish = false;
        let mut l3_possible_to_construct = true;
        for segment_num_data in &n_sentence.sim_s_segments_numerical {
            if let Some(seg_lemmas_num) = n_sentence.sim_s_lemmas_numerical.iter()
                .find(|sl_num| sl_num.segment_id_str == segment_num_data.id_str) {
                let mut use_sim_s_phrase_for_segment = true;
                if seg_lemmas_num.lemma_ids.is_empty() { 
                    // Segment has no trackable lemmas, use SimS part (which is text, contributes 0 IDs here)
                    use_sim_s_phrase_for_segment = true; 
                } else {
                    for &lemma_id in &seg_lemmas_num.lemma_ids {
                        if !profile.is_lemma_known_or_active(lemma_id) {
                            use_sim_s_phrase_for_segment = false; 
                            break;
                        }
                    }
                }
                if use_sim_s_phrase_for_segment {
                    temp_l3_ids.extend(&seg_lemmas_num.lemma_ids); 
                    if !seg_lemmas_num.lemma_ids.is_empty() { 
                        l3_produced_any_spanish = true; 
                    }
                } // Else: SimE part chosen (0 IDs added to temp_l3_ids)
            } else { 
                l3_possible_to_construct = false; 
                // eprintln!("[Core L3 Warn] No SimSL for SimS Segment {} in Sent {}", segment_num_data.id_str, n_sentence.sentence_id_str);
                break; 
            }
        }
        if l3_possible_to_construct && l3_produced_any_spanish {
            sentence_output_ids = temp_l3_ids; 
            level_determined = true;
        }
    }

    // L4
    if !level_determined && !n_sentence.diglot_map_numerical.is_empty() {
        let mut temp_l4_ids = Vec::new();
        let mut substitutions_made_l4 = false;
        for seg_map_num in &n_sentence.diglot_map_numerical {
            // L4 logic: substitute *one* "best" (e.g. lowest exposure active, or just first viable active)
            // word per original SimE segment/phrase boundary that the diglot map corresponds to.
            // The current diglot_map_numerical is a Vec<NumericalDiglotSegmentMap>, one per original SimS_Segment.
            let mut best_candidate_for_this_segment: Option<u32> = None;
            // For this simplified version, we just find *if* any substitution is possible in this segment.
            // A more advanced version would pick the "best" one if multiple are available.
            for entry_num in &seg_map_num.entries {
                if entry_num.viable && profile.is_lemma_known_or_active(entry_num.spa_lemma_id) {
                    best_candidate_for_this_segment = Some(entry_num.spa_lemma_id);
                    substitutions_made_l4 = true; 
                    break; // Found one viable substitution for this segment, move to next segment
                }
            }
            if let Some(lemma_id_to_add) = best_candidate_for_this_segment {
                temp_l4_ids.push(lemma_id_to_add);
            }
        }
        if substitutions_made_l4 { // If any substitutions were made across all segments
            temp_l4_ids.sort_unstable(); // Sort before dedup
            temp_l4_ids.dedup();         // Deduplicate, as same lemma might be chosen for diff segments
            sentence_output_ids = temp_l4_ids;
            // level_determined = true; // Last assignment for this, not strictly needed to set if no L5 follows
        }
    }
    sentence_output_ids
}
// ... (rest of run_simulation_numerical as it was in the last correct version)
// Make sure to copy the entire run_simulation_numerical function below this point from your working version.
// The changes below are only for run_simulation_numerical, assuming determine_sentence_output_lemma_ids is now refined.

pub fn run_simulation_numerical(
    block_sentences_numerical: &[&NumericalProcessedSentence], 
    initial_profile_for_block_run: NumericalLearnerProfile,
    available_new_lemma_ids_for_activation: &[(u32, u32)], 
    max_regeneration_attempts_per_block: u32,
    target_ct_comprehensible_threshold: f32,
    max_words_to_activate_per_regen_attempt: usize,
) -> Result<SimulationBlockResult, String> {

    let mut simulation_log_entries: Vec<String> = Vec::new();
    simulation_log_entries.push(format!(
        "Core Algo: Processing block of {} sentences. Max regen attempts: {}. Target CT: {:.2}%. Profile K: {}, A: {}",
        block_sentences_numerical.len(), max_regeneration_attempts_per_block, target_ct_comprehensible_threshold * 100.0,
        initial_profile_for_block_run.count_known(), initial_profile_for_block_run.count_active_only()
    ));

    let mut profile_being_refined_for_block = initial_profile_for_block_run.clone();
    
    for regen_attempt in 1..=max_regeneration_attempts_per_block {
        simulation_log_entries.push(format!(
            "  Regen Attempt: {}/{}",
            regen_attempt, max_regeneration_attempts_per_block
        ));

        let profile_for_this_pass = profile_being_refined_for_block.clone();
        
        let mut lemma_ids_for_current_pass: Vec<u32> = Vec::new(); 
        for n_sentence_ref in block_sentences_numerical.iter() { 
            let n_sentence = *n_sentence_ref; 
            let sentence_ids = determine_sentence_output_lemma_ids(&n_sentence, &profile_for_this_pass); 
            lemma_ids_for_current_pass.extend(sentence_ids);
        }

        let total_spanish_lemmas_this_pass = lemma_ids_for_current_pass.len();
        let known_lemmas_this_pass = if total_spanish_lemmas_this_pass > 0 {
            lemma_ids_for_current_pass.iter()
                .filter(|&&id| profile_for_this_pass.get_lemma_info(id).map_or(false, |info| info.state == LemmaState::Known))
                .count()
        } else {
            0
        };
        let actual_ct_this_pass = if total_spanish_lemmas_this_pass > 0 {
            known_lemmas_this_pass as f32 / total_spanish_lemmas_this_pass as f32
        } else { 
            0.0 
        };

        simulation_log_entries.push(format!(
            "    Pass CT: {:.2}% ({}K / {}Total). Profile for pass: K={}, A={}",
            actual_ct_this_pass * 100.0, known_lemmas_this_pass, total_spanish_lemmas_this_pass,
            profile_for_this_pass.count_known(), profile_for_this_pass.count_active_only()
        ));

        let block_is_too_easy = actual_ct_this_pass >= target_ct_comprehensible_threshold && total_spanish_lemmas_this_pass > 0;
        let block_has_no_spanish = total_spanish_lemmas_this_pass == 0;
        let is_final_regen_attempt = regen_attempt == max_regeneration_attempts_per_block;

        // Refined finalization condition
        let should_finalize = (!block_is_too_easy && !block_has_no_spanish) || // CT good and has Spanish
                              is_final_regen_attempt ||                      // Last chance
                              (block_has_no_spanish && regen_attempt > 1 && available_new_lemma_ids_for_activation.is_empty()); // No Spanish, tried activating, but no new words left to try

        if should_finalize {
            let mut message = "    Finalizing block: ".to_string();
            if is_final_regen_attempt && (block_is_too_easy || (block_has_no_spanish && regen_attempt == 1 && !available_new_lemma_ids_for_activation.is_empty())) {
                 message.push_str("Max regen attempts reached (or was too easy/no_spanish on last try).");
            } else if !block_has_no_spanish {
                 message.push_str(&format!("CT {:.2}% acceptable or final attempt with Spanish.", actual_ct_this_pass * 100.0));
            } else if block_has_no_spanish && available_new_lemma_ids_for_activation.is_empty() {
                 message.push_str("No Spanish content and no new words left to activate.");
            } else { // Default finalization message if other specific conditions weren't met for logging
                 message.push_str("Conditions met for finalization.");
            }
            simulation_log_entries.push(message);
            
            let final_profile_state_for_text_generation_val = profile_for_this_pass; 
            
            let mut profile_after_exposure = final_profile_state_for_text_generation_val.clone();
            profile_after_exposure.record_exposures(&lemma_ids_for_current_pass); 
            
            return Ok(SimulationBlockResult {
                profile_state_for_text_generation: final_profile_state_for_text_generation_val, 
                profile_state_after_block_exposure: profile_after_exposure,
                output_lemma_ids_for_block: lemma_ids_for_current_pass, 
                simulation_log_entries,
                final_ct_for_block: actual_ct_this_pass,
                known_lemmas_in_block: known_lemmas_this_pass,
                total_spanish_lemmas_in_block: total_spanish_lemmas_this_pass,
            });
        } else { // Activation needed
            let mut activation_needed_message = "    Activation Triggered: ".to_string();
            if block_has_no_spanish { 
                 activation_needed_message.push_str("No Spanish content on first try (or subsequent tries if new words are available).");
            } else { // block_is_too_easy
                 activation_needed_message.push_str(&format!("CT {:.2}% is too easy.", actual_ct_this_pass * 100.0));
            }
            simulation_log_entries.push(activation_needed_message);

            let mut words_activated_count = 0;
            // Ensure we only try to activate from the *provided list* of available new words for *this block's context*
            for (lemma_id, freq) in available_new_lemma_ids_for_activation.iter() {
                // The list available_new_lemma_ids_for_activation should already contain only 'New' words.
                // We just need to check if it's already been activated *in this current refinement cycle for the block*.
                if profile_being_refined_for_block.get_lemma_info(*lemma_id).map_or(true, |info| info.state == LemmaState::New) {
                    profile_being_refined_for_block.set_lemma_state(*lemma_id, LemmaState::Active);
                    simulation_log_entries.push(format!("      Activated Lemma ID: {} (SourceFreq: {}) to Active.", lemma_id, freq));
                    words_activated_count += 1;
                    if words_activated_count >= max_words_to_activate_per_regen_attempt { break; }
                } else if profile_being_refined_for_block.get_lemma_info(*lemma_id).map_or(false, |info| info.state == LemmaState::Active) {
                    // Already active (perhaps from a previous regen attempt for this same block), skip.
                }
            }

            if words_activated_count == 0 {
                simulation_log_entries.push("    No 'New' words were available from the pre-filtered activation list OR all suitable ones already activated in this block's refinement. Finalizing block.".to_string());
                
                let final_profile_state_for_text_generation_val = profile_for_this_pass;
                let mut profile_after_exposure = final_profile_state_for_text_generation_val.clone();
                profile_after_exposure.record_exposures(&lemma_ids_for_current_pass);

                return Ok(SimulationBlockResult {
                    profile_state_for_text_generation: final_profile_state_for_text_generation_val,
                    profile_state_after_block_exposure: profile_after_exposure,
                    output_lemma_ids_for_block: lemma_ids_for_current_pass,
                    simulation_log_entries,
                    final_ct_for_block: actual_ct_this_pass,
                    known_lemmas_in_block: known_lemmas_this_pass,
                    total_spanish_lemmas_in_block: total_spanish_lemmas_this_pass,
                });
            }
        }
    } 
    
    Err("Core algo loop completed without finalizing a block result (should be unreachable).".to_string())
}