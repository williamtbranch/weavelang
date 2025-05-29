//*** START FILE: src/simulation/text_generator.rs ***//
use crate::types::llm_data::ProcessedSentence as StringProcessedSentence; 
use super::numerical_types::NumericalLearnerProfile; 
use super::dictionary::GlobalLemmaDictionary; 
// LemmaState is used via profile_for_generation.is_lemma_known_or_active, so direct import not strictly needed here
// use crate::profile::LemmaState; 
use regex::Regex;

pub fn generate_final_text_block(
    block_string_sentences: &[&StringProcessedSentence], 
    dictionary: &GlobalLemmaDictionary, 
    profile_for_generation: &NumericalLearnerProfile,
) -> Result<String, String> { 
    
    let mut woven_block_text_parts: Vec<String> = Vec::new();

    if block_string_sentences.is_empty() {
        return Ok(String::new());
    }

    for s_sentence_ref in block_string_sentences.iter() {
        let s_sentence = *s_sentence_ref; 

        let mut generated_sentence_text: String = s_sentence.sim_e.clone(); 
        let mut level_determined = false; 

        // --- Level 1: AdvS (Advanced Spanish) ---
        // Mirroring core_algo: L1 if !adv_s_lemmas.is_empty() AND all adv_s_lemmas are K/A
        if !s_sentence.adv_s_lemmas.is_empty() && !s_sentence.adv_s.trim().is_empty() {
            let mut can_do_l1 = true;
            for lemma_str in &s_sentence.adv_s_lemmas {
                if lemma_str.trim().is_empty() { continue; }
                match dictionary.get_id(lemma_str) {
                    Some(lemma_id) => {
                        if !profile_for_generation.is_lemma_known_or_active(lemma_id) {
                            can_do_l1 = false; break;
                        }
                    }
                    None => { can_do_l1 = false; break; }
                }
            }
            if can_do_l1 {
                generated_sentence_text = s_sentence.adv_s.clone();
                level_determined = true;
            }
        }
        
        // --- Level 2: SimS (Simple Spanish) ---
        // Mirroring core_algo: L2 if sim_s text exists AND all trackable lemmas in all SimS segments are K/A.
        if !level_determined && !s_sentence.sim_s.trim().is_empty() {
            let mut can_do_l2 = true;
            if s_sentence.sim_s_lemmas.is_empty() && !s_sentence.sim_s_segments.is_empty() {
                // If SimS has segments, but no corresponding lemma entries (sim_s_lemmas is empty),
                // we can't verify L2 based on lemmas for those segments.
                can_do_l2 = false;
            }
            
            if can_do_l2 { // Only check lemmas if still potentially L2
                for seg_lemmas_str_obj in &s_sentence.sim_s_lemmas {
                    // An empty seg_lemmas_str_obj.lemmas is fine if that segment has no trackable words.
                    for lemma_str in &seg_lemmas_str_obj.lemmas {
                        if lemma_str.trim().is_empty() { continue; }
                        match dictionary.get_id(lemma_str) {
                            Some(lemma_id) => {
                                if !profile_for_generation.is_lemma_known_or_active(lemma_id) {
                                    can_do_l2 = false; break;
                                }
                            }
                            None => { can_do_l2 = false; break; }
                        }
                    }
                    if !can_do_l2 { break; }
                }
            }

            if can_do_l2 {
                generated_sentence_text = s_sentence.sim_s.clone();
                level_determined = true;
            }
        }

        // --- Level 3: Woven SimS/SimE ---
        // Mirroring core_algo: L3 if segments exist, construction is possible, AND some Spanish was produced.
        if !level_determined && !s_sentence.sim_s_segments.is_empty() {
            let mut l3_woven_parts: Vec<String> = Vec::new();
            let mut l3_produced_any_spanish = false;
            let mut l3_possible_to_construct = true;

            for segment_data_str in &s_sentence.sim_s_segments { 
                if let Some(segment_sim_s_lemmas_str_obj) = s_sentence.sim_s_lemmas.iter()
                    .find(|sl_str| sl_str.segment_id == segment_data_str.id)
                {
                    let mut use_sim_s_phrase_for_segment = true;
                    if segment_sim_s_lemmas_str_obj.lemmas.is_empty() {
                        // Segment has no trackable lemmas, use its SimS text.
                        use_sim_s_phrase_for_segment = true; 
                    } else {
                        for lemma_str in &segment_sim_s_lemmas_str_obj.lemmas {
                            if lemma_str.trim().is_empty() { continue; }
                            match dictionary.get_id(lemma_str) {
                                Some(lemma_id) => {
                                    if !profile_for_generation.is_lemma_known_or_active(lemma_id) {
                                        use_sim_s_phrase_for_segment = false; break;
                                    }
                                }
                                None => { use_sim_s_phrase_for_segment = false; break; }
                            }
                        }
                    }
                    
                    if use_sim_s_phrase_for_segment { 
                        l3_woven_parts.push(segment_data_str.text.clone());
                        if !segment_sim_s_lemmas_str_obj.lemmas.is_empty() { // Count as Spanish if it had trackable lemmas
                           l3_produced_any_spanish = true;
                        }
                    } else { 
                        if let Some(alignment) = s_sentence.phrase_alignments.iter().find(|pa_str| pa_str.segment_id == segment_data_str.id) {
                            l3_woven_parts.push(alignment.sim_e_span.clone());
                        } else {
                            eprintln!("[TextGen L3 Err] Sent {}: Missing PHRASE_ALIGN for SimE fallback of seg {}", s_sentence.sentence_id, segment_data_str.id);
                            l3_possible_to_construct = false; break; 
                        }
                    }
                } else { 
                    eprintln!("[TextGen L3 Err] Sent {}: Missing SimSL for seg {}", s_sentence.sentence_id, segment_data_str.id);
                    l3_possible_to_construct = false; break; 
                }
            }

            if l3_possible_to_construct && l3_produced_any_spanish {
                generated_sentence_text = l3_woven_parts.join(" "); 
                level_determined = true;
            }
        }
        
        // --- Level 4: Diglot SimE/Spa ---
        // Mirroring core_algo: L4 if diglot map exists AND at least one viable, K/A substitution is made.
        // The text generator performs actual regex replacement.
        if !level_determined && !s_sentence.diglot_map.is_empty() {
            let mut l4_text_build = s_sentence.sim_e.clone(); // Start with SimE for this attempt
            let mut substitutions_made_l4 = 0;

            // Iterate over SimS_Segments to respect the "one substitution per original phrase" idea if possible
            // This requires diglot_map entries to be associated with original SimS_Segments implicitly by their order or explicitly.
            // The current s_sentence.diglot_map is Vec<DiglotSegmentMap>, one per SimS_Segment.
            for s_segment_map in &s_sentence.diglot_map {
                let current_segment_text_portion = if substitutions_made_l4 == 0 && s_segment_map.segment_id == "S1" { // approximation
                    l4_text_build.clone() // On first segment, work on whole sentence text
                } else {
                    // More complex: need to find the SimE span corresponding to this s_segment_map.segment_id
                    // For now, let's simplify: L4 regex applies to the whole evolving l4_text_build.
                    // This might lead to multiple substitutions if same EngWord appears multiple times.
                    // This simplification is different from core_algo's L4 ID collection which was "one per segment map".
                    // To truly match, text_generator L4 would need to find SimE spans for each segment.
                    // Let's stick to the simpler global regex for now for text_generator.
                    // The *impact* for text is just more L4 words if they appear. CT calc is more conservative.
                    String::new() // This part of the logic is tricky for text_generator to perfectly mirror.
                                  // For now, global replacement on l4_text_build.
                };


                let mut replaced_in_this_segment = false;
                for s_entry in &s_segment_map.entries {
                    if s_entry.spa_lemma.trim().is_empty() { continue; }
                    match dictionary.get_id(&s_entry.spa_lemma) {
                        Some(spa_lemma_id) => {
                            if s_entry.viable && profile_for_generation.is_lemma_known_or_active(spa_lemma_id) {
                                if !s_entry.eng_word.is_empty() && !s_entry.exact_spa_form.is_empty() {
                                    let pattern_string = format!(r"\b{}\b", regex::escape(&s_entry.eng_word));
                                    if let Ok(re) = Regex::new(&pattern_string) {
                                        if re.is_match(&l4_text_build) { // Check against the full evolving sentence
                                            let original_text_snapshot = l4_text_build.clone();
                                            l4_text_build = re.replacen(&l4_text_build, 1, &*s_entry.exact_spa_form).to_string();
                                            if l4_text_build != original_text_snapshot {
                                                substitutions_made_l4 +=1;
                                                replaced_in_this_segment = true;
                                                break; // Rule: One substitution per original SimS segment boundary
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        None => { /* optional warning */ }
                    }
                }
                // If applying to segments: update overall l4_text_build with modified current_segment_text_portion
            }
            if substitutions_made_l4 > 0 {
                generated_sentence_text = l4_text_build;
                // level_determined = true; // Last check, assignment not read
            }
        }
        
        woven_block_text_parts.push(generated_sentence_text);
    } 

    Ok(woven_block_text_parts.join("\n\n").trim_end().to_string())
}
//*** END FILE: src/simulation/text_generator.rs ***//