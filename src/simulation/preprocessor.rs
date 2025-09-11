// In src/simulation/preprocessor.rs

use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::numerical_types::{
    NumericalAdvSegmentBundle, NumericalChapter, NumericalDiglotEntry, NumericalDiglotSegmentMap,
    NumericalProcessedSentence, WordToken,
};
use crate::types::json_types::{
    JsonChapter, JsonContentBlock, JsonSentenceBlock, JsonTokenV2, JsonTokenType, JsonTierV2
};
use std::vec::Vec;

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
                let numerical_sentence =
                    json_sentence_to_numerical(s, dictionary, &json_chapter.book_meta.book_name);
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

// Helper function to find a tier or panic with a clear error
fn find_tier_or_panic<'a>(tiers: &'a [JsonTierV2], tier_id: &str, s_id: &str) -> &'a JsonTierV2 {
    tiers.iter().find(|t| t.tier_id == tier_id)
        .unwrap_or_else(|| panic!("Data integrity error: Could not find '{}' tier in sentence '{}'", tier_id, s_id))
}

pub fn json_sentence_to_numerical(
    s_sentence: &JsonSentenceBlock,
    dictionary: &mut GlobalLemmaDictionary,
    book_name: &str,
) -> NumericalProcessedSentence {
    let string_lemmas_to_ids = |lemmas: &[String], dict: &mut GlobalLemmaDictionary| -> Vec<u32> {
        lemmas
            .iter()
            .map(|s| dict.get_id_or_insert(s))
            .collect()
    };
    
    let s_id = &s_sentence.s_id;
    let base_tier = find_tier_or_panic(&s_sentence.tiers, "base", s_id);
    let adv_target_tier = find_tier_or_panic(&s_sentence.tiers, "advanced_target", s_id);
    let mod_target_tier = find_tier_or_panic(&s_sentence.tiers, "moderate_target", s_id);
    let bas_target_tier = find_tier_or_panic(&s_sentence.tiers, "basic_target", s_id);
    let sim_target_tier = find_tier_or_panic(&s_sentence.tiers, "simple_target", s_id);

    let all_sim_tier_words: Vec<_> = sim_target_tier
        .segments
        .iter()
        .flat_map(|seg| seg.tokenized_text.iter())
        .filter(|tok| tok.token_type == JsonTokenType::Word)
        .collect();

    let adv_segment_bundles_numerical: Vec<NumericalAdvSegmentBundle> = adv_target_tier
        .segments
        .iter()
        .map(|adv_seg| {
            let seg_id = &adv_seg.seg_id;
            let mod_seg = mod_target_tier.segments.iter().find(|s| &s.seg_id == seg_id).unwrap_or_else(|| panic!("Mismatch in seg_id '{}' for moderate tier in s_id '{}'", seg_id, s_id));
            let bas_seg = bas_target_tier.segments.iter().find(|s| &s.seg_id == seg_id).unwrap_or_else(|| panic!("Mismatch in seg_id '{}' for basic tier in s_id '{}'", seg_id, s_id));
            let sim_seg = sim_target_tier.segments.iter().find(|s| &s.seg_id == seg_id).unwrap_or_else(|| panic!("Mismatch in seg_id '{}' for simple tier in s_id '{}'", seg_id, s_id));

            let inv_diglot_mapping = s_sentence.mappings.adv_target_to_base_inv_diglot.get(seg_id).cloned().unwrap_or_default();
            let (sim_text_words, sim_text_backgrounds) = map_tokenized_text(&sim_seg.tokenized_text);

            NumericalAdvSegmentBundle {
                a_id_str: adv_seg.seg_id.clone(),
                adv_text_original: adv_seg.text.clone(),
                adv_lemma_ids: string_lemmas_to_ids(&adv_seg.lemmas, dictionary),
                
                // --- THIS IS THE FIX ---
                mod_text_original: mod_seg.text.clone(),
                mod_lemma_ids: string_lemmas_to_ids(&mod_seg.lemmas, dictionary),
                // --- END OF FIX ---
                
                bas_text_original: bas_seg.text.clone(),
                bas_lemma_ids: string_lemmas_to_ids(&bas_seg.lemmas, dictionary),
                sim_text_original: sim_seg.text.clone(),
                sim_lemma_ids: string_lemmas_to_ids(&sim_seg.lemmas, dictionary),
                inverse_diglot_map_numerical: inv_diglot_mapping
                    .iter()
                    .map(|(v_token_idx, lemmas, sub, eng_wc)| {
                        // NOTE: Using v_token_idx directly here assumes a flat list of words across all segments of the simple tier
                        let original_word = all_sim_tier_words.get(*v_token_idx).map_or("", |token| &token.value);
                        let lemma_ids = lemmas.iter().map(|l| dictionary.get_id_or_insert(l)).collect();
                        (original_word.to_string(), lemma_ids, sub.clone(), *eng_wc)
                    })
                    .collect(),
                sim_text_words,
                sim_text_backgrounds,
            }
        })
        .collect();

    let diglot_map_numerical: Vec<NumericalDiglotSegmentMap> = s_sentence
        .mappings
        .simple_target_to_base_diglot
        .iter()
        .map(|(seg_id, entries)| {
            NumericalDiglotSegmentMap {
                s_segment_id_str: seg_id.clone(),
                entries: entries
                    .iter()
                    .map(|(base_di, lemmas, form, viable, eng_wc)| {
                        let base_word = base_tier.segments.iter().flat_map(|s| &s.tokenized_text)
                            .find(|t| t.diglot_index == Some(*base_di)).map_or("", |t| &t.value);
                        NumericalDiglotEntry {
                            base_word_di: *base_di,
                            eng_word_original: base_word.to_string(),
                            spa_lemma_ids: lemmas.iter().map(|s| dictionary.get_id_or_insert(s)).collect(),
                            exact_spa_form_original: form.clone(),
                            viable: *viable,
                            eng_word_count: *eng_wc,
                        }
                    })
                    .collect(),
            }
        })
        .collect();

    NumericalProcessedSentence {
        source_file_name_original: book_name.to_string(),
        sentence_id_str: s_sentence.s_id.clone(),
        eng_text_original: base_tier.full_text.clone(),
        eng_text_word_count: base_tier.segments.iter().flat_map(|s| &s.tokenized_text).filter(|t| t.token_type == JsonTokenType::Word).count(),
        base_tier_tokenized: base_tier.segments.clone(),
        adv_s_text_original: adv_target_tier.full_text.clone(),
        adv_sl_overall_lemma_ids: string_lemmas_to_ids(&adv_target_tier.lemmas, dictionary),
        adv_segment_bundles_numerical,
        diglot_map_numerical,
    }
}