// src/domain/bridge.rs

use crate::domain::{
    mapping::{MappingEntry, TierMapping},
    primitives::{WordData, WordId},
    segment::Segment,
    sentence::Sentence,
    tier::Tier,
    token_stream::{Token, TokenStream},
};
use crate::types::json_types::{
    JsonBookMetaV2, JsonChapter, JsonContentBlock,
    JsonMappingsV2, JsonSegmentV2, JsonSentenceBlock, JsonTokenType, JsonTokenV2, JsonTierV2,
};
use std::collections::HashMap;
use std::error::Error;

pub fn json_to_domain_sentence(json_block: &JsonSentenceBlock) -> Result<Sentence, Box<dyn Error>> {
    let mut sentence = Sentence::new(json_block.s_id.clone());

    // 1. Convert Tiers (Preserving Segments)
    for json_tier in &json_block.tiers {
        let mut domain_tier = Tier::new(json_tier.tier_id.clone());
        // Preserve bulk tier lemmas
        domain_tier.lemmas = json_tier.lemmas.clone();

        let mut auto_word_id_counter: u64 = 0;
        let use_json_di = json_tier.tier_id == "basic_base";

        for json_segment in &json_tier.segments {
            let mut domain_tokens = Vec::new();

            for json_token in &json_segment.tokenized_text {
                let token = match json_token.token_type {
                    JsonTokenType::Background => Token::Background(json_token.value.clone()),
                    JsonTokenType::Word => {
                        let id_val = if use_json_di {
                            if let Some(di) = json_token.diglot_index {
                                di as u64
                            } else {
                                let id = auto_word_id_counter;
                                auto_word_id_counter += 1;
                                id
                            }
                        } else {
                            let id = auto_word_id_counter;
                            auto_word_id_counter += 1;
                            id
                        };

                        Token::Word(WordData::new(
                            WordId(id_val),
                            json_token.value.clone(),
                            json_token.lemmas.clone(),
                        ))
                    }
                };
                domain_tokens.push(token);
            }

            let stream = if domain_tokens.is_empty() {
                TokenStream::new(&json_segment.text)
            } else {
                TokenStream::from_tokens(domain_tokens)
            };

            let segment = Segment::from_stream(
                json_segment.seg_id.clone(),
                stream,
                json_segment.lemmas.clone(),
            );

            domain_tier.add_segment(segment);
        }

        sentence.add_tier(domain_tier);
    }

    // 2. Convert Mappings (Code largely unchanged, just context)
    if sentence.get_tier("basic_base").is_some() {
        let mut forward_mapping =
            TierMapping::new("basic_base".to_string(), "basic_target".to_string());
        for entries in json_block.mappings.basic_diglot.values() {
            for entry in entries {
                let word_idx = entry.0;
                let target_lemmas = &entry.1;
                let target_text = &entry.2;
                let is_viable = entry.3;
                let source_id = WordId(word_idx as u64);
                let mut map_entry =
                    MappingEntry::new(source_id, target_text.clone(), target_lemmas.clone());
                map_entry.is_viable = is_viable;
                map_entry.is_proper_noun = !entry.5.is_empty(); // Roughly infer proper noun if list present
                forward_mapping.add_entry(map_entry);
            }
        }
        if !forward_mapping.entries.is_empty() {
            sentence.add_mapping(forward_mapping);
        }
    }

    if sentence.get_tier("basic_target").is_some() {
        let mut inverse_mapping =
            TierMapping::new("basic_target".to_string(), "basic_base".to_string());
        for entries in json_block.mappings.basic_inverse_diglot.values() {
            for entry in entries {
                let word_idx = entry.0;
                let target_lemmas = &entry.1;
                let target_text = &entry.2;
                let source_id = WordId(word_idx as u64);
                let map_entry =
                    MappingEntry::new(source_id, target_text.clone(), target_lemmas.clone());
                inverse_mapping.add_entry(map_entry);
            }
        }
        if !inverse_mapping.entries.is_empty() {
            sentence.add_mapping(inverse_mapping);
        }
    }

    Ok(sentence)
}

