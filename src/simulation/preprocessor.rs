use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::numerical_types::{
    NumericalAdvSegmentBundle, NumericalChapter, NumericalDiglotEntry, NumericalDiglotSegmentMap,
    NumericalPhraseAlignmentToEng, NumericalProcessedSentence, NumericalSegmentData,
    NumericalSegmentLemmas, WordToken,
};
use crate::types::json_types::{
    JsonChapter, JsonContentBlock, JsonSentenceBlock, JsonTokenV2, JsonTokenType,
};

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
                let numerical_sentence = json_sentence_to_numerical(s, dictionary, &json_chapter.book_meta.book_name);
                english_word_counts.push(numerical_sentence.eng_text_word_count);
                Some(numerical_sentence)
            }
            JsonContentBlock::ChapterMarker(_) => None,
        })
        .collect();

    let numerical_chapter = NumericalChapter {
        source_file_name_original: json_chapter.book_meta.book_name.clone(),
        sentences_numerical,
    };

    (numerical_chapter, english_word_counts)
}

fn map_tokenized_text(tokens: &[JsonTokenV2]) -> (Vec<WordToken>, Vec<String>) {
    let mut words = Vec::new();
    let mut backgrounds = Vec::new();
    let mut current_background = String::new();

    for token in tokens {
        match token.token_type {
            JsonTokenType::Background => {
                current_background.push_str(&token.value);
            }
            JsonTokenType::Word => {
                backgrounds.push(current_background);
                current_background = String::new();
                words.push(WordToken {
                    text: token.value.clone(),
                    diglot_index: token.diglot_index.unwrap_or(0),
                });
            }
        }
    }
    backgrounds.push(current_background);

    (words, backgrounds)
}

