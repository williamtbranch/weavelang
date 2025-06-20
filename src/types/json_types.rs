// src/types/json_types.rs
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug, Clone)]
pub struct JsonChapter {
    pub book_name: String,
    pub processing_status: String,
    pub content_blocks: Vec<JsonContentBlock>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "block_type")]
#[serde(rename_all = "snake_case")]
pub enum JsonContentBlock {
    ChapterMarker(JsonChapterMarkerBlock),
    Sentence(JsonSentenceBlock),
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonChapterMarkerBlock {
    pub marker_text: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonTextAndLemmas {
    pub text: String,
    pub lemmas: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonAdvSpanishSegment {
    pub segment_id: String,
    pub advanced_text: String,
    pub advanced_lemmas: Vec<String>,
    pub simpler_text: String,
    pub simpler_lemmas: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonSimpleSpanishL3Segment {
    pub segment_id: String,
    pub simple_text: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonPhraseAlignmentL3ToEng {
    pub segment_id: String,
    pub simple_spanish_text: String,
    pub english_span_text: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonDiglotMapEntry {
    pub segment_id: String,
    pub english_word: String,
    pub spanish_lemma: String,
    pub exact_spanish_form: String,
    pub is_viable_for_substitution: bool,
    // --- FIELD ADDED ---
    pub note: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonSentenceBlock {
    // --- FIELDS ADDED ---
    pub source_index_in_original_file: u64, // Using u64 for flexibility
    pub llm_block_id: String,
    // --- END ADDED FIELDS ---

    pub original_sentence_s_id: String,
    pub english_text: String,
    
    // L0 Data
    pub adv_spanish_full: JsonTextAndLemmas,
    
    // L1 & L2 Data
    pub adv_spanish_segments: Vec<JsonAdvSpanishSegment>,
    pub simpler_adv_spanish_full: JsonTextAndLemmas,
    
    // L3 & L4 Data
    pub simple_spanish_l3_full: JsonTextAndLemmas,
    pub simple_spanish_l3_segments: Vec<JsonSimpleSpanishL3Segment>,
    pub phrase_alignments_l3_to_english: Vec<JsonPhraseAlignmentL3ToEng>,
    pub simple_spanish_l3_lemmas_per_segment: HashMap<String, Vec<String>>,
    
    // L5 Data (Diglot)
    pub diglot_map_entries: Vec<JsonDiglotMapEntry>,
    
    // --- FIELDS ADDED ---
    pub llm_call_status: HashMap<String, String>,
    pub processing_notes: Vec<String>,
    // --- END ADDED FIELDS ---
}