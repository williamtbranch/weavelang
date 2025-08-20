use serde::{Deserialize, Deserializer}; // <-- Add Deserializer
use std::collections::HashMap;

// --- Re-usable child structs ---

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

// --- NEW HELPER STRUCT for custom deserialization ---
// This temporary struct matches the JSON structure on disk exactly.
#[derive(Deserialize)]
struct SegmentOnDisk {
    seg_id: String,
    tokenized_text: Vec<JsonTokenV2>,
    #[serde(default)]
    lemmas: Vec<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonSegmentV2 {
    pub seg_id: String,
    // This field will be computed during deserialization
    pub text: String,
    pub tokenized_text: Vec<JsonTokenV2>,
    #[serde(default)]
    pub lemmas: Vec<String>,
}

// --- NEW CUSTOM DESERIALIZATION FUNCTION ---
// This function tells Serde how to build a JsonSegmentV2
fn deserialize_segment_with_reconstructed_text<'de, D>(deserializer: D) -> Result<JsonSegmentV2, D::Error>
where
    D: Deserializer<'de>,
{
    // 1. Deserialize the JSON into our temporary struct that matches the file.
    let temp_segment = SegmentOnDisk::deserialize(deserializer)?;
    
    // 2. Compute the 'text' field by joining the token values.
    let reconstructed_text = temp_segment.tokenized_text.iter()
        .map(|token| token.value.as_str())
        .collect::<String>();

    // 3. Build and return the final JsonSegmentV2 struct with the computed field.
    Ok(JsonSegmentV2 {
        seg_id: temp_segment.seg_id,
        text: reconstructed_text, // <-- The computed value is placed here
        tokenized_text: temp_segment.tokenized_text,
        lemmas: temp_segment.lemmas,
    })
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
    // Use the custom deserializer for all segments
    #[serde(deserialize_with = "deserialize_segments_vec")]
    pub segments: Vec<JsonSegmentV2>,
}

// Helper function to apply the custom deserializer to a Vec of segments
fn deserialize_segments_vec<'de, D>(deserializer: D) -> Result<Vec<JsonSegmentV2>, D::Error>
where
    D: Deserializer<'de>,
{
    let temp_vec: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
    temp_vec.into_iter().map(|v| {
        serde_json::from_value(v).map_err(serde::de::Error::custom)
            .and_then(|temp_seg: SegmentOnDisk| {
                let text = temp_seg.tokenized_text.iter().map(|t| t.value.as_str()).collect();
                Ok(JsonSegmentV2 {
                    seg_id: temp_seg.seg_id,
                    text,
                    tokenized_text: temp_seg.tokenized_text,
                    lemmas: temp_seg.lemmas,
                })
            })
    }).collect()
}


#[derive(Deserialize, Debug, Clone, Default)]
pub struct JsonMappingsV2 {
    #[serde(default)]
    pub simple_target_to_base_diglot: HashMap<String, Vec<(usize, String, String, bool)>>,
    // Use serde(rename) to handle the name mismatch cleanly
    #[serde(default, rename = "simpler_adv_target_to_base_inv_diglot")]
    pub adv_target_to_base_inv_diglot: HashMap<String, Vec<(usize, String, String)>>,
}