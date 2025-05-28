//*** START FILE: src/simulation/mod.rs ***//
pub mod dictionary;
pub mod numerical_types;
pub mod preprocessor;
pub mod core_algo;
pub mod text_generator;

// Re-export key items that main.rs and other top-level modules might use
pub use dictionary::GlobalLemmaDictionary;
pub use numerical_types::{
    NumericalLearnerProfile, // Assuming this will be the primary profile type used in simulation
    // Add other numerical_types structs here if they need to be directly accessed,
    // e.g., NumericalChapter, but often these are intermediate.
};
pub use preprocessor::to_numerical_chapter; // Function to convert string data to numerical

// The core simulation function might return a result struct, which could also be exported
// pub use core_algo::SimulationBlockResult; 
pub use core_algo::run_simulation_numerical;

// The final text generation function
pub use text_generator::generate_final_text_block;
//*** END FILE: src/simulation/mod.rs ***//