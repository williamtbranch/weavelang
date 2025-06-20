// src/lib.rs

// --- Declare all top-level modules ---
pub mod config;
pub mod corpus_generator; // For CLI mode
pub mod parsing;
pub mod profile;
pub mod profile_io;
pub mod simulation;
pub mod types;

// --- Re-exports for easier access from main.rs or other crates ---

// Config
pub use config::Config;

// Core data types (from JSON)
pub use types::json_types::{JsonChapter, JsonContentBlock, JsonSentenceBlock};

// Parser function
pub use parsing::json_parser::parse_chapter_from_json;

// Core profile types
pub use profile::{LearnerLemmaInfo, LemmaState};
pub use profile_io::{load_profile_snapshot, save_profile_snapshot, ProfileSnapshot};

// Simulation components
pub use simulation::dictionary::GlobalLemmaDictionary;
pub use simulation::numerical_types::{
    NumericalLearnerProfile, NumericalChapter, NumericalProcessedSentence,
};
pub use simulation::preprocessor::json_chapter_to_numerical;

// --- UPDATED EXPORTS FROM core_algo.rs ---
pub use simulation::core_algo::{
    determine_and_annotate_sentence_expression, // NEW function
    ChosenLevelOutput, // NEW struct
    OutputLevel,
};
// --- END UPDATED EXPORTS ---

pub use simulation::text_generator::generate_final_text_for_block_from_levels;

// Corpus generator function for CLI
pub use corpus_generator::{run_corpus_generation, GenerationArgs};