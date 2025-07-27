// src/simulation/text_generator.rs
use super::core_algo::{L0SegmentChoice, L1PartChoice, OutputLevel};
use crate::simulation::core_algo::ChosenLevelOutput;
use crate::types::json_types::JsonSentenceBlock;
use once_cell::sync::Lazy;
use regex::Regex;

static ITALIC_CLEANER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"_([^_[:space:]]+)_").unwrap());

pub fn clean_text_for_tts(text: &str) -> String {
    let italics_cleaned = ITALIC_CLEANER_REGEX.replace_all(text, "$1");
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
            OutputLevel::AdvancedWeave | OutputLevel::SimpleHybrid => {
                let parts: Vec<String> = if chosen_output.level == OutputLevel::AdvancedWeave {
                    chosen_output.l0_segment_choices.as_ref().map_or(Vec::new(), |choices| {
                        choices.iter().map(|choice| match choice {
                            L0SegmentChoice::Adv(text) | L0SegmentChoice::SimplerAdv(text) | L0SegmentChoice::InverseDiglot(text) => text.clone(),
                        }).collect()
                    })
                } else {
                    chosen_output.l1_part_choices.as_ref().map_or(Vec::new(), |choices| {
                        choices.iter().map(|choice| match choice {
                            L1PartChoice::Spanish(text) => if add_debug_markers { format!("(%%{}%%)", text) } else { text.clone() },
                            L1PartChoice::Woven(text, c) => if add_debug_markers && *c { format!("(%ED% {} %)", text) } else { text.clone() },
                            L1PartChoice::English(text) => text.clone(),
                        }).collect()
                    })
                };
                
                if parts.is_empty() {
                    let level_str = if chosen_output.level == OutputLevel::AdvancedWeave { "L0" } else { "L1" };
                     return Err(format!(
                        "TextGen {} Error: No segment/part choices for sentence {}",
                        level_str, s_sentence.original_sentence_s_id
                    ));
                }

                // *** FIX: Revert to joining with a space. This is the correct rule for joining phrases. ***
                parts.join(" ")
            }
        };

        if assembled_sentence_text.trim().is_empty() {
            assembled_sentence_text = s_sentence.english_text.clone();
        }
        
        // *** FIX: This cleanup is the necessary second step to handle the artifacts of the simple join rule. ***
        let cleaned_spacing = assembled_sentence_text
            .replace(" ,", ",")
            .replace(" .", ".")
            .replace(" :", ":")
            .replace(" ;", ";")
            .replace(" ?", "?")
            .replace(" !", "!")
            .replace("¡ ", "¡")
            .replace("¿ ", "¿")
            .replace(",\"", ", \"")
            .replace(":\"", ": \"");

        woven_block_text_parts.push(cleaned_spacing);
    }

    Ok(woven_block_text_parts.join("\n\n").trim().to_string())
}