// ---------------------------------------------------------------------------
// Domain → JSON conversion (reverse bridge)
// ---------------------------------------------------------------------------

fn domain_tokens_to_json(tokens: &[Token]) -> Vec<JsonTokenV2> {
    tokens.iter().map(|t| match t {
        Token::Background(s) => JsonTokenV2 {
            token_type: JsonTokenType::Background,
            value: s.clone(),
            diglot_index: None,
            lemmas: vec![],
            is_pn: None,
        },
        Token::Word(wd) => JsonTokenV2 {
            token_type: JsonTokenType::Word,
            value: wd.text.clone(),
            diglot_index: Some(wd.id.0 as usize),
            lemmas: wd.lemmas.clone(),
            is_pn: None,
        },
    }).collect()
}

fn domain_segment_to_json(seg: &Segment) -> JsonSegmentV2 {
    let tokenized = domain_tokens_to_json(seg.stream.tokens());

    // If the segment has no bulk lemmas, collect them from the token-level
    // lemmas.  The preprocessor relies on segment-level lemmas for the
    // NumericalAdvSegmentBundle / basic_target_lemma_ids.
    let lemmas = if seg.lemmas.is_empty() {
        seg.stream
            .tokens()
            .iter()
            .filter_map(|t| match t {
                Token::Word(wd) => Some(wd.lemmas.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    } else {
        seg.lemmas.clone()
    };

    JsonSegmentV2 {
        seg_id: seg.id.clone(),
        text: seg.full_text(),
        tokenized_text: tokenized,
        lemmas,
    }
}

fn domain_tier_to_json(tier: &Tier) -> JsonTierV2 {
    let segments: Vec<JsonSegmentV2> = tier.segments.iter().map(domain_segment_to_json).collect();

    // If the tier has no bulk lemmas, collect them from all segment lemmas.
    let lemmas = if tier.lemmas.is_empty() {
        segments.iter().flat_map(|s| s.lemmas.clone()).collect()
    } else {
        tier.lemmas.clone()
    };

    JsonTierV2 {
        tier_id: tier.id.clone(),
        full_text: tier.full_text(),
        lemmas,
        segments,
    }
}

/// Convert a domain `Sentence` back to `JsonSentenceBlock`.
///
/// The mapping conversion is best-effort: it rebuilds the
/// `basic_diglot` and `basic_inverse_diglot` JSON maps from the
/// domain `TierMapping` entries.
pub fn domain_sentence_to_json(sent: &Sentence) -> JsonSentenceBlock {
    // Tier order matters: base, advanced_target, moderate_target, basic_target, basic_base
    let tier_order = ["base", "advanced_target", "moderate_target", "basic_target", "basic_base"];
    let mut tiers = Vec::new();
    for tid in &tier_order {
        if let Some(t) = sent.tiers.get(*tid) {
            tiers.push(domain_tier_to_json(t));
        }
    }
    // Include any tiers not in the canonical order
    for (tid, t) in &sent.tiers {
        if !tier_order.contains(&tid.as_str()) {
            tiers.push(domain_tier_to_json(t));
        }
    }

    // Rebuild JsonMappingsV2 from domain TierMapping objects
    let mut basic_diglot: HashMap<String, Vec<(usize, Vec<String>, String, bool, usize, Vec<String>)>> = HashMap::new();
    let mut basic_inverse_diglot: HashMap<String, Vec<(usize, Vec<String>, String, usize, usize)>> = HashMap::new();

    for mapping in &sent.mappings {
        if mapping.from_tier_id == "basic_base" && mapping.to_tier_id == "basic_target" {
            // Forward diglot: keyed by sentence ID (single segment, use sent.id)
            let key = sent.id.clone();
            // For forward diglot the "lemmas" column must be the TARGET
            // language (Spanish) lemmas.  Look them up from the
            // basic_target tier's token stream (text→lemma map).
            let target_lemma_lookup: HashMap<String, Vec<String>> = sent
                .get_tier("basic_target")
                .map(|tier| {
                    let mut map: HashMap<String, Vec<String>> = HashMap::new();
                    for seg in &tier.segments {
                        for token in seg.stream.tokens() {
                            if let Token::Word(wd) = token {
                                map.entry(wd.text.to_lowercase())
                                    .or_insert_with(|| wd.lemmas.clone());
                            }
                        }
                    }
                    map
                })
                .unwrap_or_default();
            let entries: Vec<_> = mapping.entries.iter().map(|e| {
                let target_lemmas = if !e.target_lemmas.is_empty() {
                    e.target_lemmas.clone()
                } else {
                    // Try to resolve from basic_target tier tokens
                    target_lemma_lookup
                        .get(&e.target_text.to_lowercase())
                        .cloned()
                        .unwrap_or_else(|| {
                            // Multi-word: try first word
                            let first_word = e.target_text.split_whitespace().next().unwrap_or("");
                            target_lemma_lookup
                                .get(&first_word.to_lowercase())
                                .cloned()
                                .unwrap_or_else(|| vec![e.target_text.to_lowercase()])
                        })
                };
                let proper_lemmas: Vec<String> = if e.is_proper_noun { target_lemmas.clone() } else { vec![] };
                (
                    e.source_word_id.0 as usize,
                    target_lemmas,
                    e.target_text.clone(),
                    e.is_viable,
                    1usize, // eng_word_count — default to 1
                    proper_lemmas,
                )
            }).collect();
            basic_diglot.insert(key, entries);
        } else if mapping.from_tier_id == "basic_target" && mapping.to_tier_id == "basic_base" {
            let key = sent.id.clone();
            // For inverse diglot the "lemmas" column must be the SOURCE
            // language (Spanish) lemmas.  Look them up from the
            // basic_target tier's token stream.
            let basic_target_tier = sent.get_tier("basic_target");
            let entries: Vec<_> = mapping.entries.iter().map(|e| {
                let source_lemmas: Vec<String> = basic_target_tier
                    .and_then(|tier| {
                        tier.segments.iter().find_map(|seg| {
                            seg.stream.tokens().iter().find_map(|t| {
                                if let Token::Word(wd) = t {
                                    if wd.id == e.source_word_id {
                                        Some(wd.lemmas.clone())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                        })
                    })
                    .unwrap_or_default();
                (
                    e.source_word_id.0 as usize,
                    source_lemmas,
                    e.target_text.clone(),
                    1usize, // eng_word_count
                    1usize, // spa_word_count
                )
            }).collect();
            basic_inverse_diglot.insert(key, entries);
        }
    }

    JsonSentenceBlock {
        s_id: sent.id.clone(),
        tiers,
        mappings: JsonMappingsV2 {
            basic_diglot,
            basic_inverse_diglot,
        },
    }
}

/// Build a `JsonChapter` from an in-memory document, suitable for
/// `preprocessor::json_chapter_to_numerical` and `generate_book_instance`.
pub fn domain_sentences_to_json_chapter(
    sentences: &[Sentence],
    book_name: &str,
    base_lang: &str,
    target_lang: &str,
    level_maps: Option<&HashMap<String, crate::types::json_types::JsonCurriculumMap>>,
) -> JsonChapter {
    let content_blocks: Vec<JsonContentBlock> = sentences
        .iter()
        .map(|s| JsonContentBlock::Sentence(domain_sentence_to_json(s)))
        .collect();

    JsonChapter {
        book_meta: JsonBookMetaV2 {
            book_name: book_name.to_string(),
            schema_version: "3.2".to_string(),
            base_language: base_lang.to_string(),
            target_language: target_lang.to_string(),
        },
        content_blocks,
        u_level_maps: level_maps.cloned().unwrap_or_default(),
    }
}
