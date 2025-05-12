use crate::parser::{ProcessedChapter, DiglotEntry};
use crate::profile::{LearnerProfile, LemmaState}; // Ensure LemmaState is used or qualify its use
use regex::Regex; // For whole-word replacements in L4

// Constants (not strictly needed by this file anymore if main.rs handles thresholds for activation)
// const THRESHOLD_LOW_VOCAB_FOR_DIGLOT: usize = 50; 

pub struct GenerationOutput {
    pub woven_text: String,
    pub spanish_lemmas_used_in_output: Vec<String>,
}

pub fn generate_woven_text_for_chapter(
    chapter_data: &ProcessedChapter,
    profile: &mut LearnerProfile,
) -> Result<GenerationOutput, String> {
    
    let mut woven_batch_text = String::new();
    let mut all_spanish_lemmas_in_output_for_batch: Vec<String> = Vec::new();

    for sentence in &chapter_data.sentences {
        // println!("\nProcessing Sentence ID: {}", sentence.sentence_id);
        let mut generated_sentence_text: String = sentence.sim_e.clone(); 
        let mut current_sentence_spanish_lemmas: Vec<String> = Vec::new();
        let mut level_determined = false;

        // --- Level 1: Advanced Spanish (AdvS) ---
        if !level_determined {
            let can_do_l1 = !sentence.adv_s_lemmas.is_empty() && 
                            sentence.adv_s_lemmas.iter().all(|lemma| profile.is_lemma_known_or_active(lemma));
            if can_do_l1 {
                generated_sentence_text = sentence.adv_s.clone();
                current_sentence_spanish_lemmas.extend(sentence.adv_s_lemmas.iter().cloned());
                level_determined = true;
            }
        }

        // --- Level 2: Simple Spanish (SimS) ---
        if !level_determined {
            let can_do_l2 = !sentence.sim_s_lemmas.is_empty() && 
                            sentence.sim_s_lemmas.iter().all(|seg_lemmas| 
                                !seg_lemmas.lemmas.is_empty() &&
                                seg_lemmas.lemmas.iter().all(|lemma| profile.is_lemma_known_or_active(lemma))
                            );
            if can_do_l2 {
                generated_sentence_text = sentence.sim_s.clone();
                for seg_lemmas in &sentence.sim_s_lemmas {
                    current_sentence_spanish_lemmas.extend(seg_lemmas.lemmas.iter().cloned());
                }
                level_determined = true;
            }
        }

        // --- Level 3: Woven Simple Spanish / Simple English (SimS/SimE) ---
        if !level_determined && !sentence.sim_s_segments.is_empty() {
            let mut woven_sim_s_sim_e_parts: Vec<String> = Vec::new();
            let mut l3_lemmas_for_this_sentence: Vec<String> = Vec::new();
            let mut l3_structure_build_successful = true;
            let mut l3_produced_any_spanish = false;

            for segment_data in &sentence.sim_s_segments {
                if let Some(segment_sim_s_lemmas_obj) = sentence.sim_s_lemmas.iter().find(|sl| sl.segment_id == segment_data.id) {
                    let use_sim_s_phrase = !segment_sim_s_lemmas_obj.lemmas.is_empty() &&
                        segment_sim_s_lemmas_obj.lemmas.iter().all(|lemma| profile.is_lemma_known_or_active(lemma));

                    if use_sim_s_phrase {
                        woven_sim_s_sim_e_parts.push(segment_data.text.clone());
                        l3_lemmas_for_this_sentence.extend(segment_sim_s_lemmas_obj.lemmas.iter().cloned());
                        l3_produced_any_spanish = true;
                    } else { 
                        if let Some(alignment) = sentence.phrase_alignments.iter().find(|pa| pa.segment_id == segment_data.id) {
                            woven_sim_s_sim_e_parts.push(alignment.sim_e_span.clone());
                        } else {
                            eprintln!("[Gen L3 Err] Sentence {}: Missing PHRASE_ALIGN for segment {}", sentence.sentence_id, segment_data.id);
                            l3_structure_build_successful = false; break; 
                        }
                    }
                } else {
                    eprintln!("[Gen L3 Err] Sentence {}: Missing SimSL for segment {}", sentence.sentence_id, segment_data.id);
                    l3_structure_build_successful = false; break; 
                }
            }

            if l3_structure_build_successful && l3_produced_any_spanish {
                generated_sentence_text = woven_sim_s_sim_e_parts.join(" ");
                current_sentence_spanish_lemmas.extend(l3_lemmas_for_this_sentence);
                level_determined = true;
            }
        }
        
        // --- Level 4: Dense Diglot over Simple English (SimE base) ---
        if !level_determined { // Reached if L1, L2, and Spanish-producing L3 fail
            if !sentence.diglot_map.is_empty() {
                let mut l4_text = sentence.sim_e.clone(); 
                let mut l4_lemmas_this_sentence: Vec<String> = Vec::new();
                let mut substitutions_made_in_l4 = 0;

                // Collect applicable diglot entries first to handle potential overlapping eng_words better later (optional)
                // For now, direct iteration and replacement with regex.
                for segment_map in &sentence.diglot_map {
                    for entry in &segment_map.entries {
                        if entry.viable && profile.is_lemma_known_or_active(&entry.spa_lemma) {
                            if !entry.eng_word.is_empty() {
                                // Regex to match whole word, case-insensitively for more robustness if desired,
                                // but SimE and eng_word should ideally have consistent casing.
                                // For now, case-sensitive whole word.
                                // Escape entry.eng_word in case it contains regex special characters.
                                let pattern_string = format!(r"\b{}\b", regex::escape(&entry.eng_word));
                                match Regex::new(&pattern_string) {
                                    Ok(re) => {
                                        // Check if the pattern exists before attempting replacement.
                                        // `replacen` will do nothing if pattern not found, but this check can be explicit.
                                        if re.is_match(&l4_text) {
                                            let original_l4_text = l4_text.clone();
                                            // Replace only the first occurrence matched by the regex for this entry
                                            l4_text = re.replacen(&l4_text, 1, &*entry.exact_spa_form).to_string();
                                            
                                            if l4_text != original_l4_text { // Check if text actually changed
                                                l4_lemmas_this_sentence.push(entry.spa_lemma.clone());
                                                substitutions_made_in_l4 += 1;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[Gen L4 Regex Err] Sentence {}: Failed to compile regex for eng_word '{}': {}", sentence.sentence_id, entry.eng_word, e);
                                    }
                                }
                            }
                        }
                    }
                }

                if substitutions_made_in_l4 > 0 {
                    generated_sentence_text = l4_text;
                    current_sentence_spanish_lemmas.extend(l4_lemmas_this_sentence);
                    level_determined = true;
                }
            }
        }

        // --- Level 5: Simple English (SimE) ---
        // Default if no other level was determined or produced Spanish.
        // `generated_sentence_text` is already `sentence.sim_e.clone()` if no other level set it.
        // `current_sentence_spanish_lemmas` is empty if no Spanish was generated.

        profile.record_exposures(&current_sentence_spanish_lemmas); // CRITICAL LINE
        
        all_spanish_lemmas_in_output_for_batch.extend(current_sentence_spanish_lemmas.clone());

        woven_batch_text.push_str(&generated_sentence_text);
        woven_batch_text.push_str("\n\n"); 
    }

    Ok(GenerationOutput { 
        woven_text: woven_batch_text.trim().to_string(), 
        spanish_lemmas_used_in_output: all_spanish_lemmas_in_output_for_batch 
    })
}