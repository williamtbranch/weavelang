//*** START FILE: src/lib.rs ***//

// Declare all modules that are part of this library
pub mod config;
pub mod types {
    pub mod llm_data;
}
pub mod parsing {
    pub mod llm_parser;
}
pub mod simulation {
    pub mod dictionary;
    pub mod numerical_types;
    pub mod preprocessor;
    pub mod core_algo;
    pub mod text_generator;
}
pub mod profile;
pub mod profile_io;       // We added this
pub mod corpus_generator; // We added this

// You might also choose to re-export key items for convenience if main.rs
// or other external crates were to use this library, e.g.:
// pub use config::Config;
// pub use types::llm_data::ProcessedChapter;
// etc.
// For now, just declaring the modules should be enough for internal use.

//*** END FILE: src/lib.rs ***//