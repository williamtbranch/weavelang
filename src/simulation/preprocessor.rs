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

/// Converts a `JsonChapter` into a `NumericalChapter` and also returns a
/// vector of the word counts for each source English sentence.
pub fn json_chapter_to_numerical(
    json_chapter: &JsonChapter,
    dictionary: &mut GlobalLemmaDictionary,
) -> (NumericalChapter, Vec<usize>) { // Return type changed
    
    let mut english_word_counts: Vec<usize> = Vec::new();

    let sentences_numerical: Vec<NumericalProcessedSentence> = json_chapter
        .content_blocks
        .iter()
        .filter_map(|block| match block {
            JsonContentBlock::Sentence(s) => {
                let numerical_sentence = json_sentence_to_numerical(s, dictionary);
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

    (numerical_chapter, english_word_counts) // Return the list of counts
}

// --- The rest of the file is unchanged, but included for completeness ---
fn json_sentence_to_numerical(
    s_sentence: &JsonSentenceBlock,
    dictionary: &mut GlobalLemmaDictionary,
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
        .map(|s_bundle| NumericalAdvSegmentBundle {
            a_id_str: s_bundle.segment_id.clone(),
            adv_text_original: s_bundle.advanced_text.clone(),
            adv_lemma_ids: string_lemmas_to_ids(&s_bundle.advanced_lemmas, dictionary),
            simpler_text_original: s_bundle.simpler_text.clone(),
            simpler_lemma_ids: string_lemmas_to_ids(&s_bundle.simpler_lemmas, dictionary),
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
        .map(|pa| NumericalPhraseAlignmentToEng {
            s_segment_id_str: pa.segment_id.clone(),
            sims_l3_segment_text_original: pa.simple_spanish_text.clone(),
            eng_span_text_original: pa.english_span_text.clone(),
            eng_span_word_count: count_words(&pa.english_span_text),
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
        sentence_id_str: s_sentence.original_sentence_s_id.clone(),
        eng_text_original: s_sentence.english_text.clone(),
        eng_text_word_count: count_words(&s_sentence.english_text),
        adv_s_text_original: s_sentence.adv_spanish_full.text.clone(),
        adv_sl_overall_lemma_ids: string_lemmas_to_ids(&s_sentence.adv_spanish_full.lemmas, dictionary),
        adv_segment_bundles_numerical,
        simpler_adv_s_text_original: s_sentence.simpler_adv_spanish_full.text.clone(),
        simpler_adv_sl_overall_lemma_ids: string_lemmas_to_ids(&s_sentence.simpler_adv_spanish_full.lemmas, dictionary),
        l3_sim_s_text_original: s_sentence.simple_spanish_l3_full.text.clone(),
        l3_sim_sl_overall_lemma_ids: string_lemmas_to_ids(&s_sentence.simple_spanish_l3_full.lemmas, dictionary),
        sims_l3_segments_numerical,
        phrase_alignments_l3_to_eng_numerical,
        l3_simsl_per_segment_numerical,
        diglot_map_numerical,
    }
}