// src/simulation/text_generator.rs

use super::core_algo::{L0SegmentChoice, OutputLevel};
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
    _block_string_sentences: &[&JsonSentenceBlock],
    chosen_level_outputs: &[ChosenLevelOutput],
    _add_debug_markers: bool,
) -> Result<String, String> {
    if chosen_level_outputs.len() != 1 {
        return Err("Expected exactly one output per call.".to_string());
    }
    let chosen_output = &chosen_level_outputs[0];

    let assembled_sentence_text = match chosen_output.level {
        OutputLevel::AdvancedWeave => {
            chosen_output.l0_segment_choices.as_ref().map_or_else(
                || "".to_string(),
                |choices| {
                    choices
                        .iter()
                        .map(|choice| match choice {
                            // --- THIS IS THE FIX ---
                            // Only Adv and Mod choices exist now
                            L0SegmentChoice::Adv(t)
                            | L0SegmentChoice::Mod(t) => t.clone(),
                        })
                        .collect::<String>()
                },
            )
        }
        // --- THIS IS THE FIX ---
        // All other levels now just use the pre-assembled l1_final_text
        OutputLevel::BasicTarget |
        OutputLevel::InverseDiglot |
        OutputLevel::BasicBaseDiglot => {
            chosen_output
                .l1_final_text
                .as_ref()
                .map_or_else(|| "".to_string(), |text| text.clone())
        }
    };

    Ok(assembled_sentence_text.trim().to_string())
}