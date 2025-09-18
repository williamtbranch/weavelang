// In src/lib.rs

pub mod config;
pub mod corpus_generator;
pub mod parsing;
pub mod profile;
pub mod profile_io;
pub mod simulation;
pub mod types;

pub use config::Config;
pub use types::json_types::{JsonChapter, JsonContentBlock, JsonSentenceBlock};
pub use parsing::json_parser::parse_chapter_from_json;
pub use profile::{LearnerLemmaInfo, LemmaState};
pub use profile_io::{load_profile_snapshot, save_profile_snapshot, ProfileSnapshot};
pub use simulation::dictionary::GlobalLemmaDictionary;
pub use simulation::numerical_types::{
    NumericalLearnerProfile, NumericalChapter, NumericalProcessedSentence,
};
pub use simulation::preprocessor::json_chapter_to_numerical;
pub use simulation::core_algo::{
    determine_and_annotate_sentence_expression,
    ChosenLevelOutput,
    OutputLevel,
};
pub use simulation::text_generator::generate_raw_text_from_levels;
pub use corpus_generator::{run_corpus_generation};

// --- THIS SECTION IS UPDATED ---
pub use simulation::avd_hunter;
// We now export our new unified function from the calibrator module.
pub use simulation::calibrator::{self, run_unified_calibration};