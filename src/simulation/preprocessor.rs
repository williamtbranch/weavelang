// src/simulation/preprocessor.rs

use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::numerical_types::{
    NumericalAdvSegmentBundle, NumericalChapter, NumericalDiglotEntry, NumericalDiglotSegmentMap,
    NumericalPhraseAlignmentToEng, NumericalProcessedSentence, NumericalSegmentData,
    NumericalSegmentLemmas,
};
use crate::types::json_types::{JsonChapter, JsonContentBlock, JsonSentenceBlock};
use std::collections::HashMap;

/// Helper function to count words in a string slice.
fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Converts a `JsonChapter` into a `NumericalChapter` and also generates a
/// frequency map of all lemma IDs within that chapter.
pub fn json_chapter_to_numerical(
    json_chapter: &JsonChapter,
    dictionary: &mut GlobalLemmaDictionary,
) -> (NumericalChapter, HashMap<u32, u32>) {
    let mut book_frequency_map: HashMap<u32, u32> = HashMap::new();

    let sentences_numerical: Vec<NumericalProcessedSentence> = json_chapter
        .content_blocks
        .iter()
        .filter_map(|block| match block {
            JsonContentBlock::Sentence(s) => {
                Some(json_sentence_to_numerical(s, dictionary, &mut book_frequency_map))
            }
            JsonContentBlock::ChapterMarker(_) => None,
        })
        .collect();

    let numerical_chapter = NumericalChapter {
        source_file_name_original: json_chapter.book_name.clone(),
        sentences_numerical,
    };

    (numerical_chapter, book_frequency_map)
}

/// Helper function to convert a single `JsonSentenceBlock` to `NumericalProcessedSentence`
/// while also populating the book-wide frequency map.
fn json_sentence_to_numerical(
    s_sentence: &JsonSentenceBlock,
    dictionary: &mut GlobalLemmaDictionary,
    book_frequency_map: &mut HashMap<u32, u32>,
) -> NumericalProcessedSentence {
    // Helper closure to convert string lemmas to IDs and update frequency map
    let string_lemmas_to_ids =
        |lemmas: &[String], dict: &mut GlobalLemmaDictionary, freq_map: &mut HashMap<u32, u32>| -> Vec<u32> {
            lemmas
                .iter()
                .map(|s| {
                    let id = dict.get_id_or_insert(s);
                    if id != u32::MAX {
                        *freq_map.entry(id).or_insert(0) += 1;
                    }
                    id
                })
                .collect()
        };

    // L1 Data
    let adv_segment_bundles_numerical: Vec<NumericalAdvSegmentBundle> = s_sentence
        .adv_spanish_segments
        .iter()
        .map(|s_bundle| NumericalAdvSegmentBundle {
            a_id_str: s_bundle.segment_id.clone(),
            adv_text_original: s_bundle.advanced_text.clone(),
            adv_lemma_ids: string_lemmas_to_ids(
                &s_bundle.advanced_lemmas,
                dictionary,
                book_frequency_map,
            ),
            simpler_text_original: s_bundle.simpler_text.clone(),
            simpler_lemma_ids: string_lemmas_to_ids(
                &s_bundle.simpler_lemmas,
                dictionary,
                book_frequency_map,
            ),
        })
        .collect();

    // L4 Data
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
        .map(|pa| NumericalPhraseAlignmentToEng {
            s_segment_id_str: pa.segment_id.clone(),
            sims_l3_segment_text_original: pa.simple_spanish_text.clone(),
            eng_span_text_original: pa.english_span_text.clone(),
            // --- POPULATE NEW FIELD ---
            eng_span_word_count: count_words(&pa.english_span_text),
        })
        .collect();

    let l3_simsl_per_segment_numerical: Vec<NumericalSegmentLemmas> = s_sentence
        .simple_spanish_l3_lemmas_per_segment
        .iter()
        .map(|(id, lemmas)| NumericalSegmentLemmas {
            segment_id_str: id.clone(),
            lemma_ids: string_lemmas_to_ids(lemmas, dictionary, book_frequency_map),
        })
        .collect();

    // L5 Data (Diglot)
    let mut diglot_map_by_segment: HashMap<String, Vec<NumericalDiglotEntry>> = HashMap::new();
    for entry in &s_sentence.diglot_map_entries {
        let spa_lemma_id = dictionary.get_id_or_insert(&entry.spanish_lemma);
        if spa_lemma_id != u32::MAX {
            *book_frequency_map.entry(spa_lemma_id).or_insert(0) += 1;
        }

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
    // Sort for deterministic order
    diglot_map_numerical.sort_by(|a, b| a.s_segment_id_str.cmp(&b.s_segment_id_str));

    NumericalProcessedSentence {
        sentence_id_str: s_sentence.original_sentence_s_id.clone(),
        eng_text_original: s_sentence.english_text.clone(),
        // --- POPULATE NEW FIELD ---
        eng_text_word_count: count_words(&s_sentence.english_text),
        // L0
        adv_s_text_original: s_sentence.adv_spanish_full.text.clone(),
        adv_sl_overall_lemma_ids: string_lemmas_to_ids(
            &s_sentence.adv_spanish_full.lemmas,
            dictionary,
            book_frequency_map,
        ),
        // L1
        adv_segment_bundles_numerical,
        // L2
        simpler_adv_s_text_original: s_sentence.simpler_adv_spanish_full.text.clone(),
        simpler_adv_sl_overall_lemma_ids: string_lemmas_to_ids(
            &s_sentence.simpler_adv_spanish_full.lemmas,
            dictionary,
            book_frequency_map,
        ),
        // L3
        l3_sim_s_text_original: s_sentence.simple_spanish_l3_full.text.clone(),
        l3_sim_sl_overall_lemma_ids: string_lemmas_to_ids(
            &s_sentence.simple_spanish_l3_full.lemmas,
            dictionary,
            book_frequency_map,
        ),
        // L4
        sims_l3_segments_numerical,
        phrase_alignments_l3_to_eng_numerical,
        l3_simsl_per_segment_numerical,
        // L5
        diglot_map_numerical,

        // P&C fields will be populated later, so they are defaulted.
        l0_upgrade_pc: Default::default(),
        l1_segment_upgrade_pcs: Default::default(),
    }
}