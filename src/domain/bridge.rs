// src/domain/bridge.rs

use crate::domain::{
    mapping::{MappingEntry, TierMapping},
    primitives::{WordData, WordId},
    segment::Segment,
    sentence::Sentence,
    tier::Tier,
    token_stream::{Token, TokenStream},
};
use crate::types::json_types::{JsonSentenceBlock, JsonTokenType};
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
