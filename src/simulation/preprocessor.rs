// src/simulation/preprocessor.rs
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::numerical_types::{
    NumericalAdvSegmentBundle, NumericalChapter, NumericalDiglotEntry, NumericalDiglotSegmentMap,
    NumericalPhraseAlignmentToEng, NumericalProcessedSentence, NumericalSegmentData,
    NumericalSegmentLemmas, WordToken, // Added WordToken
};
use crate::types::json_types::{JsonChapter, JsonContentBlock, JsonSentenceBlock};
use once_cell::sync::Lazy; // Added for regex
use regex::Regex;           // Added for regex
use std::collections::HashMap;

/// Helper function to count words in a string slice.
fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

// --- NEW TOKENIZATION LOGIC ---
static TOKENIZER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([\w'-’]+)|([^\w'-’]+)").unwrap());

/// Disassembles a raw string into a structured format of words and backgrounds.
/// This enforces the `[B, W, B, W, ..., B]` template.
// --- END NEW TOKENIZATION LOGIC ---

fn disassemble_string(
    raw_text: &str,
    _is_inverse_diglot: bool, // No longer needed
) -> (Vec<WordToken>, Vec<String>) {
    // A simpler regex that just finds the words.
    static WORD_FINDER: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\w'-’]+").unwrap());

    let mut words: Vec<WordToken> = Vec::new();
    let mut backgrounds: Vec<String> = Vec::new();
    let mut last_index = 0;

    for (diglot_idx, mat) in WORD_FINDER.find_iter(raw_text).enumerate() {
        // The background is the text between the end of the last match and the start of this one.
        let background_text = &raw_text[last_index..mat.start()];
        backgrounds.push(background_text.to_string());
        
        // The word is the text that was matched.
        words.push(WordToken {
            text: mat.as_str().to_string(),
            diglot_index: diglot_idx,
        });

        // Update our position.
        last_index = mat.end();
    }

    // The final background is everything from the end of the last word to the end of the string.
    let final_background = &raw_text[last_index..];
    backgrounds.push(final_background.to_string());
    
    (words, backgrounds)
}


/// Converts a `JsonChapter` into a `NumericalChapter` and also returns a
/// vector of the word counts for each source English sentence.
pub fn json_chapter_to_numerical(
    json_chapter: &JsonChapter,
    dictionary: &mut GlobalLemmaDictionary,
) -> (NumericalChapter, Vec<usize>) {
    let mut english_word_counts: Vec<usize> = Vec::new();

    let sentences_numerical: Vec<NumericalProcessedSentence> = json_chapter
        .content_blocks
        .iter()
        .filter_map(|block| match block {
            JsonContentBlock::Sentence(s) => {
                let numerical_sentence = json_sentence_to_numerical(s, dictionary, &json_chapter.book_name);
                english_word_counts.push(numerical_sentence.eng_text_word_count);
                Some(numerical_sentence)
            }
            JsonContentBlock::ChapterMarker(_) => None,
        })
        .collect();

    let numerical_chapter = NumericalChapter {
        source_file_name_original: json_chapter.book_name.clone(),
        sentences_numerical,
    };

    (numerical_chapter, english_word_counts)
}

