// src/profile.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LemmaState { New, Active, Known }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LearnerLemmaInfo {
    pub state: LemmaState,
    // The `exposure_count` field has been removed.
}

impl Default for LearnerLemmaInfo {
    fn default() -> Self {
        Self { state: LemmaState::New }
    }
}