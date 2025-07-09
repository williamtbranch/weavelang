// src/simulation/text_generator.rs
use super::core_algo::{L0SegmentChoice, L1PartChoice, OutputLevel};
use crate::simulation::core_algo::ChosenLevelOutput;
use crate::types::json_types::JsonSentenceBlock;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use std::collections::HashMap;

// This regex finds a word surrounded by underscores, like _word_, and captures just the word.
static ITALIC_CLEANER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"_([^_[:space:]]+)_").unwrap());

/// **NEW FUNCTION:** Takes any generated text and cleans it for production/TTS output.
/// This is the function the REAL application will call just before saving the file.
pub fn clean_text_for_tts(text: &str) -> String {
    // Step 1: Handle italics like _word_ -> word
    let italics_cleaned = ITALIC_CLEANER_REGEX.replace_all(text, "$1");
    // Step 2: Globally replace all remaining underscores with spaces for the TTS.
    italics_cleaned.replace('_', " ")
}

/// Helper to perform whole-word replacement for inverse diglots.
fn replace_words(text: &str, substitutions: &HashMap<String, String>) -> String {
    if substitutions.is_empty() {
        return text.to_string();
    }
    let keys: Vec<String> = substitutions.keys().map(|k| regex::escape(k)).collect();
    let pattern = format!(r"\b({})\b", keys.join("|"));
    let re = Regex::new(&pattern).unwrap();
    re.replace_all(text, |caps: &Captures| {
        let matched_word = &caps[0];
        format!("[{}]", substitutions.get(matched_word).unwrap_or(&"ERROR".to_string()))
    }).to_string()
}

/// **RENAMED FUNCTION:** Generates the raw text, preserving underscores for testing.
pub fn generate_raw_text_from_levels(
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
                if let Some(segment_choices) = &chosen_output.l0_segment_choices {
                    let parts: Vec<String> = segment_choices
                        .iter()
                        .map(|choice| match choice {
                            L0SegmentChoice::Adv(text) => text.clone(),
                            L0SegmentChoice::SimplerAdv(text) => text.clone(),
                            L0SegmentChoice::InverseDiglot { original_text, substitutions } => {
                                let final_text = replace_words(original_text, substitutions);
                                if add_debug_markers {
                                    format!("(%ID% {} %)", final_text)
                                } else {
                                    final_text.replace(['[', ']'], "")
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
            assembled_sentence_text = s_sentence.english_text.clone();
        }
        
        // Return the raw text, leaving underscores for the test runner.
        // The separate cleaning function will handle italics.
        woven_block_text_parts.push(assembled_sentence_text);
    }

    Ok(woven_block_text_parts.join("\n\n").trim().to_string())
}