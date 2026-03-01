// src/simulation/dictionary.rs
use crate::types::json_types::{JsonChapter, JsonContentBlock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LemmaDictEntry {
    pub text: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GlobalLemmaDictionary {
    pub str_to_id: HashMap<String, u32>,
    pub id_to_entry: Vec<LemmaDictEntry>,
    next_id: u32,
}

impl GlobalLemmaDictionary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_id_or_insert(&mut self, lemma_str: &str) -> u32 {
        let cleaned_lemma = lemma_str.trim().to_lowercase();
        if cleaned_lemma.is_empty() {
            return u32::MAX;
        }

        *self
            .str_to_id
            .entry(cleaned_lemma.clone())
            .or_insert_with(|| {
                let id = self.next_id;
                self.id_to_entry.push(LemmaDictEntry {
                    text: cleaned_lemma,
                });
                self.next_id += 1;
                id
            })
    }

    pub fn get_id(&self, lemma_str: &str) -> Option<u32> {
        self.str_to_id
            .get(&lemma_str.trim().to_lowercase())
            .copied()
    }

    pub fn get_str(&self, lemma_id: u32) -> Option<&String> {
        self.id_to_entry
            .get(lemma_id as usize)
            .map(|entry| &entry.text)
    }

    pub fn populate_from_json_chapter(&mut self, json_chapter_data: &JsonChapter) {
        for block in &json_chapter_data.content_blocks {
            if let JsonContentBlock::Sentence(s_sentence) = block {
                for tier in &s_sentence.tiers {
                    for lemma in &tier.lemmas {
                        self.get_id_or_insert(lemma);
                    }
                    for segment in &tier.segments {
                        for token in &segment.tokenized_text {
                            for lemma in &token.lemmas {
                                self.get_id_or_insert(lemma);
                            }
                        }
                    }
                }

                // --- THIS IS THE FIX for the forward map ---
                for entries in s_sentence.mappings.basic_diglot.values() {
                    for (_, lemmas, _, viable, _, _) in entries {
                        if *viable {
                            for lemma in lemmas {
                                self.get_id_or_insert(lemma);
                            }
                        }
                    }
                }

                // --- THIS IS THE FIX for the inverse map ---
                for entries in s_sentence.mappings.basic_inverse_diglot.values() {
                    for (_, lemmas, _, _, _) in entries {
                        for lemma in lemmas {
                            self.get_id_or_insert(lemma);
                        }
                    }
                }
            }
        }
    }
}
