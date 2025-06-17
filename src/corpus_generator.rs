// src/corpus_generator.rs
use crate::config::Config;
use crate::profile::LemmaState;
use crate::profile_io::{load_profile_snapshot, save_profile_snapshot};
use crate::parsing::json_parser;
use crate::simulation::{
    core_algo::{self, OutputLevel}, // <-- ADDED `OutputLevel` to imports
    dictionary::GlobalLemmaDictionary, 
    numerical_types::NumericalLearnerProfile,
    preprocessor, 
    text_generator,
};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::fs::{File, OpenOptions}; // <-- ADDED `OpenOptions`
use std::io::{BufRead, Write}; // <-- ADDED `Write`
use std::path::{Path, PathBuf}; // <-- ADDED `Path`

// --- NEW FUNCTION: log_analysis_to_file ---
// We add this new helper function to the file.
fn log_analysis_to_file(
    log_file_path: &Path,
    book_instance_unique_id: &str,
    stats: &Vec<(OutputLevel, usize)>,
    total_sentences: usize,
    profile_after_book: &NumericalLearnerProfile,
) -> Result<(), std::io::Error> {
    // Use OpenOptions to create the file if it doesn't exist and to append to it.
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)?;

    // Get a timestamp for this log entry
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    writeln!(file, "--- Analysis for Book Instance: {} (at {}) ---", book_instance_unique_id, timestamp)?;
    
    let total_sentences_float = total_sentences as f32;
    if total_sentences_float > 0.0 {
        // We can reuse the same logic as the console printout for consistency
        let l0_count = stats.iter().find(|(l, _)| *l == OutputLevel::L0).map_or(0, |(_, c)| *c);
        let l1_count = stats.iter().find(|(l, _)| *l == OutputLevel::L1).map_or(0, |(_, c)| *c);
        let l2_count = stats.iter().find(|(l, _)| *l == OutputLevel::L2).map_or(0, |(_, c)| *c);
        let l3_count = stats.iter().find(|(l, _)| *l == OutputLevel::L3).map_or(0, |(_, c)| *c);
        let l4_count = stats.iter().find(|(l, _)| *l == OutputLevel::L4).map_or(0, |(_, c)| *c);
        let l5_count = stats.iter().find(|(l, _)| *l == OutputLevel::L5).map_or(0, |(_, c)| *c);
        let l6_count = stats.iter().find(|(l, _)| *l == OutputLevel::L6).map_or(0, |(_, c)| *c);

        writeln!(file, "  Level Distribution:")?;
        writeln!(file, "    L0 (Adv Full):        {:>5} sentences ({:>6.2}%)", l0_count, (l0_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L1 (Adv Woven):       {:>5} sentences ({:>6.2}%)", l1_count, (l1_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L2 (SimplerAdv Full): {:>5} sentences ({:>6.2}%)", l2_count, (l2_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L3 (Simp Full):       {:>5} sentences ({:>6.2}%)", l3_count, (l3_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L4 (Simp Woven):      {:>5} sentences ({:>6.2}%)", l4_count, (l4_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L5 (Diglot):          {:>5} sentences ({:>6.2}%)", l5_count, (l5_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L6 (English):         {:>5} sentences ({:>6.2}%)", l6_count, (l6_count as f32 / total_sentences_float) * 100.0)?;
    } else {
        writeln!(file, "  No sentences processed for this instance.")?;
    }

    writeln!(file, "  Profile State at End:")?;
    writeln!(file, "    Known Lemmas: {}", profile_after_book.count_known())?;
    writeln!(file, "    Active Lemmas: {}", profile_after_book.count_active_only())?;
    writeln!(file, "    Total K/A:     {}", profile_after_book.count_total_known_or_active())?;
    writeln!(file, "---------------------------------------------------------\n")?; // Add newlines for spacing

    Ok(())
}


#[derive(Debug, Clone)]
pub struct GenerationArgs {
    pub sequence_path: PathBuf,
    pub input_json_dir: PathBuf,
    pub tts_output_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub start_profile_path: Option<PathBuf>,
    pub sentences_per_block: usize,
    pub max_regen_attempts_per_block: u32,
    pub target_ct_threshold: f32,
    pub max_words_to_activate_per_regen: usize,
    pub words_per_level: u32,
}

pub fn run_corpus_generation(
    project_config: &Config,
    args: &GenerationArgs,
) -> Result<(), Box<dyn Error>> {
    println!("Starting corpus generation run (from JSON input)...");

    // ... (The first part of the function is unchanged) ...
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

    fs::create_dir_all(&args.tts_output_dir)?;
    fs::create_dir_all(&args.profiles_dir)?;

    let sequence_file = File::open(&args.sequence_path)?;
    let reader = std::io::BufReader::new(sequence_file);
    let mut corpus_sequence: Vec<String> = Vec::new();
    for line_result in reader.lines() {
        let line = line_result?;
        let book_stem = line.trim();
        if !book_stem.is_empty() && !book_stem.starts_with('#') {
            corpus_sequence.push(book_stem.to_string());
        }
    }
    if corpus_sequence.is_empty() {
        println!("No book stems in sequence file. Exiting.");
        return Ok(());
    }
    println!("Processing sequence of {} book instance(s): {:?}", corpus_sequence.len(), corpus_sequence);

    // --- NEW: Define the path for our analysis log file ---
    let analysis_log_path = args.profiles_dir.join("corpus_analysis_log.txt");
    println!("Analysis log will be written to: {}", analysis_log_path.display());


    let mut book_instance_counter: HashMap<String, usize> = HashMap::new();

    for book_stem_orig in &corpus_sequence {
        let count = book_instance_counter.entry(book_stem_orig.clone()).or_insert(0);
        *count += 1;
        let book_instance_unique_id = format!("{}_inst{:02}", book_stem_orig, *count);
        
        println!("\n--- Processing book instance: {} (Original stem: {}) ---", book_instance_unique_id, book_stem_orig);
        
        // --- ADDED: Create an aggregator for book-level stats ---
        let mut book_level_stats: HashMap<OutputLevel, usize> = HashMap::new();

        // ... (The rest of the logic for saving in-profile, loading JSON, etc. is unchanged) ...
        let in_profile_filename = format!("{}_in.profile.json", book_instance_unique_id);
        let in_profile_path = args.profiles_dir.join(&in_profile_filename);
        if let Err(e) = save_profile_snapshot(&learner_profile, &global_lemma_dictionary, &in_profile_path) {
            eprintln!("  ERROR: Failed to save in-profile for {}: {}", book_instance_unique_id, e);
        } else {
            println!("  Saved in-profile to: {}", in_profile_path.display());
        }

        let words_per_level_val = if args.words_per_level > 0 { args.words_per_level } else { 100 };
        let learner_level_at_book_start = learner_profile.count_total_known_or_active() / 100;


        let json_file_name = format!("{}.stage7.json", book_stem_orig);
        let json_file_path = PathBuf::from(&project_config.content_project_dir)
            .join(&args.input_json_dir)
            .join(&json_file_name);

        let json_content_str = match fs::read_to_string(&json_file_path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("  ERROR: Failed to read {}: {}. Skipping book instance.", json_file_path.display(), e);
                continue;
            }
        };

        let json_chapter = match json_parser::parse_chapter_from_json(&json_content_str) {
            Ok(ch) => ch,
            Err(e) => {
                eprintln!("  ERROR: Failed to parse {}: {}. Skipping book instance.", json_file_path.display(), e);
                continue;
            }
        };
        
        global_lemma_dictionary.populate_from_json_chapter(&json_chapter);

        let numerical_chapter = preprocessor::json_chapter_to_numerical(&json_chapter, &mut global_lemma_dictionary);
        println!("  Parsed {} sentences for {}.", numerical_chapter.sentences_numerical.len(), book_instance_unique_id);

        if numerical_chapter.sentences_numerical.is_empty() {
            eprintln!("  Warning: No sentences found after parsing and converting {} for {}. Skipping TTS file generation.", json_file_name, book_instance_unique_id);
            continue;
        }
        
        let all_string_sentence_blocks: Vec<_> = json_chapter.content_blocks.iter().filter_map(|cb| match cb {
            crate::types::json_types::JsonContentBlock::Sentence(s) => Some(s),
            _ => None,
        }).collect();


        let mut this_book_instance_tts_text_parts: Vec<String> = Vec::new();
        let num_sentences_in_book = numerical_chapter.sentences_numerical.len();
        let mut current_sentence_idx_in_book = 0;
        let mut block_counter = 0;

        while current_sentence_idx_in_book < num_sentences_in_book {
            block_counter += 1;
            let end_block_idx_in_book = std::cmp::min(
                current_sentence_idx_in_book + args.sentences_per_block,
                num_sentences_in_book,
            );

            println!("    Processing block {} (sentences {}-{}) for {}.", 
                     block_counter, current_sentence_idx_in_book, end_block_idx_in_book - 1, book_instance_unique_id);
            
            let cur_block_numerical_sentences_refs: Vec<_> = 
                numerical_chapter.sentences_numerical[current_sentence_idx_in_book..end_block_idx_in_book].iter().collect();
            
            let cur_block_string_sentences_slice: &[&crate::types::json_types::JsonSentenceBlock] = 
                &all_string_sentence_blocks[current_sentence_idx_in_book..end_block_idx_in_book];
                
            if cur_block_numerical_sentences_refs.is_empty() { break; }

            let mut block_new_lemma_freq: HashMap<u32, u32> = HashMap::new();
            for num_sentence_ref in &cur_block_numerical_sentences_refs {
                let mut temp_ids_for_freq: HashSet<u32> = HashSet::new();
                temp_ids_for_freq.extend(&num_sentence_ref.adv_sl_overall_lemma_ids);
                temp_ids_for_freq.extend(&num_sentence_ref.simpler_adv_sl_overall_lemma_ids);
                for bundle in &num_sentence_ref.adv_segment_bundles_numerical {
                    temp_ids_for_freq.extend(&bundle.adv_lemma_ids);
                    temp_ids_for_freq.extend(&bundle.simpler_lemma_ids);
                }
                for l3_sl_seg in &num_sentence_ref.l3_simsl_per_segment_numerical {
                    temp_ids_for_freq.extend(&l3_sl_seg.lemma_ids);
                }
                for diglot_seg_map in &num_sentence_ref.diglot_map_numerical {
                    for entry in &diglot_seg_map.entries {
                        if entry.viable { temp_ids_for_freq.insert(entry.spa_lemma_id); }
                    }
                }
                for &lemma_id in &temp_ids_for_freq {
                    if lemma_id != u32::MAX && learner_profile.get_lemma_info(lemma_id).map_or(true, |info| info.state == LemmaState::New) {
                        *block_new_lemma_freq.entry(lemma_id).or_insert(0) += 1;
                    }
                }
            }
            let mut sorted_block_specific_new_lemma_ids_for_activation: Vec<(u32, u32)> = 
                block_new_lemma_freq.into_iter().collect();
            sorted_block_specific_new_lemma_ids_for_activation.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

            match core_algo::run_simulation_numerical(
                &cur_block_numerical_sentences_refs, 
                learner_profile.clone(), 
                &global_lemma_dictionary,
                &sorted_block_specific_new_lemma_ids_for_activation,
                args.max_regen_attempts_per_block,
                args.target_ct_threshold,
                args.max_words_to_activate_per_regen,
            ) {
                Ok(block_simulation_result) => {
                    println!("      Block {} CT: {:.2}%. Known: {}, Total Spanish: {}. Words Newly Activated: {}. Regen Loops: {}.",
                             block_counter,
                             block_simulation_result.final_ct_for_block * 100.0,
                             block_simulation_result.known_and_active_lemmas_in_block, 
                             block_simulation_result.total_spanish_lemmas_in_block,
                             block_simulation_result.activated_lemma_ids_this_block_run.len(),
                             block_simulation_result.simulation_log_entries.iter().filter(|s| s.contains("Regen Attempt:")).count()
                    );
                    
                    // --- ADDED: Aggregate stats from the block result ---
                    for (level, count) in block_simulation_result.level_stats.iter() {
                        *book_level_stats.entry(level.clone()).or_insert(0) += count;
                    }

                    match text_generator::generate_final_text_for_block_from_levels(
                        cur_block_string_sentences_slice,
                        &block_simulation_result.chosen_level_outputs_for_sentences,
                    ) {
                        Ok(generated_text) => {
                            if !generated_text.trim().is_empty() {
                                this_book_instance_tts_text_parts.push(generated_text);
                            }
                        }
                        Err(e) => {
                            eprintln!("    ERROR: Text generation failed for block {}: {}", block_counter, e);
                        }
                    }
                    learner_profile = block_simulation_result.profile_state_after_block_exposure;
                }
                Err(e) => {
                    eprintln!("    ERROR: Core simulation failed for block {}: {}. Profile not updated.", block_counter, e);
                }
            }
            current_sentence_idx_in_book = end_block_idx_in_book; 
        }
        
        // --- ADDED: The entire analysis block, now with file logging ---
        println!("  --- Book Instance Analysis for {} ---", book_instance_unique_id);
        let total_sentences_in_book_float = num_sentences_in_book as f32;
        let mut sorted_stats: Vec<_> = book_level_stats.into_iter().collect();
        // Sort by a numeric representation of the level for consistent order
        sorted_stats.sort_by_key(|(level, _)| *level as u8);

        if total_sentences_in_book_float > 0.0 {
            println!("    Level Distribution:");
            for (level, count) in &sorted_stats {
                let percentage = (*count as f32 / total_sentences_in_book_float) * 100.0;
                // A helper to get the label for each level
                let level_label = match level {
                    OutputLevel::L0 => "L0 (Adv Full)",
                    OutputLevel::L1 => "L1 (Adv Woven)",
                    OutputLevel::L2 => "L2 (SimplerAdv Full)",
                    OutputLevel::L3 => "L3 (Simp Full)",
                    OutputLevel::L4 => "L4 (Simp Woven)",
                    OutputLevel::L5 => "L5 (Diglot)",
                    OutputLevel::L6 => "L6 (English)",
                };
                println!("      {:<20}: {:>5} sentences ({:>6.2}%)", level_label, count, percentage);
            }
        }
        println!("  ----------------------------------------");
        
        // --- ADDED: Call the new logging function ---
        if let Err(e) = log_analysis_to_file(&analysis_log_path, &book_instance_unique_id, &sorted_stats, num_sentences_in_book, &learner_profile) {
            eprintln!("  Warning: Failed to write to analysis log file: {}", e);
        }


        // ... (The rest of the function for saving TTS and out-profile is unchanged) ...
        let learner_level_at_book_end = (learner_profile.count_total_known_or_active() as u32 / words_per_level_val) as usize;
        let tts_filename_stem = format!(
            "{}_L{:02}_to_L{:02}",
            book_instance_unique_id,
            learner_level_at_book_start,
            learner_level_at_book_end
        );
        let tts_output_file_path = args.tts_output_dir.join(format!("{}.txt", tts_filename_stem));
        
        let final_tts_text = this_book_instance_tts_text_parts.join("\n\n");
        if !final_tts_text.trim().is_empty() {
            fs::write(&tts_output_file_path, final_tts_text)?;
            println!("  Saved TTS input to: {}", tts_output_file_path.display());
        } else {
            println!("  Warning: No text generated for TTS file for {}.", book_instance_unique_id);
        }

        let out_profile_filename = format!("{}_out.profile.json", book_instance_unique_id);
        let out_profile_path = args.profiles_dir.join(&out_profile_filename);
        save_profile_snapshot(&learner_profile, &global_lemma_dictionary, &out_profile_path)?;
        println!("  Saved out-profile to: {}", out_profile_path.display());
        println!("  Finished book instance: {}. Profile Known Words: {}. Total K/A: {}", 
                 book_instance_unique_id, learner_profile.count_known(), learner_profile.count_total_known_or_active());
    }

    println!("\nCorpus generation run finished.");
    Ok(())
}