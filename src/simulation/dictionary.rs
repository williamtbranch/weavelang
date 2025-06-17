// src/simulation/dictionary.rs
use crate::simulation::frequency_manager;
use crate::types::json_types::{JsonChapter, JsonContentBlock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LemmaDictEntry {
    pub text: String,
    pub required_exposure_threshold: u32,
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

    /// Gets the ID for a given lemma string. If the lemma is new, it's inserted
    /// into the dictionary, its required exposure threshold is calculated once,
    /// and a new ID is assigned.
    pub fn get_id_or_insert(&mut self, lemma_str: &str) -> u32 {
        let cleaned_lemma = lemma_str.trim().to_lowercase();
        if cleaned_lemma.is_empty() {
            return u32::MAX; // Use a sentinel for empty/invalid lemmas
        }

        // Use the entry API to avoid cloning the string if it already exists.
        *self.str_to_id.entry(cleaned_lemma.clone()).or_insert_with(|| {
            let id = self.next_id;
            // Calculate the threshold here, ONCE, upon first discovery.
            let threshold = frequency_manager::get_exposure_threshold_for_lemma(&cleaned_lemma);

            self.id_to_entry.push(LemmaDictEntry {
                text: cleaned_lemma,
                required_exposure_threshold: threshold,
            });

            self.next_id += 1;
            id
        })
    }

    /// Gets the ID for a lemma string if it exists in the dictionary.
    pub fn get_id(&self, lemma_str: &str) -> Option<u32> {
        self.str_to_id.get(lemma_str.trim().to_lowercase().as_str()).copied()
    }

    /// Gets the string representation of a lemma from its ID.
    pub fn get_str(&self, lemma_id: u32) -> Option<&String> {
        self.id_to_entry.get(lemma_id as usize).map(|entry| &entry.text)
    }
    
    /// Gets the required exposure threshold for a lemma from its ID.
    pub fn get_threshold(&self, lemma_id: u32) -> Option<u32> {
        self.id_to_entry.get(lemma_id as usize).map(|entry| entry.required_exposure_threshold)
    }

    /// Populates the dictionary by scanning all lemma strings from a `JsonChapter`.
    /// This is the main entry point for discovering all words in a book.
    pub fn populate_from_json_chapter(&mut self, json_chapter_data: &JsonChapter) {
        for block in &json_chapter_data.content_blocks {
            if let JsonContentBlock::Sentence(s_sentence) = block {
                // L0
                for lemma in &s_sentence.adv_spanish_full.lemmas {
                    self.get_id_or_insert(lemma);
                }
                // L1/L2
                for segment in &s_sentence.adv_spanish_segments {
                    for lemma in &segment.advanced_lemmas {
                        self.get_id_or_insert(lemma);
                    }
                    for lemma in &segment.simpler_lemmas {
                        self.get_id_or_insert(lemma);
                    }
                }
                for lemma in &s_sentence.simpler_adv_spanish_full.lemmas {
                    self.get_id_or_insert(lemma);
                }
                // L3/L4
                for lemma in &s_sentence.simple_spanish_l3_full.lemmas {
                    self.get_id_or_insert(lemma);
                }
                for (_s_id, lemmas) in &s_sentence.simple_spanish_l3_lemmas_per_segment {
                    for lemma in lemmas {
                        self.get_id_or_insert(lemma);
                    }
                }
                // L5
                for entry in &s_sentence.diglot_map_entries {
                    if entry.is_viable_for_substitution {
                        self.get_id_or_insert(&entry.spanish_lemma);
                    }
                }
            }
        }
    }
}