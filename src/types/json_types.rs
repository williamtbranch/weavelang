// src/types/json_types.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::simulation::numerical_types::VLevelRecipe;

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonChapterMarkerBlock {
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonTokenV2 {
    #[serde(rename = "t")]
    pub token_type: JsonTokenType,
    #[serde(rename = "v")]
    pub value: String,
    #[serde(default, rename = "di")]
    pub diglot_index: Option<usize>,
    #[serde(default, rename = "l")]
    pub lemmas: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_pn: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonSegmentV2 {
    pub seg_id: String,
    pub text: String,
    #[serde(default)]
    pub tokenized_text: Vec<JsonTokenV2>,
    #[serde(default)]
    pub lemmas: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TierId {
    Simple, // Note: This is now unused but kept for historical/parsing safety if needed
    Basic,
    Moderate,
    Advanced,
}

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
    #[serde(rename = "chapter")]
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
    #[serde(default)]
    pub u_level_maps: HashMap<String, JsonCurriculumMap>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonChapterForParsing {
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

// --- THIS STRUCT IS THE PRIMARY CHANGE IN THIS FILE ---
#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonMappingsV2 {
    // Renamed to reflect that this maps between the 'basic' base and target tiers
    #[serde(default, rename = "basic_spanish_to_basic_english_diglot")]
    pub basic_diglot: HashMap<String, Vec<(usize, Vec<String>, String, bool, usize, Vec<String>)>>,

    // Renamed for clarity and to reflect its source/target
    #[serde(default, rename = "basic_target_to_basic_base_inv_diglot")]
    pub basic_inverse_diglot: HashMap<String, Vec<(usize, Vec<String>, String, usize, usize)>>,
}
// --- END OF PRIMARY CHANGE ---

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonCurriculumMapEntry {
    pub level: f32,
    pub start_sentence_idx: usize,
    pub recipe: VLevelRecipe,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonCurriculumMap {
    pub end_level: f32,
    pub map: Vec<JsonCurriculumMapEntry>,
}