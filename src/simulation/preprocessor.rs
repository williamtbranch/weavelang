//*** START FILE: src/simulation/preprocessor.rs ***//
use crate::types::llm_data::{
    ProcessedChapter as StringProcessedChapter,
    // The sub-structs like ProcessedSentence, SegmentData etc. from llm_data
    // are implicitly used via field access on StringProcessedChapter and its sentences.
    // We don't need to explicitly import their type names here unless we were
    // creating them or using their type names in function signatures within this file.
};
use super::dictionary::GlobalLemmaDictionary;
use super::numerical_types::{
    NumericalChapter,
    NumericalProcessedSentence,
    NumericalSegmentData,
    NumericalPhraseAlignment,
    NumericalSegmentLemmas,
    NumericalDiglotSegmentMap,
    NumericalDiglotEntry,
};

pub fn to_numerical_chapter(
    string_chapter: &StringProcessedChapter,
    dictionary: &mut GlobalLemmaDictionary, // Mutable to insert new lemma IDs if encountered
) -> NumericalChapter {
    let mut sentences_numerical = Vec::with_capacity(string_chapter.sentences.len());

    for s_sentence in &string_chapter.sentences { // s_sentence is &llm_data::ProcessedSentence
        let adv_s_lemma_ids: Vec<u32> = s_sentence
            .adv_s_lemmas
            .iter()
            .filter_map(|lemma_str| { // Filter out empty strings before getting ID
                let cleaned = lemma_str.trim();
                if !cleaned.is_empty() {
                    Some(dictionary.get_id_or_insert(cleaned))
                } else {
                    None
                }
            })
            .collect();

        let sim_s_lemmas_numerical: Vec<NumericalSegmentLemmas> = s_sentence
            .sim_s_lemmas
            .iter()
            .map(|s_seg_lemmas| NumericalSegmentLemmas { // s_seg_lemmas is &llm_data::SegmentLemmas
                segment_id_str: s_seg_lemmas.segment_id.clone(),
                lemma_ids: s_seg_lemmas
                    .lemmas
                    .iter()
                    .filter_map(|lemma_str| {
                        let cleaned = lemma_str.trim();
                        if !cleaned.is_empty() {
                            Some(dictionary.get_id_or_insert(cleaned))
                        } else {
                            None
                        }
                    })
                    .collect(),
            })
            .collect();
        
        let diglot_map_numerical: Vec<NumericalDiglotSegmentMap> = s_sentence
            .diglot_map
            .iter()
            .map(|s_diglot_map| NumericalDiglotSegmentMap { // s_diglot_map is &llm_data::DiglotSegmentMap
                segment_id_str: s_diglot_map.segment_id.clone(),
                entries: s_diglot_map
                    .entries
                    .iter()
                    .filter_map(|s_entry| { // s_entry is &llm_data::DiglotEntry
                        let cleaned_spa_lemma = s_entry.spa_lemma.trim();
                        if !cleaned_spa_lemma.is_empty() {
                            Some(NumericalDiglotEntry {
                                eng_word_original: s_entry.eng_word.clone(),
                                spa_lemma_id: dictionary.get_id_or_insert(cleaned_spa_lemma),
                                exact_spa_form_original: s_entry.exact_spa_form.clone(),
                                viable: s_entry.viable,
                            })
                        } else {
                            // Optionally log if a diglot entry has an empty spa_lemma
                            // eprintln!("Warning: Diglot entry for Eng '{}' has empty SpaLemma in sentence {}", s_entry.eng_word, s_sentence.sentence_id_str);
                            None
                        }
                    })
                    .collect(),
            })
            .collect();

        let sim_s_segments_numerical: Vec<NumericalSegmentData> = s_sentence
            .sim_s_segments
            .iter()
            .map(|s_seg_data| NumericalSegmentData { // s_seg_data is &llm_data::SegmentData
                id_str: s_seg_data.id.clone(),
                text_original: s_seg_data.text.clone(),
            })
            .collect();

        let phrase_alignments_numerical: Vec<NumericalPhraseAlignment> = s_sentence
            .phrase_alignments
            .iter()
            .map(|s_pa| NumericalPhraseAlignment { // s_pa is &llm_data::PhraseAlignment
                segment_id_str: s_pa.segment_id.clone(),
                adv_s_span_original: s_pa.adv_s_span.clone(),
                sim_e_span_original: s_pa.sim_e_span.clone(),
            })
            .collect();

        let n_sentence = NumericalProcessedSentence {
            sentence_id_str: s_sentence.sentence_id.clone(),
            adv_s_original: s_sentence.adv_s.clone(),
            sim_s_original: s_sentence.sim_s.clone(),
            sim_e_original: s_sentence.sim_e.clone(),
            sim_s_segments_numerical,
            phrase_alignments_numerical,
            sim_s_lemmas_numerical,
            adv_s_lemma_ids,
            diglot_map_numerical,
            locked_phrase_segment_id_strs: s_sentence.locked_phrases.clone(),
        };
        sentences_numerical.push(n_sentence);
    }

    NumericalChapter {
        source_file_name_original: string_chapter.source_file_name.clone(),
        sentences_numerical,
    }
}
//*** END FILE: src/simulation/preprocessor.rs ***//