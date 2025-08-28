use serde::{Deserialize, Deserializer};
use std::collections::HashMap;

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonChapterMarkerBlock {
    pub text: String,
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

#[derive(Deserialize)]
struct SegmentOnDisk {
    seg_id: String,
    tokenized_text: Vec<JsonTokenV2>,
    #[serde(default)]
    lemmas: Vec<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(from = "SegmentOnDisk")]
pub struct JsonSegmentV2 {
    pub seg_id: String,
    pub text: String,
    pub tokenized_text: Vec<JsonTokenV2>,
    #[serde(default)]
    pub lemmas: Vec<String>,
}

impl From<SegmentOnDisk> for JsonSegmentV2 {
    fn from(temp: SegmentOnDisk) -> Self {
        let reconstructed_text = temp.tokenized_text.iter()
            .map(|token| token.value.as_str())
            .collect::<String>();
        
        JsonSegmentV2 {
            seg_id: temp.seg_id,
            text: reconstructed_text,
            tokenized_text: temp.tokenized_text,
            lemmas: temp.lemmas,
        }
    }
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
    pub simple_target_to_base_diglot: HashMap<String, Vec<(usize, Vec<String>, String, bool)>>,
    #[serde(default, rename = "simpler_adv_target_to_base_inv_diglot")]
    pub adv_target_to_base_inv_diglot: HashMap<String, Vec<(usize, Vec<String>, String)>>,
}