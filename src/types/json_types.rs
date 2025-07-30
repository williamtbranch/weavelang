// src/types/json_types.rs
use serde::Deserialize;
use std::collections::HashMap;

// --- Re-usable child structs ---

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonChapterMarkerBlock {
    pub marker_text: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JsonTokenType {
    #[serde(rename = "b")]
    Background,
    #[serde(rename = "w")]
    Word,
}

impl Default for JsonTokenType {
    fn default() -> Self {
        JsonTokenType::Background
    }
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonTokenV2 {
    #[serde(rename = "t")]
    pub token_type: JsonTokenType,
    #[serde(rename = "v")]
    pub value: String,
    #[serde(default, rename = "di")]
    pub diglot_index: Option<usize>,
    #[serde(default, rename = "l")]
    pub lemmas: Vec<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonSegmentV2 {
    pub seg_id: String,
    pub post_separator: String,
    pub tokenized_text: Vec<JsonTokenV2>,
    // FIX: Add a field to hold the original, top-level lemma list from the DSL.
    #[serde(default)]
    pub dsl_lemmas: Vec<String>,
}

// --- Top-level block structures ---

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonSentenceBlock {
    pub s_id: String,
    pub tiers: Vec<JsonTierV2>,
    pub mappings: JsonMappingsV2,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "block_type")]
#[serde(rename_all = "snake_case")]
pub enum JsonContentBlock {
    ChapterMarker(JsonChapterMarkerBlock),
    Sentence(JsonSentenceBlock),
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonBookMetaV2 {
    pub book_name: String,
    pub schema_version: String,
    pub base_language: String,
    pub target_language: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonChapter {
    pub book_meta: JsonBookMetaV2,
    pub content_blocks: Vec<JsonContentBlock>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonTierV2 {
    pub tier_id: String,
    pub full_text: String,
    #[serde(default)]
    pub lemmas: Vec<String>,
    pub segments: Vec<JsonSegmentV2>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonMappingsV2 {
    #[serde(default)]
    pub simple_target_to_base_diglot: HashMap<String, Vec<(usize, String, String, bool)>>,
    #[serde(default)]
    pub adv_target_to_base_inv_diglot: HashMap<String, Vec<(usize, String, String)>>,
}