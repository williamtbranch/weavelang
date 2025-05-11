mod generation;
mod parser; // Ensure this module is declared
mod profile; // Ensure this module is declared

use crate::generation::generate_woven_text_for_chapter;
use crate::parser::load_and_parse_chapter_data; // Should now resolve if parser.rs has this pub fn
use crate::profile::LearnerProfile;


fn main() {
    println!("WeaveLang CLI (Rust Prototype) - Starting...");

    let mut content_path_display = "Config: Using hardcoded paths for now".to_string();
    println!("Content Path (from config/default): {}", content_path_display);


    // --- 2. Load Learner Profile ---
    // LearnerProfile::load_profile should now exist in profile.rs
    let mut learner_profile = LearnerProfile::load_profile("profiles/default_profile.json") 
        .unwrap_or_else(|err| {
        eprintln!("Warning: Could not load profile (profiles/default_profile.json): {}. Initializing new profile.", err);
        LearnerProfile::new() 
    });
    
    // Example: Add a known lemma to the profile for testing L1/L2
    // use crate::profile::{LearnerLemmaInfo, LemmaState}; // Add to top if not already there
    // learner_profile.vocabulary.insert("lemmaC".to_string(), LearnerLemmaInfo {
    //     lemma: "lemmaC".to_string(),
    //     state: LemmaState::Known,
    //     exposure_count: 20,
    //     required_threshold: 20,
    // });
    // learner_profile.vocabulary.insert("lemmaD".to_string(), LearnerLemmaInfo {
    //     lemma: "lemmaD".to_string(),
    //     state: LemmaState::Active,
    //     exposure_count: 5,
    //     required_threshold: 20,
    // });


    // --- 3. Load Content Data (e.g., one chapter) ---
    let chapter_file_path = "data/example_chapter.json"; // Ensure this file exists or load_and_parse_chapter_data can handle it
    content_path_display = chapter_file_path.to_string();

    match load_and_parse_chapter_data(chapter_file_path) {
        Ok(processed_chapter) => {
            println!("Successfully loaded and parsed chapter data from: {}", content_path_display);

            if processed_chapter.sentences.is_empty() || processed_chapter.sentence_order.is_empty() {
                 println!("Warning: Loaded chapter data but it appears to be empty. No text will be generated.");
            } else {
                // --- 4. Generate Woven Text ---
                // Pass a threshold_low_vocab value, e.g.
                match generate_woven_text_for_chapter(&processed_chapter, &mut learner_profile, 500 /*example threshold_low_vocab*/) {
                    Ok(generated_text_bundle) => { 
                        println!("\n--- Generated Woven Text ---");
                        println!("{}", generated_text_bundle); 
                        println!("--- End of Generated Text ---");

                        // --- 6. (Optional) Save Profile ---
                        if let Err(e) = learner_profile.save_profile("profiles/default_profile_updated.json") {
                            eprintln!("Failed to save learner profile: {}", e);
                        } else {
                            println!("Successfully saved updated profile to profiles/default_profile_updated.json");
                        }
                    }
                    Err(e) => {
                        eprintln!("\nError during text generation: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to load or parse chapter data from {}: {}", content_path_display, e);
        }
    }

    println!("\nWeaveLang CLI finished.");
}
