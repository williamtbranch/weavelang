//*** START FILE: src/simulation/dictionary.rs ***//
use std::collections::HashMap;
use crate::types::llm_data::ProcessedChapter; // To populate from a chapter

#[derive(Debug, Default, Clone)]
pub struct GlobalLemmaDictionary {
    pub str_to_id: HashMap<String, u32>,
    pub id_to_str: Vec<String>, // Index is the u32 ID
    next_id: u32,
}

impl GlobalLemmaDictionary {
    pub fn new() -> Self {
        GlobalLemmaDictionary {
            str_to_id: HashMap::new(),
            id_to_str: Vec::new(),
            next_id: 0, // Start IDs from 0. ID 0 will be the first word encountered.
        }
    }

    /// Gets the ID for a lemma string. If the lemma is new, it's added to the
    /// dictionary and a new ID is assigned.
    /// Lemma strings are converted to lowercase and trimmed.
    pub fn get_id_or_insert(&mut self, lemma_str: &str) -> u32 {
        let cleaned_lemma = lemma_str.trim().to_lowercase();
        // Avoid adding empty strings to the dictionary if they somehow appear.
        // The simulation logic should ideally not process empty lemma strings.
        if cleaned_lemma.is_empty() {
            // This case needs careful consideration. Returning a "dummy" ID could mask issues.
            // Panicking might be too harsh if empty lemmas are rare and ignorable.
            // For now, let's assume pre-validation ensures lemmas are non-empty.
            // If an empty string *must* be handled, it might get ID 0 if it's the first "word",
            // or a specific designated ID if we reserve one.
            // For robust behavior, let's prevent empty strings from getting valid IDs that mix with real words.
            // A simple approach is to return a high, unlikely ID or panic.
            // Or, ensure upstream (LLM output/parser) doesn't produce empty lemmas.
            // For now, let it proceed, but be mindful of this. If "" is common, it will get an ID.
        }
        
        if let Some(id) = self.str_to_id.get(&cleaned_lemma) {
            *id
        } else {
            let id = self.next_id;
            self.str_to_id.insert(cleaned_lemma.clone(), id);
            self.id_to_str.push(cleaned_lemma); // Store the cleaned (lowercase, trimmed) version
            self.next_id += 1;
            id
        }
    }

    /// Gets the ID for a lemma string if it exists. Returns None otherwise.
    /// This method does not add new lemmas.
    pub fn get_id(&self, lemma_str: &str) -> Option<u32> {
        let cleaned_lemma = lemma_str.trim().to_lowercase();
        if cleaned_lemma.is_empty() {
            return None;
        }
        self.str_to_id.get(&cleaned_lemma).copied()
    }


    /// Gets the lemma string for a given ID.
    pub fn get_str(&self, lemma_id: u32) -> Option<&String> {
        self.id_to_str.get(lemma_id as usize)
    }

    /// Returns the total number of unique lemmas in the dictionary.
    pub fn size(&self) -> usize {
        self.id_to_str.len()
    }

    /// Populates the dictionary by scanning all lemmas from a ProcessedChapter.
    pub fn populate_from_chapter(&mut self, chapter_data: &ProcessedChapter) {
        for sentence in &chapter_data.sentences {
            for lemma in &sentence.adv_s_lemmas {
                if !lemma.trim().is_empty() { // Ensure non-empty before inserting
                    self.get_id_or_insert(lemma);
                }
            }
            for segment_lemmas in &sentence.sim_s_lemmas {
                for lemma in &segment_lemmas.lemmas {
                    if !lemma.trim().is_empty() {
                        self.get_id_or_insert(lemma);
                    }
                }
            }
            for diglot_segment_map in &sentence.diglot_map {
                for entry in &diglot_segment_map.entries {
                    if !entry.spa_lemma.trim().is_empty() {
                        self.get_id_or_insert(&entry.spa_lemma);
                    }
                }
            }
        }
    }
}
//*** END FILE: src/simulation/dictionary.rs ***//