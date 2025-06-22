// src/simulation/text_generator.rs

// --- START FIX: Use the correct, public path for the Regex struct. ---
use regex::Regex;
use once_cell::sync::Lazy;

use super::core_algo::{ChosenLevelOutput, L0SegmentChoice, L1PartChoice, OutputLevel};
use crate::types::json_types::JsonSentenceBlock;

static ITALIC_CLEANER_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"_([^_[:space:]]+)_").unwrap());

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

        let mut assembled_sentence_text = match chosen_output.level {
            OutputLevel::AdvancedWeave => {
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
            }
            OutputLevel::SimpleHybrid => {
                if let Some(part_choices) = &chosen_level_outputs[idx].l1_part_choices {
                    let parts: Vec<String> = part_choices
                        .iter()
                        .enumerate()
                        .map(|(part_idx, choice)| -> String {
                            match choice {
                                L1PartChoice::Spanish(text) => {
                                    if !text.trim().is_empty() {
                                        text.clone()
                                    } else {
                                        s_sentence.phrase_alignments_l3_to_english.get(part_idx)
                                            .map_or_else(|| text.clone(), |align| align.english_span_text.clone())
                                    }
                                }
                                L1PartChoice::Hybrid { base_english_phrase, substitution, } => {
                                    // --- START FIX: Corrected Regex logic with no typos ---
                                    let word_to_find = regex::escape(&substitution.eng_word_to_replace);
                                    let re_pattern = format!(r"(?i)\b{}\b", word_to_find);
                                    
                                    // Use a single, correct Regex::new call.
                                    if let Ok(re) = Regex::new(&re_pattern) {
                                        re.replace(base_english_phrase, &substitution.spa_form_to_insert).to_string()
                                    } else {
                                        // If regex compilation fails, fallback to the original phrase to avoid crashing.
                                        base_english_phrase.clone()
                                    }
                                    // --- END FIX ---
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
        // The `$1` here refers to the first captured group in the regex, which is the word inside the underscores.
        // This replaces `_word_` with just `word`, effectively removing the underscores.
        // It does not add extra spaces, relying on the original spacing.
        let cleaned_sentence = ITALIC_CLEANER_REGEX.replace_all(&assembled_sentence_text, "$1").to_string();
        
        // This second regex handles the `else'_but` case by replacing any remaining single underscores
        // with a space. We run this *after* the first replacement.
        let final_sentence = cleaned_sentence.replace('_', " ");

        woven_block_text_parts.push(final_sentence);
    }

    Ok(woven_block_text_parts.join("\n\n").trim().to_string())
}