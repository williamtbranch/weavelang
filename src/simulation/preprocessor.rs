// src/simulation/preprocessor.rs

use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::numerical_types::{
    NumericalAdvSegmentBundle, NumericalChapter, NumericalDiglotEntry, NumericalDiglotSegmentMap,
    NumericalProcessedSentence,
};
use crate::types::json_types::{JsonChapter, JsonContentBlock, JsonSentenceBlock, JsonTierV2};
use std::vec::Vec;

pub fn json_chapter_to_numerical(
    json_chapter: &JsonChapter,
    dictionary: &mut GlobalLemmaDictionary,
) -> (NumericalChapter, Vec<usize>) {
    let english_word_counts: Vec<usize> = Vec::new();

    let sentences_numerical: Vec<NumericalProcessedSentence> = json_chapter
        .content_blocks
        .iter()
        .filter_map(|block| match block {
            JsonContentBlock::Sentence(s) => {
                // The basic branch is the minimum required by every recipe.
                // advanced_target / moderate_target are optional in simple
                // mode (recipes that don't pull from those tiers should
                // still produce output). Missing optional tiers are
                // synthesized as empty stubs by `json_sentence_to_numerical`.
                let required = ["basic_base", "basic_target"];
                let has_all = required.iter().all(|tid| s.tiers.iter().any(|t| t.tier_id == *tid));
                if !has_all {
                    return None;
                }
                let numerical_sentence =
                    json_sentence_to_numerical(s, dictionary, &json_chapter.book_meta.book_name);
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
fn find_tier_or_panic<'a>(tiers: &'a [JsonTierV2], tier_id: &str, s_id: &str) -> &'a JsonTierV2 {
    tiers
        .iter()
        .find(|t| t.tier_id == tier_id)
        .unwrap_or_else(|| {
            panic!("Data integrity error: Could not find '{tier_id}' tier in sentence '{s_id}'")
        })
}

pub fn json_sentence_to_numerical(
    s_sentence: &JsonSentenceBlock,
    dictionary: &mut GlobalLemmaDictionary,
    book_name: &str,
) -> NumericalProcessedSentence {
    let string_lemmas_to_ids = |lemmas: &[String], dict: &mut GlobalLemmaDictionary| -> Vec<u32> {
        lemmas.iter().map(|s| dict.get_id_or_insert(s)).collect()
    };

    let s_id = &s_sentence.s_id;

    // We no longer need the original 'base' tier. 'basic_base' is now our source of truth for English.
    // let literary_base_tier = find_tier_or_panic(&s_sentence.tiers, "base", s_id);
    let basic_base_tier = find_tier_or_panic(&s_sentence.tiers, "basic_base", s_id);
    let basic_target_tier = find_tier_or_panic(&s_sentence.tiers, "basic_target", s_id);

    // advanced_target / moderate_target are optional in simple mode. When
    // absent, fall back to empty default tiers so downstream code (which
    // only consults them for non-basic recipes) sees a well-formed shape.
    let empty_tier = JsonTierV2::default();
    let mod_target_tier = s_sentence
        .tiers
        .iter()
        .find(|t| t.tier_id == "moderate_target")
        .unwrap_or(&empty_tier);
    let adv_target_tier = s_sentence
        .tiers
        .iter()
        .find(|t| t.tier_id == "advanced_target")
        .unwrap_or(&empty_tier);

    let adv_segment_bundles_numerical: Vec<NumericalAdvSegmentBundle> = adv_target_tier
        .segments
        .iter()
        .map(|adv_seg| {
            let seg_id = &adv_seg.seg_id;
            let mod_seg = mod_target_tier
                .segments
                .iter()
                .find(|s| &s.seg_id == seg_id)
                .unwrap_or_else(|| {
                    panic!("Mismatch in seg_id '{seg_id}' for moderate tier in s_id '{s_id}'")
                });

            NumericalAdvSegmentBundle {
                a_id_str: adv_seg.seg_id.clone(),
                adv_text_original: adv_seg.text.clone(),
                adv_lemma_ids: string_lemmas_to_ids(&adv_seg.lemmas, dictionary),
                mod_text_original: mod_seg.text.clone(),
                mod_lemma_ids: string_lemmas_to_ids(&mod_seg.lemmas, dictionary),
            }
        })
        .collect();

    let basic_diglot_map_numerical: Vec<NumericalDiglotSegmentMap> = s_sentence
        .mappings
        .basic_diglot
        .iter()
        .map(|(seg_id, entries)| NumericalDiglotSegmentMap {
            s_segment_id_str: seg_id.clone(),
            entries: entries
                .iter()
                .map(
                    |(base_di, lemmas, form, viable, eng_wc, proper_noun_lemmas)| {
                        let base_word = basic_base_tier
                            .segments
                            .iter()
                            .flat_map(|s| &s.tokenized_text)
                            .find(|t| t.diglot_index == Some(*base_di))
                            .map_or("", |t| &t.value);

                        NumericalDiglotEntry {
                            base_word_di: *base_di,
                            eng_word_original: base_word.to_string(),
                            spa_lemma_ids: string_lemmas_to_ids(lemmas, dictionary),
                            exact_spa_form_original: form.clone(),
                            viable: *viable,
                            eng_word_count: *eng_wc,
                            is_base_token_pn: !proper_noun_lemmas.is_empty(),
                        }
                    },
                )
                .collect(),
        })
        .collect();

    let mut basic_inverse_diglot_map_numerical: Vec<(String, Vec<u32>, String, usize, usize)> =
        Vec::new();
    let tokens: Vec<_> = basic_target_tier
        .segments
        .iter()
        .flat_map(|s| &s.tokenized_text)
        .filter(|t| t.token_type == crate::types::json_types::JsonTokenType::Word)
        .collect();

    for entries in s_sentence.mappings.basic_inverse_diglot.values() {
        for (idx, lemmas, sub, eng_wc, _spa_wc) in entries {
            let original_word_group = tokens.get(*idx).map_or("", |t| &t.value);
            let spa_word_count = original_word_group.split_whitespace().count();
            let lemma_ids = string_lemmas_to_ids(lemmas, dictionary);
            basic_inverse_diglot_map_numerical.push((
                original_word_group.to_string(),
                lemma_ids,
                sub.clone(),
                *eng_wc,
                spa_word_count,
            ));
        }
    }

    NumericalProcessedSentence {
        source_file_name_original: book_name.to_string(),
        sentence_id_str: s_sentence.s_id.clone(),
        adv_segment_bundles_numerical,
        basic_base_tier_tokenized: basic_base_tier.segments.clone(),
        basic_target_tier_tokenized: basic_target_tier.segments.clone(),
        basic_target_lemma_ids: string_lemmas_to_ids(&basic_target_tier.lemmas, dictionary),
        basic_diglot_map_numerical,
        basic_inverse_diglot_map_numerical,
        proper_noun_lemma_ids: string_lemmas_to_ids(&s_sentence.proper_noun_lemmas, dictionary),
        eng_text_original: basic_base_tier.full_text.clone(), // <-- USE THE CORRECT TIER HERE
        eng_text_word_count: 0,
        adv_s_text_original: adv_target_tier.full_text.clone(),
    }
}
