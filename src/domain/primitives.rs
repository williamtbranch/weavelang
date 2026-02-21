// src/domain/primitives.rs

use serde::{Deserialize, Serialize};

/// A stable, unique identifier for a specific word instance within a TokenStream.
/// Using a dedicated type prevents confusion with array indices.
/// This allows us to modify the stream (insert/delete) without breaking external references (mappings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WordId(pub u64);

/// Represents the semantic content of a word token.
/// This separates the "what" (text/lemmas) from the "where" (position in stream).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordData {
    /// The unique stable ID for this word instance.
    pub id: WordId,
    
    /// The actual text displayed (e.g., "cats").
    pub text: String,
    
    /// The normalized lemmas (e.g., ["cat"]).
    pub lemmas: Vec<String>,
}

impl WordData {
    pub fn new(id: WordId, text: String, lemmas: Vec<String>) -> Self {
        Self { id, text, lemmas }
    }
}