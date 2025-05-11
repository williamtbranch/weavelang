use crate::parser::{ProcessedChapter, ProcessedSentence, DiglotEntry}; // Assuming DiglotEntry and ProcessedSentence will be used
use crate::profile::{LearnerProfile, LemmaState}; // Import LemmaState correctly

// Placeholder for your GenerationOutput struct/enum if it's defined elsewhere
// pub struct GenerationOutput {
//     pub text: String,
//     pub stats: String, // Example field
// }

// Placeholder for your DiglotCandidateType - define this according to your needs
// It should probably hold enough info to make a substitution, e.g., exact Spanish form, and sorting criteria.
#[derive(Debug, Clone)] // Added derive for potential debugging
pub struct DiglotCandidate {
    pub spa_lemma: String,
    pub exact_spa_form: String,
    pub eng_word: String,
    pub exposure_count: u32, // Assuming exposure_count is u32
    // Add any other fields needed, like original segment_id if applicable
}


pub fn generate_woven_text_for_chapter(
    chapter_data: &ProcessedChapter, // chapter_data is a reference
    profile: &mut LearnerProfile,
    // target_ct: f32, // Example: Comprehensibility Threshold
    threshold_low_vocab: u32 // Example: Threshold for L4
) -> Result<String, String> { // Changed to Result<String, String> for basic error handling
    let mut woven_text_parts: Vec<String> = Vec::new();

    // Access sentence_order directly from chapter_data (since it's a ref to ProcessedChapter)
    for sentence_id in &chapter_data.sentence_order { // No longer &chapter_data.sentence_order
        if let Some(sentence) = chapter_data.sentences.get(sentence_id) {
            // --- Start Adaptive Strategy (L1-L5) for the sentence ---
            let mut sentence_output: Option<String> = None;
            let mut l3_build_successful = true; // Initialize for L3 attempt

            // L1: Advanced Spanish
            // Now uses profile.can_understand_all_lemmas and sentence.adv_s_lemmas
            if profile.can_understand_all_lemmas(&sentence.adv_s_lemmas, true) { // true means target is "Known"
                sentence_output = Some(sentence.adv_s_text.clone());
            }

            // L2: Simple Spanish (if L1 failed or not attempted)
            if sentence_output.is_none() {
                // false means target is "Known or Active"
                if profile.can_understand_all_lemmas(&sentence.sim_s_lemmas, false) {
                    sentence_output = Some(sentence.sim_s_text.clone());
                }
            }

            // L3: Woven SimS/SimE (if L2 failed or not attempted)
            if sentence_output.is_none() {
                let mut l3_parts: Vec<String> = Vec::new();
                // Conceptual loop for L3 generation:
                // This part needs to iterate over actual SimS_Segments from the `sentence`
                // and check lemmas in `sentence.sim_s_lemmas_phrasal` (or similar) against `profile`.
                // For now, it's a placeholder.
                // for segment_info in sentence.get_sim_s_segments_with_alignments() { // Assuming a method like this
                    let use_sim_s_phrase = true; // Placeholder: determine based on lemma checks
                    let mut operation_failed_for_current_step = false; 

                    if use_sim_s_phrase { 
                        // l3_parts.push(segment_info.sim_s_phrase_text.clone());
                        l3_parts.push("Placeholder SimS phrase".to_string()); // Actual L3 SimS part
                    } else { 
                        // l3_parts.push(segment_info.get_sim_e_equivalent().unwrap_or_else(|_| {
                        //    operation_failed_for_current_step = true;
                        //    "[SimE Error]".to_string()
                        // }));
                        l3_parts.push("Placeholder SimE phrase".to_string()); // Actual L3 SimE part
                        
                        if operation_failed_for_current_step { 
                            l3_build_successful = false; 
                            break; 
                        }
                    }
                // } // End conceptual loop for L3

                if l3_build_successful && !l3_parts.is_empty() {
                    sentence_output = Some(l3_parts.join(" ")); 
                } else if !l3_build_successful {
                    // If L3 build failed, ensure sentence_output is None so L4/L5 can be tried
                    sentence_output = None;
                }
            }

            // L4: Diglot SimE/Spa (if L3 failed or not attempted, and vocab is low)
            if sentence_output.is_none() && profile.total_known_or_active_lemmas() < threshold_low_vocab {
                let best_candidate_for_l4: Option<DiglotCandidate> = {
                    let mut potential_candidates: Vec<DiglotCandidate> = Vec::new();
                    
                    // This needs to iterate over sentence.diglot_map (or similar structure)
                    // For example:
                    // if let Some(diglot_map_for_sentence) = &sentence.diglot_map {
                    //     for (_segment_key, entries) in diglot_map_for_sentence {
                    //         for diglot_entry in entries { // diglot_entry is &DiglotEntry
                    //             if diglot_entry.is_viable {
                    //                 if let Some(lemma_info) = profile.get_lemma_details(&diglot_entry.spa_lemma) {
                    //                     if lemma_info.state == LemmaState::Active { // LemmaState is now used
                    //                         potential_candidates.push(DiglotCandidate {
                    //                             spa_lemma: diglot_entry.spa_lemma.clone(),
                    //                             exact_spa_form: diglot_entry.exact_spa_form.clone(),
                    //                             eng_word: diglot_entry.eng_word.clone(),
                    //                             exposure_count: lemma_info.exposure_count,
                    //                         });
                    //                     }
                    //                 }
                    //             }
                    //         }
                    //     }
                    // }
                    
                    if !potential_candidates.is_empty() {
                        potential_candidates.sort_by_key(|cand| cand.exposure_count);
                        potential_candidates.into_iter().next()
                    } else {
                        None
                    }
                }; 

                if let Some(cand) = best_candidate_for_l4 {
                    // Construct the L4 sentence using cand.exact_spa_form and sentence.sim_e_text
                    let l4_text = format!("L4: {} (substituted for {})", cand.exact_spa_form, cand.eng_word); // Very basic placeholder
                    sentence_output = Some(l4_text); 
                }
            }

            // L5: Simple English (fallback)
            if sentence_output.is_none() {
                sentence_output = Some(sentence.sim_e_text.clone());
            }

            // --- End Adaptive Strategy ---

            if let Some(output_str) = sentence_output {
                woven_text_parts.push(output_str);
            } else {
                 return Err(format!("Could not generate output for sentence ID: {}", sentence_id));
            }
        } else {
            return Err(format!("Sentence ID {} not found in chapter data.", sentence_id));
        }
    }

    if woven_text_parts.is_empty() && !chapter_data.sentence_order.is_empty() {
        return Err("Generated empty text for a non-empty chapter.".to_string());
    }
    Ok(woven_text_parts.join("\n"))
}
