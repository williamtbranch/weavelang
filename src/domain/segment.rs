// src/domain/segment.rs

use crate::domain::token_stream::TokenStream;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// The segment ID (e.g., "S1", "A1").
    pub id: String,

    /// The content of this segment.
    pub stream: TokenStream,

    /// Lemmas specific to this segment (critical for L0 Engine logic).
    #[serde(default)]
    pub lemmas: Vec<String>,
}

impl Segment {
    pub fn new(id: String, text: &str, lemmas: Vec<String>) -> Self {
        Self {
            id,
            stream: TokenStream::new(text),
            lemmas,
        }
    }

    pub fn from_stream(id: String, stream: TokenStream, lemmas: Vec<String>) -> Self {
        Self { id, stream, lemmas }
    }

    pub fn full_text(&self) -> String {
        self.stream.full_text()
    }
}
