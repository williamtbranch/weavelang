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

/// **NEW**: Defines the structure for a single entry in the inverse diglot map.
/// This will be produced by the Python pipeline (Stage 10).
#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonInverseDiglotMapEntry {
    #[serde(default)]
    pub spanish_word: String,
    #[serde(default)]
    pub spanish_lemma: String,
    #[serde(default)]
    pub english_substitute: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct JsonAdvSpanishSegment {
    pub segment_id: String,
    pub advanced_text: String,
    pub advanced_lemmas: Vec<String>,
    pub simpler_text: String,
    pub simpler_lemmas: Vec<String>,
    /// **MODIFIED**: This is now a Vector of the new entry struct, which correctly
    /// holds all the necessary data from the pipeline.
    #[serde(default)]
    pub inverse_diglot_map: Vec<JsonInverseDiglotMapEntry>,
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
    pub note: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonSentenceBlock {
    #[serde(default)]
    pub source_index_in_original_file: u64,
    #[serde(default)]
    pub llm_block_id: String,
    #[serde(default)]
    pub original_sentence_s_id: String,
    #[serde(default)]
    pub english_text: String,
    #[serde(default)]
    pub adv_spanish_full: JsonTextAndLemmas,
    #[serde(default)]
    pub adv_spanish_segments: Vec<JsonAdvSpanishSegment>,
    #[serde(default)]
    pub simpler_adv_spanish_full: JsonTextAndLemmas,
    #[serde(default)]
    pub simple_spanish_l3_full: JsonTextAndLemmas,
    #[serde(default)]
    pub simple_spanish_l3_segments: Vec<JsonSimpleSpanishL3Segment>,
    #[serde(default)]
    pub phrase_alignments_l3_to_english: Vec<JsonPhraseAlignmentL3ToEng>,
    #[serde(default)]
    pub simple_spanish_l3_lemmas_per_segment: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub diglot_map_entries: Vec<JsonDiglotMapEntry>,
    #[serde(default)]
    pub llm_call_status: HashMap<String, String>,
    #[serde(default)]
    pub processing_notes: Vec<String>,
}

impl Default for JsonTextAndLemmas {
    fn default() -> Self {
        Self {
            text: String::new(),
            lemmas: Vec::new(),
        }
    }
}