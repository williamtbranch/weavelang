//*** START FILE: src/corpus_generator.rs ***//
use crate::config::Config; // Assuming your config struct is named Config
use crate::profile_io::{load_profile_snapshot, save_profile_snapshot};
use crate::parsing::llm_parser; // Assuming this is how you access parse_llm_text_to_chapter
use crate::simulation::{
    dictionary::GlobalLemmaDictionary,
    numerical_types::{NumericalLearnerProfile, NumericalProcessedSentence},
    preprocessor,
    core_algo,
    text_generator,
};
use crate::profile::LemmaState; // For checking new words for activation list

use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::error::Error;
use std::io::BufRead; // For reading sequence file line by line

// Define a struct for CLI arguments related to generation,
// makes function signatures cleaner.
// You'll populate this from `clap` in main.rs or your CLI entry point.
#[derive(Debug, Clone)]
pub struct GenerationArgs {
    pub sequence_path: PathBuf,
    pub tts_output_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub start_profile_path: Option<PathBuf>,
    pub sentences_per_block: usize,
    pub max_regen_attempts_per_block: u32,
    pub target_ct_threshold: f32,
    pub max_words_to_activate_per_regen: usize,
    // Add other relevant params like config_path if not passed directly
}

pub fn run_corpus_generation(
    project_config: &Config, // Loaded from config.toml
    args: &GenerationArgs,
) -> Result<(), Box<dyn Error>> {
    println!("Starting corpus generation run...");

    // --- 1. Initialize Profile and Dictionary ---
    let mut learner_profile: NumericalLearnerProfile;
    let mut global_lemma_dictionary: GlobalLemmaDictionary;

    if let Some(start_profile_path) = &args.start_profile_path {
        println!("Attempting to load starting profile from: {}", start_profile_path.display());
        match load_profile_snapshot(start_profile_path) {
            Ok((loaded_profile, loaded_dict)) => {
                learner_profile = loaded_profile;
                global_lemma_dictionary = loaded_dict;
                println!("Successfully loaded starting profile and dictionary. Known words: {}", learner_profile.count_known());
            }
            Err(e) => {
                eprintln!("Error loading starting profile/dictionary: {}. Starting with empty profile and dictionary.", e);
                learner_profile = NumericalLearnerProfile::new();
                global_lemma_dictionary = GlobalLemmaDictionary::new();
            }
        }
    } else {
        learner_profile = NumericalLearnerProfile::new();
        global_lemma_dictionary = GlobalLemmaDictionary::new();
        println!("Starting with a new empty profile and dictionary.");
    }

    // Ensure output directories exist
    fs::create_dir_all(&args.tts_output_dir).map_err(|e| format!("Failed to create TTS output directory {:?}: {}", args.tts_output_dir, e))?;
    fs::create_dir_all(&args.profiles_dir).map_err(|e| format!("Failed to create profiles directory {:?}: {}", args.profiles_dir, e))?;

    // --- 2. Load Book Sequence ---
    let sequence_file = File::open(&args.sequence_path).map_err(|e| format!("Failed to open sequence file {:?}: {}", args.sequence_path, e))?;
    let reader = std::io::BufReader::new(sequence_file);
    let mut corpus_sequence: Vec<String> = Vec::new();
    for line_result in reader.lines() {
        let line = line_result.map_err(|e| format!("Failed to read line from sequence file: {}", e))?;
        let book_stem = line.trim();
        if !book_stem.is_empty() && !book_stem.starts_with('#') { // Ignore empty lines and comments
            corpus_sequence.push(book_stem.to_string());
        }
    }

    if corpus_sequence.is_empty() {
        println!("No book stems found in the sequence file. Exiting.");
        return Ok(());
    }
    println!("Processing sequence of {} book instance(s): {:?}", corpus_sequence.len(), corpus_sequence);

    let mut book_instance_counter: HashMap<String, usize> = HashMap::new();

    // --- 3. Iterate Through the Book Sequence ---
    for book_stem_orig in &corpus_sequence {
        let count = book_instance_counter.entry(book_stem_orig.clone()).or_insert(0);
        *count += 1;
        let book_instance_unique_id = format!("{}_inst{:02}", book_stem_orig, *count);
        
        println!("\n--- Processing book instance: {} (Original stem: {}) ---", book_instance_unique_id, book_stem_orig);

        // --- 3a. Save "_in.profile" for this instance ---
        let in_profile_filename = format!("{}_in.profile.json", book_instance_unique_id);
        let in_profile_path = args.profiles_dir.join(&in_profile_filename);
        if let Err(e) = save_profile_snapshot(&learner_profile, &global_lemma_dictionary, &in_profile_path) {
            eprintln!("  ERROR: Failed to save in-profile for {}: {}. Continuing without saving this snapshot.", book_instance_unique_id, e);
        } else {
            println!("  Saved in-profile to: {}", in_profile_path.display());
        }
        
        let learner_level_at_book_instance_start = learner_profile.count_known() / 100; // Integer division

        // --- 3b. Load and Parse .llm.txt file ---
        let llm_file_name = format!("{}.llm.txt", book_stem_orig);
        let llm_file_path = PathBuf::from(&project_config.content_project_dir)
            .join("stage") // Assuming .llm.txt files are in "project_config.content_project_dir/stage/"
            .join(&llm_file_name);

        let string_chapter = match fs::read_to_string(&llm_file_path) {
            Ok(content) => {
                match llm_parser::parse_llm_text_to_chapter(&llm_file_name, &content) {
                    Ok(ch) => ch,
                    Err(e) => {
                        eprintln!("  ERROR: Failed to parse {}: {}. Skipping this book instance.", llm_file_path.display(), e);
                        continue; 
                    }
                }
            }
            Err(e) => {
                eprintln!("  ERROR: Failed to read {}: {}. Skipping this book instance.", llm_file_path.display(), e);
                continue;
            }
        };

        // Convert to numerical, updating the global dictionary
        // Note: global_lemma_dictionary is cumulative across all book instances
        let numerical_chapter = preprocessor::to_numerical_chapter(&string_chapter, &mut global_lemma_dictionary);
        println!("  Parsed {} sentences for {}.", numerical_chapter.sentences_numerical.len(), book_instance_unique_id);


        // --- 3c. Process Book in Blocks ---
        let mut this_book_instance_output_text_segments: Vec<String> = Vec::new();
        let num_sentences_in_book = numerical_chapter.sentences_numerical.len();
        let mut current_sentence_idx_in_book = 0;
        let mut block_counter = 0;

        while current_sentence_idx_in_book < num_sentences_in_book {
            block_counter += 1;
            let end_block_idx_in_book = std::cmp::min(
                current_sentence_idx_in_book + args.sentences_per_block,
                num_sentences_in_book,
            );
            
            println!("    Processing block {} (sentences {} to {}) for {}.", 
                     block_counter, current_sentence_idx_in_book, end_block_idx_in_book -1, book_instance_unique_id);

            let current_block_numerical_sentences_refs: Vec<&NumericalProcessedSentence> =
                numerical_chapter.sentences_numerical[current_sentence_idx_in_book..end_block_idx_in_book].iter().collect();
            
            let current_block_string_sentences_refs: Vec<&crate::types::llm_data::ProcessedSentence> =
                string_chapter.sentences[current_sentence_idx_in_book..end_block_idx_in_book].iter().collect();

            if current_block_numerical_sentences_refs.is_empty() {
                break; 
            }
            
            // Prepare available_new_lemma_ids_for_activation for this specific block
            let mut block_new_lemma_freq: HashMap<u32, u32> = HashMap::new();
            for num_sentence_ref in &current_block_numerical_sentences_refs {
                let mut sentence_lemma_ids_for_freq_check: Vec<u32> = Vec::new();
                sentence_lemma_ids_for_freq_check.extend(&num_sentence_ref.adv_s_lemma_ids);
                for nsl in &num_sentence_ref.sim_s_lemmas_numerical {
                    sentence_lemma_ids_for_freq_check.extend(&nsl.lemma_ids);
                }
                for ndsm in &num_sentence_ref.diglot_map_numerical {
                    for nde in &ndsm.entries {
                        if nde.viable { sentence_lemma_ids_for_freq_check.push(nde.spa_lemma_id); }
                    }
                }
                for &lemma_id in &sentence_lemma_ids_for_freq_check {
                    // Check against the *current state* of the evolving learner_profile
                    if learner_profile.get_lemma_info(lemma_id).map_or(true, |info| info.state == LemmaState::New) {
                        *block_new_lemma_freq.entry(lemma_id).or_insert(0) += 1;
                    }
                }
            }
            let mut sorted_block_specific_new_lemma_ids_for_activation: Vec<(u32, u32)> = 
                block_new_lemma_freq.into_iter().collect();
            sorted_block_specific_new_lemma_ids_for_activation.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));


            match core_algo::run_simulation_numerical(
                &current_block_numerical_sentences_refs,
                learner_profile.clone(), // Pass a clone for the block's simulation cycle
                &sorted_block_specific_new_lemma_ids_for_activation,
                args.max_regen_attempts_per_block,
                args.target_ct_threshold,
                args.max_words_to_activate_per_regen,
            ) {
                Ok(block_simulation_result) => {
                    // Log CT for the block
                    println!("      Block {} CT: {:.2}%. Known: {}, Total Spanish: {}. Words Activated: {}. Regen Loops: {}.",
                             block_counter,
                             block_simulation_result.final_ct_for_block * 100.0,
                             block_simulation_result.known_lemmas_in_block,
                             block_simulation_result.total_spanish_lemmas_in_block,
                             block_simulation_result.profile_state_for_text_generation.count_active_only() - learner_profile.count_active_only(), // A bit approximative for "activated in this block"
                             block_simulation_result.simulation_log_entries.iter().filter(|s| s.contains("Regen Attempt:")).count()
                    );


                    match text_generator::generate_final_text_block(
                        &current_block_string_sentences_refs,
                        &global_lemma_dictionary,
                        &block_simulation_result.profile_state_for_text_generation, // Use this profile for text
                    ) {
                        Ok(generated_text_for_block) => {
                            if !generated_text_for_block.trim().is_empty() {
                                this_book_instance_output_text_segments.push(generated_text_for_block);
                            }
                        }
                        Err(e) => {
                            eprintln!("    ERROR: Text generation failed for block {} in {}: {}. Skipping text for this block.", block_counter, book_instance_unique_id, e);
                        }
                    }
                    // CRITICAL: Update the main, persistent learner_profile
                    learner_profile = block_simulation_result.profile_state_after_block_exposure;
                }
                Err(e) => {
                    eprintln!("    ERROR: Core simulation failed for block {} in {}: {}. Profile not updated for this block. Trying to continue.", block_counter, book_instance_unique_id, e);
                    // Decide if a block failure should halt the entire book or just skip the block.
                    // For now, we log and continue with the profile *before* this failed block.
                }
            }
            current_sentence_idx_in_book = end_block_idx_in_book;
        }

        // --- 3d. Record Ending Level & Save TTS Output Text File ---
        let learner_level_at_book_instance_end = learner_profile.count_known() / 100;
        let tts_filename_stem = format!(
            "{}_lvl{:02}_lvl{:02}",
            book_instance_unique_id, // Use unique ID for TTS file to match profiles
            learner_level_at_book_instance_start,
            learner_level_at_book_instance_end
        );
        let tts_output_file_path = args.tts_output_dir.join(format!("{}.txt", tts_filename_stem));
        
        // Join text segments with double newlines
        let final_tts_text = this_book_instance_output_text_segments.join("\n\n");
        match fs::write(&tts_output_file_path, final_tts_text) {
            Ok(_) => println!("  Saved TTS input to: {}", tts_output_file_path.display()),
            Err(e) => eprintln!("  ERROR: Failed to write TTS input file {}: {}", tts_output_file_path.display(), e),
        }

        // --- 3e. Save "_out.profile" for this instance ---
        let out_profile_filename = format!("{}_out.profile.json", book_instance_unique_id);
        let out_profile_path = args.profiles_dir.join(&out_profile_filename);
        if let Err(e) = save_profile_snapshot(&learner_profile, &global_lemma_dictionary, &out_profile_path) {
             eprintln!("  ERROR: Failed to save out-profile for {}: {}. Profile state for next book might be inaccurate if run is interrupted here.", book_instance_unique_id, e);
        } else {
            println!("  Saved out-profile to: {}", out_profile_path.display());
        }
        println!("  Finished book instance: {}. Profile Known Words: {}", book_instance_unique_id, learner_profile.count_known());
    }

    println!("\nCorpus generation run finished.");
    Ok(())
}
//*** END FILE: src/corpus_generator.rs ***//