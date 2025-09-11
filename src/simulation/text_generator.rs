// In src/simulation/text_generator.rs

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
    // The block_string_sentences parameter is no longer needed but kept for signature stability for now.
    _block_string_sentences: &[&JsonSentenceBlock],
    chosen_level_outputs: &[ChosenLevelOutput],
    _add_debug_markers: bool, // Debug markers for L1 are no longer applicable
) -> Result<String, String> {
    if chosen_level_outputs.len() != 1 {
        return Err("Expected exactly one output per call.".to_string());
    }
    let chosen_output = &chosen_level_outputs[0];

    let assembled_sentence_text = match chosen_output.level {
        OutputLevel::AdvancedWeave => {
            // This logic for L0 remains unchanged.
            chosen_output.l0_segment_choices.as_ref().map_or_else(
                || "".to_string(),
                |choices| {
                    choices
                        .iter()
                        .map(|choice| match choice {
                            L0SegmentChoice::Adv(t)
                            | L0SegmentChoice::Mod(t)
                            | L0SegmentChoice::Bas(t)
                            | L0SegmentChoice::Sim(t)
                            | L0SegmentChoice::InverseDiglot(t) => t.clone(),
                        })
                        .collect::<String>()
                },
            )
        }
        OutputLevel::SimpleHybrid => {
            // This is the simplified logic for L1.
            // We just get the final, pre-assembled string.
            chosen_output
                .l1_final_text
                .as_ref()
                .map_or_else(|| "".to_string(), |text| text.clone())
        }
    };

    Ok(assembled_sentence_text.trim().to_string())
}