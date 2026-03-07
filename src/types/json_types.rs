// src/types/json_types.rs
use crate::simulation::numerical_types::{LLevelRecipe, VLevelRecipe};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonChapterMarkerBlock {
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum JsonTokenType {
    #[serde(rename = "b")]
    #[default]
    Background,
    #[serde(rename = "w")]
    Word,
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

// --- MODIFIED SECTION START ---
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TierId {
    // 'Simple' has been removed.
    Basic,
    Moderate,
    Advanced,
}
// --- MODIFIED SECTION END ---

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonSentenceBlock {
    pub s_id: String,
    pub tiers: Vec<JsonTierV2>,
    pub mappings: JsonMappingsV2,
    /// Lemmas flagged as proper nouns (from `{{…}}` markers in the forward
    /// diglot map).  Persisted so the weave algorithm can always treat
    /// these lemmas as known, regardless of frequency rank.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proper_noun_lemmas: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "block_type")]
#[serde(rename_all = "snake_case")]
pub enum JsonContentBlock {
    #[serde(rename = "chapter")]
    ChapterMarker(JsonChapterMarkerBlock),
    Sentence(JsonSentenceBlock),
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonBookMetaV2 {
    pub book_name: String,
    pub schema_version: String,
    pub base_language: String,
    pub target_language: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonTierV2 {
    pub tier_id: String,
    pub full_text: String,
    #[serde(default)]
    pub lemmas: Vec<String>,
    pub segments: Vec<JsonSegmentV2>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonMappingsV2 {
    #[serde(default, rename = "basic_spanish_to_basic_english_diglot")]
    pub basic_diglot: HashMap<String, Vec<(usize, Vec<String>, String, bool, usize, Vec<String>)>>,

    #[serde(default, rename = "basic_target_to_basic_base_inv_diglot")]
    pub basic_inverse_diglot: HashMap<String, Vec<(usize, Vec<String>, String, usize, usize)>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonCurriculumMapEntry {
    pub level: f32,
    pub start_sentence_idx: usize,
    pub recipe: VLevelRecipe,
    pub l_level_recipe: LLevelRecipe,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JsonCurriculumMap {
    pub end_level: f32,
    pub map: Vec<JsonCurriculumMapEntry>,
}

/// Wrapper for .lm (level map) files — includes metadata header.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LevelMapFile {
    pub meta: LevelMapMeta,
    pub levels: HashMap<String, JsonCurriculumMap>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LevelMapMeta {
    pub book_name: String,
    pub base_language: String,
    pub target_language: String,
    pub natural_peak_level: u32,
    pub peak_avd: f64,
    pub peak_user_score: f64,
    pub total_start_levels: u32,
    pub schema_version: String,
}
