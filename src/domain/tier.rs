// src/domain/tier.rs

use crate::domain::llm_log::LlmCallRecord;
use crate::domain::segment::Segment;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TierState {
    #[default]
    Valid, // Synced and clean (Green)
    Dirty,  // Manually edited, text does not match tokens (Yellow)
    Stale,  // Upstream dependency changed (Orange)
    Broken, // Token indices invalid for mapping (Red)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tier {
    pub id: String,
    pub segments: Vec<Segment>,

    #[serde(default)]
    pub lemmas: Vec<String>,

    #[serde(default)]
    pub state: TierState,

    /// Full history of every LLM call that targeted this tier for this sentence.
    #[serde(default)]
    pub llm_log: Vec<LlmCallRecord>,
}

impl Tier {
    pub fn new(id: String) -> Self {
        Self {
            id,
            segments: Vec::new(),
            lemmas: Vec::new(),
            state: TierState::Valid,
            llm_log: Vec::new(),
        }
    }

    pub fn add_segment(&mut self, segment: Segment) {
        self.segments.push(segment);
    }

    pub fn full_text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.full_text())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Ensures that between adjacent segments, there is a space separator.
    /// Checks trailing B of segment[i] and leading B of segment[i+1];
    /// if neither contains whitespace, prepends a space to segment[i+1]'s leading B.
    pub fn ensure_inter_segment_spacing(&mut self) {
        for i in 0..self.segments.len().saturating_sub(1) {
            let trailing = self.segments[i].stream.trailing_background().to_string();
            let leading = self.segments[i + 1].stream.leading_background().to_string();

            let trailing_has_ws = trailing.chars().any(|c| c.is_whitespace());
            let leading_has_ws = leading.chars().any(|c| c.is_whitespace());

            if !trailing_has_ws && !leading_has_ws {
                // Prepend a space to the next segment's leading background
                let new_leading = format!(" {}", leading);
                self.segments[i + 1].stream.set_leading_background(new_leading);
            }
        }
    }
}