pub fn json_sentence_to_numerical(
    s_sentence: &JsonSentenceBlock,
    dictionary: &mut GlobalLemmaDictionary,
    book_name: &str,
) -> NumericalProcessedSentence {
    let string_lemmas_to_ids = |lemmas: &[String], dict: &mut GlobalLemmaDictionary| -> Vec<u32> {
        lemmas.iter().map(|s| dict.get_id_or_insert(s)).collect()
    };
    
    let base_tier = s_sentence.tiers.iter().find(|t| t.tier_id == "base").cloned().unwrap_or_default();
    let simple_target_tier = s_sentence.tiers.iter().find(|t| t.tier_id == "simple_target").cloned().unwrap_or_default();
    let adv_target_tier = s_sentence.tiers.iter().find(|t| t.tier_id == "advanced_target").cloned().unwrap_or_default();
    let simpler_adv_target_tier = s_sentence.tiers.iter().find(|t| t.tier_id == "simpler_advanced_target").cloned().unwrap_or_default();

    let all_simpler_tier_words: Vec<_> = simpler_adv_target_tier.segments.iter()
        .flat_map(|seg| seg.tokenized_text.iter())
        .filter(|tok| tok.token_type == JsonTokenType::Word)
        .collect();

    let adv_segment_bundles_numerical: Vec<NumericalAdvSegmentBundle> = adv_target_tier.segments.iter().map(|adv_seg| {
        let simpler_seg = simpler_adv_target_tier.segments.iter()
            .find(|s| s.seg_id == adv_seg.seg_id)
            .cloned()
            .unwrap_or_default();
        
        let inv_diglot_mapping = s_sentence.mappings.adv_target_to_base_inv_diglot.get(&adv_seg.seg_id).cloned().unwrap_or_default();
        let (simpler_text_words, simpler_text_backgrounds) = map_tokenized_text(&simpler_seg.tokenized_text);
        NumericalAdvSegmentBundle {
            a_id_str: adv_seg.seg_id.clone(),
            adv_text_original: adv_seg.text.clone(),
            adv_lemma_ids: string_lemmas_to_ids(&adv_seg.lemmas, dictionary),
            simpler_text_original: simpler_seg.text.clone(),
            simpler_lemma_ids: string_lemmas_to_ids(&simpler_seg.lemmas, dictionary),
            inverse_diglot_map_numerical: inv_diglot_mapping.iter().map(|(_sentence_word_idx, lemmas, sub)| {
                let original_word = all_simpler_tier_words.get(*_sentence_word_idx)
                    .map_or("", |token| &token.value);
                
                // --- THIS IS THE FIX ---
                // We are mapping over the `lemmas` Vec<String> to produce a Vec<u32>.
                // The explicit type annotation `: Vec<u32>` makes the intent clear.
                let current_entry_lemma_ids: Vec<u32> = lemmas
                    .iter()
                    .map(|l_str| dictionary.get_id_or_insert(l_str))
                    .collect();
                
                (original_word.to_string(), current_entry_lemma_ids, sub.clone())
                // --- END OF FIX ---
            }).collect(),
            simpler_text_words,
            simpler_text_backgrounds,
        }
    }).collect();
    
    let sims_l3_segments_numerical: Vec<NumericalSegmentData> = simple_target_tier.segments.iter().map(|s| {
        NumericalSegmentData {
            id_str: s.seg_id.clone(),
            text_original: s.text.clone(),
        }
    }).collect();

    let phrase_alignments_l3_to_eng_numerical: Vec<NumericalPhraseAlignmentToEng> = base_tier.segments.iter().map(|base_seg| {
        let simple_seg = simple_target_tier.segments.iter()
            .find(|s| s.seg_id == base_seg.seg_id)
            .cloned()
            .unwrap_or_default();
        
        let (eng_span_words, eng_span_backgrounds) = map_tokenized_text(&base_seg.tokenized_text);
        
        NumericalPhraseAlignmentToEng {
            s_segment_id_str: base_seg.seg_id.clone(),
            sims_l3_segment_text_original: simple_seg.text,
            eng_span_text_original: base_seg.text.clone(),
            eng_span_word_count: eng_span_words.len(),
            eng_span_words,
            eng_span_backgrounds,
        }
    }).collect();

    let l3_simsl_per_segment_numerical: Vec<NumericalSegmentLemmas> = simple_target_tier.segments.iter().map(|s| {
        NumericalSegmentLemmas {
            segment_id_str: s.seg_id.clone(),
            lemma_ids: string_lemmas_to_ids(&s.lemmas, dictionary),
        }
    }).collect();

    let diglot_map_numerical: Vec<NumericalDiglotSegmentMap> = s_sentence.mappings.simple_target_to_base_diglot.iter().map(|(seg_id, entries)| {
        NumericalDiglotSegmentMap {
            s_segment_id_str: seg_id.clone(),
            entries: entries.iter().map(|(base_di, lemmas, form, viable)| {
                let base_word = base_tier.segments.iter()
                    .flat_map(|s| &s.tokenized_text)
                    .find(|t| t.diglot_index == Some(*base_di))
                    .map_or("", |t| &t.value);

                NumericalDiglotEntry {
                    base_word_di: *base_di,
                    eng_word_original: base_word.to_string(),
                    spa_lemma_ids: lemmas.iter().map(|s| dictionary.get_id_or_insert(s)).collect(),
                    exact_spa_form_original: form.clone(),
                    viable: *viable,
                }
            }).collect()
        }
    }).collect();

    NumericalProcessedSentence {
        source_file_name_original: book_name.to_string(),
        sentence_id_str: s_sentence.s_id.clone(),
        eng_text_original: base_tier.full_text,
        eng_text_word_count: base_tier.segments.iter().flat_map(|s| &s.tokenized_text).filter(|t| t.token_type == JsonTokenType::Word).count(),
        adv_s_text_original: adv_target_tier.full_text.clone(),
        adv_sl_overall_lemma_ids: string_lemmas_to_ids(&adv_target_tier.lemmas, dictionary),
        adv_segment_bundles_numerical,
        simpler_adv_s_text_original: simpler_adv_target_tier.full_text,
        simpler_adv_sl_overall_lemma_ids: string_lemmas_to_ids(&simpler_adv_target_tier.lemmas, dictionary),
        l3_sim_s_text_original: simple_target_tier.full_text,
        l3_sim_sl_overall_lemma_ids: string_lemmas_to_ids(&simple_target_tier.lemmas, dictionary),
        sims_l3_segments_numerical,
        phrase_alignments_l3_to_eng_numerical,
        l3_simsl_per_segment_numerical,
        diglot_map_numerical,
    }
}