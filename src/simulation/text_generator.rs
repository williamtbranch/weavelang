// src/simulation/text_generator.rs
use super::core_algo::{L0SegmentChoice, L1PartChoice, OutputLevel};
use crate::simulation::core_algo::ChosenLevelOutput;
use crate::types::json_types::JsonSentenceBlock;
use once_cell::sync::Lazy;
use regex::Regex;

// This regex finds a word surrounded by underscores, like _word_, and captures just the word.
static ITALIC_CLEANER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"_([^_[:space:]]+)_").unwrap());

pub fn clean_text_for_tts(text: &str) -> String {
    // Step 1: Handle italics like _word_ -> word
    let italics_cleaned = ITALIC_CLEANER_REGEX.replace_all(text, "$1");
    // Step 2: Globally replace all remaining underscores with spaces for the TTS.
    italics_cleaned.replace('_', " ")
}

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
                            L0SegmentChoice::InverseDiglot { final_words } => {
                                // *** FIX: The `final_words` vec now contains a single,
                                // pre-assembled, punctuated string. We just need to get it.
                                final_words.get(0).cloned().unwrap_or_default()
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
        
        woven_block_text_parts.push(assembled_sentence_text);
    }

    Ok(woven_block_text_parts.join("\n\n").trim().to_string())
}