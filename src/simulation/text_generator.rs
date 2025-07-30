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
    
    if chosen_level_outputs.len() != 1 || block_string_sentences.len() != 1 {
        return Err("Expected exactly one sentence and one output per call.".to_string());
    }
    let chosen_output = &chosen_level_outputs[0];
    
    let assembled_sentence_text = match chosen_output.level {
        OutputLevel::AdvancedWeave => {
            chosen_output.l0_segment_choices.as_ref().map_or_else(
                || "".to_string(),
                |choices| {
                    choices
                        .iter()
                        .map(|choice| {
                            match choice {
                                L0SegmentChoice::Adv(t)
                                | L0SegmentChoice::SimplerAdv(t)
                                | L0SegmentChoice::InverseDiglot(t) => t.clone(),
                            }
                        })
                        // *** FIX: Simply collect the segments. No joining space. ***
                        .collect::<String>()
                },
            )
        }
        OutputLevel::SimpleHybrid => {
            chosen_output.l1_part_choices.as_ref().map_or_else(
                || "".to_string(),
                |choices| {
                    choices
                        .iter()
                        .map(|choice| {
                            let (text, marker) = match choice {
                                L1PartChoice::Spanish(t) => (t, "%%"),
                                L1PartChoice::Woven(t, _) => (t, "%ED%"),
                                L1PartChoice::English(t) => (t, ""),
                            };

                            if add_debug_markers && !marker.is_empty() {
                                format!("({}{}{})", marker, text, marker)
                            } else {
                                text.clone()
                            }
                        })
                        // *** FIX: Simply collect the segments. No joining space. ***
                        .collect::<String>()
                },
            )
        }
    };
    
    // The final text is now just the assembled sentence.
    Ok(assembled_sentence_text.trim_end().to_string())
}