pub fn json_sentence_to_numerical(
    s_sentence: &JsonSentenceBlock,
    dictionary: &mut GlobalLemmaDictionary,
    book_name: &str,
) -> NumericalProcessedSentence {
    let string_lemmas_to_ids =
        |lemmas: &[String], dict: &mut GlobalLemmaDictionary| -> Vec<u32> {
            lemmas
                .iter()
                .map(|s| dict.get_id_or_insert(s))
                .collect()
        };

    let adv_segment_bundles_numerical: Vec<NumericalAdvSegmentBundle> = s_sentence
        .adv_spanish_segments
        .iter()
        .map(|s_bundle| {
            let inverse_diglot_map_numerical: Vec<(String, u32, String)> = s_bundle
                .inverse_diglot_map
                .iter()
                .map(|entry| {
                    (
                        entry.spanish_word.clone(),
                        dictionary.get_id_or_insert(&entry.spanish_lemma),
                        entry.english_substitute.clone(),
                    )
                })
                .collect();
            
            // --- NEW: Disassemble the simpler_text for inverse diglot ---
            let (simpler_text_words, simpler_text_backgrounds) =
                disassemble_string(&s_bundle.simpler_text, true);

            NumericalAdvSegmentBundle {
                a_id_str: s_bundle.segment_id.clone(),
                adv_text_original: s_bundle.advanced_text.clone(),
                adv_lemma_ids: string_lemmas_to_ids(&s_bundle.advanced_lemmas, dictionary),
                simpler_text_original: s_bundle.simpler_text.clone(),
                simpler_lemma_ids: string_lemmas_to_ids(&s_bundle.simpler_lemmas, dictionary),
                inverse_diglot_map_numerical,
                simpler_text_words,       // Populate new field
                simpler_text_backgrounds, // Populate new field
            }
        })
        .collect();

    let sims_l3_segments_numerical: Vec<NumericalSegmentData> = s_sentence
        .simple_spanish_l3_segments
        .iter()
        .map(|s| NumericalSegmentData {
            id_str: s.segment_id.clone(),
            text_original: s.simple_text.clone(),
        })
        .collect();

    let phrase_alignments_l3_to_eng_numerical: Vec<NumericalPhraseAlignmentToEng> = s_sentence
        .phrase_alignments_l3_to_english
        .iter()
        .map(|pa| {
            // --- NEW: Disassemble the english_span_text for diglot ---
            let (eng_span_words, eng_span_backgrounds) =
                disassemble_string(&pa.english_span_text, false);

            NumericalPhraseAlignmentToEng {
                s_segment_id_str: pa.segment_id.clone(),
                sims_l3_segment_text_original: pa.simple_spanish_text.clone(),
                eng_span_text_original: pa.english_span_text.clone(),
                eng_span_word_count: count_words(&pa.english_span_text),
                eng_span_words,       // Populate new field
                eng_span_backgrounds, // Populate new field
            }
        })
        .collect();

    let l3_simsl_per_segment_numerical: Vec<NumericalSegmentLemmas> = s_sentence
        .simple_spanish_l3_lemmas_per_segment
        .iter()
        .map(|(id, lemmas)| NumericalSegmentLemmas {
            segment_id_str: id.clone(),
            lemma_ids: string_lemmas_to_ids(lemmas, dictionary),
        })
        .collect();

    let mut diglot_map_by_segment: HashMap<String, Vec<NumericalDiglotEntry>> = HashMap::new();
    for entry in &s_sentence.diglot_map_entries {
        let spa_lemma_id = dictionary.get_id_or_insert(&entry.spanish_lemma);

        diglot_map_by_segment
            .entry(entry.segment_id.clone())
            .or_default()
            .push(NumericalDiglotEntry {
                eng_word_original: entry.english_word.clone(),
                spa_lemma_id,
                exact_spa_form_original: entry.exact_spanish_form.clone(),
                viable: entry.is_viable_for_substitution,
            });
    }
    let mut diglot_map_numerical: Vec<NumericalDiglotSegmentMap> = diglot_map_by_segment
        .into_iter()
        .map(|(s_id, entries)| NumericalDiglotSegmentMap {
            s_segment_id_str: s_id,
            entries,
        })
        .collect();
    diglot_map_numerical.sort_by(|a, b| a.s_segment_id_str.cmp(&b.s_segment_id_str));

    NumericalProcessedSentence {
        source_file_name_original: book_name.to_string(),
        sentence_id_str: s_sentence.original_sentence_s_id.clone(),
        eng_text_original: s_sentence.english_text.clone(),
        eng_text_word_count: count_words(&s_sentence.english_text),
        adv_s_text_original: s_sentence.adv_spanish_full.text.clone(),
        adv_sl_overall_lemma_ids: string_lemmas_to_ids(
            &s_sentence.adv_spanish_full.lemmas,
            dictionary,
        ),
        adv_segment_bundles_numerical,
        simpler_adv_s_text_original: s_sentence.simpler_adv_spanish_full.text.clone(),
        simpler_adv_sl_overall_lemma_ids: string_lemmas_to_ids(
            &s_sentence.simpler_adv_spanish_full.lemmas,
            dictionary,
        ),
        l3_sim_s_text_original: s_sentence.simple_spanish_l3_full.text.clone(),
        l3_sim_sl_overall_lemma_ids: string_lemmas_to_ids(
            &s_sentence.simple_spanish_l3_full.lemmas,
            dictionary,
        ),
        sims_l3_segments_numerical,
        phrase_alignments_l3_to_eng_numerical,
        l3_simsl_per_segment_numerical,
        diglot_map_numerical,
    }
}