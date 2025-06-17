// src/profile.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LemmaState { New, Active, Known }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)] 
pub struct LearnerLemmaInfo { 
    pub state: LemmaState, 
    pub exposure_count: u32, 
    // The `required_exposure_threshold` field has been removed from this struct.
}

impl Default for LearnerLemmaInfo { 
    fn default() -> Self { 
        Self { 
            state: LemmaState::New, 
            exposure_count: 0,
        }
    }
}