// src/simulation/text_generator.rs
use crate::types::json_types::JsonSentenceBlock;
use super::core_algo::{ChosenLevelOutput, L1SegmentChoice, L4PartChoice, L5Substitution, OutputLevel};
use regex::Regex;
pub fn generate_final_text_for_block_from_levels(
    block_string_sentences: &[&JsonSentenceBlock],
    chosen_level_outputs: &[ChosenLevelOutput],
) -> Result<String, String> {
    if block_string_sentences.len() != chosen_level_outputs.len() {
        return Err(format!(
            "TextGen Error: Mismatch in lengths. Sentences: {}, Levels: {}",
            block_string_sentences.len(),
            chosen_level_outputs.len()
        ));
    }

    let mut woven_block_text_parts: Vec<String> = Vec::new();

    for (idx, s_sentence_ref) in block_string_sentences.iter().enumerate() {
        let s_sentence = *s_sentence_ref;
        let chosen_output = &chosen_level_outputs[idx];
        let mut current_sentence_text: String;

        match chosen_output.level {
            OutputLevel::L0 => {
                current_sentence_text = s_sentence.adv_spanish_full.text.clone();
            }
            OutputLevel::L1 => {
                if let Some(segment_choices) = &chosen_output.l1_segment_choices {
                    let parts: Vec<String> = segment_choices.iter().map(|choice| match choice {
                            L1SegmentChoice::Adv(text) => text.clone(),
                            L1SegmentChoice::SimplerAdv(text) => text.clone(),
                        }).collect();
                    current_sentence_text = parts.join(" ");
                } else {
                    return Err(format!("TextGen L1 Error: No segment choices for sentence {}", s_sentence.original_sentence_s_id));
                }
            }
            OutputLevel::L2 => {
                current_sentence_text = s_sentence.simpler_adv_spanish_full.text.clone();
            }
            OutputLevel::L3 => {
                current_sentence_text = s_sentence.simple_spanish_l3_full.text.clone();
            }
            OutputLevel::L4 => {
                // Logic to handle the new L4PartChoice enum
                if let Some(part_choices) = &chosen_output.l4_part_choices {
                    let mut parts: Vec<String> = Vec::new();
                    for choice in part_choices {
                        match choice {
                            L4PartChoice::Spanish(text) => {
                                parts.push(text.clone());
                            }
                            L4PartChoice::Hybrid { base_english_phrase, substitution } => {
                                // Perform the single-word substitution on the English phrase
                                let hybrid_phrase = base_english_phrase.replacen(
                                    &substitution.eng_word_to_replace, 
                                    &substitution.spa_form_to_insert, 
                                    1
                                );
                                parts.push(hybrid_phrase);
                            }
                            L4PartChoice::English(text) => {
                                parts.push(text.clone());
                            }
                        }
                    }
                    current_sentence_text = parts.join(" ");
                } else {
                    return Err(format!("TextGen L4 Error: No part choices for sentence {}", s_sentence.original_sentence_s_id));
                }
            }
            OutputLevel::L5 => {
                // This logic is now simpler, as it only handles one substitution per sentence
                let mut l5_text_build = s_sentence.english_text.clone();
                if let Some(substitutions) = &chosen_output.l5_substitutions {
                    // There will only be one substitution in the list for L5
                    if let Some(sub) = substitutions.first() {
                        if !sub.eng_word_to_replace.is_empty() {
                            // Use replacen to only replace the first occurrence
                            l5_text_build = l5_text_build.replacen(
                                &sub.eng_word_to_replace, 
                                &sub.spa_form_to_insert, 
                                1
                            );
                        }
                    }
                }
                current_sentence_text = l5_text_build;
            }
            OutputLevel::L6 => {
                current_sentence_text = s_sentence.english_text.clone();
            }
        }

        if current_sentence_text.trim().is_empty() {
             eprintln!("TextGen Warning: Generated empty text for {}. Using original Eng text as fallback.", s_sentence.original_sentence_s_id);
             current_sentence_text = s_sentence.english_text.clone();
        }
        
        woven_block_text_parts.push(current_sentence_text);
    }

    Ok(woven_block_text_parts.join("\n\n").trim().to_string())
}