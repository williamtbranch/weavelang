//*** START FILE: src/parsing/mod.rs ***//
pub mod llm_parser;

// Re-export the main parsing function for convenience
pub use llm_parser::parse_llm_text_to_chapter;
//*** END FILE: src/parsing/mod.rs ***//