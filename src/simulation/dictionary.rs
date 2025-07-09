// src/simulation/dictionary.rs
use crate::types::json_types::{JsonChapter, JsonContentBlock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Normalizes a lemma string to be consistent with the master frequency list.
/// This logic MUST be kept in sync with the Python script that generates the list.
fn normalize_lemma(lemma_str: &str) -> String {
    // 1. Convert to lowercase and trim whitespace.
    let s = lemma_str.trim().to_lowercase();
    
    // 2. Take only the first word if there are spaces (e.g., "mostrar yo" -> "mostrar").
    let first_word = s.split_whitespace().next().unwrap_or(&s);
    
    // 3. Perform accent stripping to match the frequency list's format.
    // This is the crucial step that was missing.
    let normalized = first_word
        .replace('á', "a")
        .replace('é', "e")
        .replace('í', "i")
        .replace('ó', "o")
        .replace('ú', "u")
        .replace('ü', "u"); // Also handle diaeresis

    // Return a cleaned string, ensuring no invalid characters remain.
    // This simple filter is sufficient given the controlled input.
    normalized.chars().filter(|c| c.is_alphanumeric() || *c == '-').collect()
}


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
        let cleaned_lemma = normalize_lemma(lemma_str);

        if cleaned_lemma.is_empty() {
            return u32::MAX;
        }

        *self.str_to_id.entry(cleaned_lemma.clone()).or_insert_with(|| {
            let id = self.next_id;
            self.id_to_entry.push(LemmaDictEntry { text: cleaned_lemma });
            self.next_id += 1;
            id
        })
    }

    pub fn get_id(&self, lemma_str: &str) -> Option<u32> {
        let cleaned_lemma = normalize_lemma(lemma_str);
        self.str_to_id.get(&cleaned_lemma).copied()
    }

    pub fn get_str(&self, lemma_id: u32) -> Option<&String> {
        self.id_to_entry.get(lemma_id as usize).map(|entry| &entry.text)
    }

    pub fn populate_from_json_chapter(&mut self, json_chapter_data: &JsonChapter) {
        for block in &json_chapter_data.content_blocks {
            if let JsonContentBlock::Sentence(s_sentence) = block {
                for lemma in &s_sentence.adv_spanish_full.lemmas { self.get_id_or_insert(lemma); }
                for segment in &s_sentence.adv_spanish_segments {
                    for lemma in &segment.advanced_lemmas { self.get_id_or_insert(lemma); }
                    for lemma in &segment.simpler_lemmas { self.get_id_or_insert(lemma); }
                }
                for lemma in &s_sentence.simpler_adv_spanish_full.lemmas { self.get_id_or_insert(lemma); }
                for lemma in &s_sentence.simple_spanish_l3_full.lemmas { self.get_id_or_insert(lemma); }
                for (_s_id, lemmas) in &s_sentence.simple_spanish_l3_lemmas_per_segment {
                    for lemma in lemmas { self.get_id_or_insert(lemma); }
                }
                for entry in &s_sentence.diglot_map_entries {
                    if entry.is_viable_for_substitution { self.get_id_or_insert(&entry.spanish_lemma); }
                }
            }
        }
    }
}