//*** START FILE: src/simulation/numerical_types.rs ***//
use std::collections::HashMap;
use crate::profile::{LearnerLemmaInfo, LemmaState};
use serde::{Serialize, Deserialize}; // <<< ADD THIS LINE

// --- Numerical Learner Profile ---
#[derive(Debug, Clone, Default, Serialize, Deserialize)] // <<< ADD Serialize, Deserialize
pub struct NumericalLearnerProfile {
    pub vocabulary: HashMap<u32, LearnerLemmaInfo>, 
}
// ... (rest of the file as it was) ...
// Ensure LearnerLemmaInfo and LemmaState also derive Serialize, Deserialize
// (They should already from src/profile.rs if that file was set up for serde)
//*** END FILE: src/simulation/numerical_types.rs ***//