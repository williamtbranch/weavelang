// src/simulation/text_generator.rs
use super::core_algo::{ChosenLevelOutput, L0SegmentChoice, L1PartChoice, OutputLevel};
use crate::types::json_types::JsonSentenceBlock;

/// Takes the results of the simulation for a block and generates the final text.
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

        let mut current_sentence_text = match chosen_output.level {
            OutputLevel::AdvancedWeave => {
                if let Some(segment_choices) = &chosen_output.l0_segment_choices {
                    let parts: Vec<String> = segment_choices
                        .iter()
                        .enumerate() // --- ADDED enumerate ---
                        .map(|(seg_idx, choice)| { // --- ADDED seg_idx ---
                            match choice {
                                L0SegmentChoice::Adv(text) => text.clone(),
                                // --- MODIFIED: Added fallback logic ---
                                L0SegmentChoice::SimplerAdv(text) => {
                                    if !text.trim().is_empty() {
                                        text.clone()
                                    } else {
                                        // Fallback to the original advanced text for this segment if simpler is empty
                                        s_sentence.adv_spanish_segments.get(seg_idx)
                                            .map_or_else(|| text.clone(), |seg| seg.advanced_text.clone())
                                    }
                                }
                            }
                        })
                        .collect();
                    parts.join(" ")
                } else {
                    return Err(format!(
                        "TextGen L0 Error: No segment choices for sentence {}",
                        s_sentence.original_sentence_s_id
                    ));
                }
            }
            OutputLevel::SimpleHybrid => {
                if let Some(part_choices) = &chosen_output.l1_part_choices {
                    let parts: Vec<String> = part_choices.iter().map(|choice| {
                        match choice {
                            L1PartChoice::Spanish(text) => text.clone(),
                            L1PartChoice::Hybrid {
                                base_english_phrase,
                                substitution,
                            } => {
                                base_english_phrase.replacen(
                                    &substitution.eng_word_to_replace,
                                    &substitution.spa_form_to_insert,
                                    1,
                                )
                            }
                            L1PartChoice::English(text) => text.clone(),
                        }
                    }).collect();
                    parts.join(" ")
                } else {
                    return Err(format!(
                        "TextGen L1 Error: No part choices for sentence {}",
                        s_sentence.original_sentence_s_id
                    ));
                }
            }
        };

        if current_sentence_text.trim().is_empty() {
            let truncated_eng = if s_sentence.english_text.len() > 70 {
                format!("{}...", &s_sentence.english_text[..67])
            } else {
                s_sentence.english_text.clone()
            };
            
            eprintln!(
                "TextGen Warning: Generated empty text for sentence {} (\"{}\"). Using original Eng text as fallback.",
                s_sentence.original_sentence_s_id,
                truncated_eng
            );
            current_sentence_text = s_sentence.english_text.clone();
        }

        woven_block_text_parts.push(current_sentence_text);
    }

    Ok(woven_block_text_parts.join("\n\n").trim().to_string())
}