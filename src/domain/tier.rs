// src/domain/tier.rs

use crate::domain::segment::Segment;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TierState {
    Valid,      // Synced and clean (Green)
    Dirty,      // Manually edited, text does not match tokens (Yellow)
    Stale,      // Upstream dependency changed (Orange)
    Broken,     // Token indices invalid for mapping (Red)
}

impl Default for TierState {
    fn default() -> Self {
        TierState::Valid
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tier {
    pub id: String,
    pub segments: Vec<Segment>,
    
    #[serde(default)]
    pub lemmas: Vec<String>,

    // --- NEW FIELD ---
    #[serde(default)]
    pub state: TierState,
}

impl Tier {
    pub fn new(id: String) -> Self {
        Self {
            id,
            segments: Vec::new(),
            lemmas: Vec::new(),
            state: TierState::Valid,
        }
    }

    pub fn add_segment(&mut self, segment: Segment) {
        self.segments.push(segment);
    }

    pub fn full_text(&self) -> String {
        self.segments.iter()
            .map(|s| s.full_text())
            .collect::<Vec<_>>()
            .join("")
    }
}