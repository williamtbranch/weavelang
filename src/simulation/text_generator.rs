// src/simulation/text_generator.rs
use super::core_algo::{L0SegmentChoice, L1PartChoice, OutputLevel};
use crate::types::json_types::JsonSentenceBlock;
use crate::simulation::core_algo::ChosenLevelOutput;
use regex::Regex;
use once_cell::sync::Lazy;


static ITALIC_CLEANER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"_([^_[:space:]]+)_").unwrap());

/// Takes the results of the simulation for a block and generates the final text.
pub fn generate_final_text_for_block_from_levels(
    block_string_sentences: &[&JsonSentenceBlock],
    chosen_level_outputs: &[ChosenLevelOutput],
    add_debug_markers: bool,
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

        let mut assembled_sentence_text = match chosen_output.level {
            OutputLevel::AdvancedWeave => {
                // --- THIS IS THE CORRECTED BLOCK ---
                if let Some(segment_choices) = &chosen_output.l0_segment_choices {
                    let parts: Vec<String> = segment_choices
                        .iter()
                        .map(|choice| match choice {
                            L0SegmentChoice::Adv(text) => text.clone(),
                            L0SegmentChoice::SimplerAdv(text) => text.clone(),
                        })
                        .collect();
                    parts.join(" ")
                } else {
                    return Err(format!(
                        "TextGen L0 Error: No segment choices for sentence {}",
                        s_sentence.original_sentence_s_id
                    ));
                }
                // --- END OF CORRECTION ---
            }
            OutputLevel::SimpleHybrid => {
                if let Some(part_choices) = &chosen_level_outputs[idx].l1_part_choices {
                    let parts: Vec<String> = part_choices
                        .iter()
                        .map(|choice| -> String {
                            match choice {
                                L1PartChoice::Spanish(text) => {
                                    if add_debug_markers {
                                        format!("(%%{}%%)", text)
                                    } else {
                                        text.clone()
                                    }
                                }
                                L1PartChoice::Woven(text, contains_english) => {
                                    if add_debug_markers && *contains_english {
                                        format!("(%ED% {} %)", text)
                                    } else {
                                        text.clone()
                                    }
                                }
                                L1PartChoice::English(text) => text.clone(),
                            }
                        })
                        .collect();
                    parts.join(" ")
                } else {
                     return Err(format!(
                        "TextGen L1 Error: No part choices for sentence {}",
                        s_sentence.original_sentence_s_id
                    ));
                }
            }
        };

        if assembled_sentence_text.trim().is_empty() {
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
            assembled_sentence_text = s_sentence.english_text.clone();
        }
        
        let cleaned_sentence = ITALIC_CLEANER_REGEX.replace_all(&assembled_sentence_text, "$1").to_string();
        let final_sentence = cleaned_sentence.replace('_', " ");

        woven_block_text_parts.push(final_sentence);
    }

    Ok(woven_block_text_parts.join("\n\n").trim().to_string())
}