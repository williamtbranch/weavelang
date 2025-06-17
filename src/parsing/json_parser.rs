// src/parsing/json_parser.rs
use crate::types::json_types::JsonChapter;
use std::error::Error;

/// Parses a JSON string into a `JsonChapter`.
pub fn parse_chapter_from_json(json_content: &str) -> Result<JsonChapter, Box<dyn Error>> {
    let chapter: JsonChapter = serde_json::from_str(json_content)?;
    Ok(chapter)
}