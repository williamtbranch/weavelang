// In src/corpus_generator.rs

use crate::config::Config;
use crate::simulation::{
    core_algo::{self, L0SegmentChoice, OutputLevel},
    dictionary::GlobalLemmaDictionary,
    frequency_manager,
    numerical_types::NumericalLearnerProfile,
    preprocessor, text_generator,
};
use crate::{parsing::json_parser, types::json_types::JsonContentBlock};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum SegmentType {
    AdvancedSpanish,
    ModerateSpanish,
    // SimpleSpanish, // <-- REMOVE
    EnglishDiglot,
    // English,       // <-- REMOVE
    InverseDiglot,
}

fn log_analysis_to_file(
    log_file_path: &Path,
    book_instance_unique_id: &str,
    level_stats: &HashMap<OutputLevel, usize>,
    segment_stats: &HashMap<SegmentType, usize>,
    total_sentences: usize,
    final_profile: &NumericalLearnerProfile,
    total_spanish_words: usize,
    total_english_words: usize,
) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)?;
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let total_output_words = total_spanish_words + total_english_words;
    let english_percentage = if total_output_words > 0 {
        (total_english_words as f32 / total_output_words as f32) * 100.0
    } else {
        0.0
    };

    writeln!(
        file,
        "--- Analysis for Book Instance: {} (at {}) ---",
        book_instance_unique_id, timestamp
    )?;
    
    writeln!(file, "  Output Word Count Summary:")?;
    writeln!(file, "    Total Target Words:  {:>5}", total_spanish_words)?; // Language-agnostic term
    writeln!(file, "    Total Base Words:    {:>5}", total_english_words)?; // Language-agnostic term
    writeln!(file, "    -------------------------")?;
    writeln!(file, "    Total Output Words:  {:>5}", total_output_words)?;
    writeln!(
        file,
        "    Base Lang Pct:       {:>5.2}%", // Language-agnostic term
        english_percentage
    )?;

    let total_sentences_float = total_sentences as f32;
    if total_sentences_float > 0.0 {
        writeln!(file, "\n  Sentence Level Distribution:")?;
        let get_level_count = |level: OutputLevel| -> usize { *level_stats.get(&level).unwrap_or(&0) };
        let l0_count = get_level_count(OutputLevel::AdvancedWeave);
        let l1_count = get_level_count(OutputLevel::SimpleHybrid);
        writeln!(
            file,
            "    L0 Advanced Weave: {:>5} sentences ({:>6.2}%)",
            l0_count,
            (l0_count as f32 / total_sentences_float) * 100.0
        )?;
        writeln!(
            file,
            "    L1 Simple Hybrid:  {:>5} sentences ({:>6.2}%)",
            l1_count,
            (l1_count as f32 / total_sentences_float) * 100.0
        )?;
    } else {
        writeln!(file, "  No sentences processed for this instance.")?;
    }
    
    let total_segments = segment_stats.values().sum::<usize>();
    if total_segments > 0 {
        writeln!(
            file,
            "\n  Segment Type Distribution (Total: {} segments):",
            total_segments
        )?;
        let get_segment_count =
            |seg_type: SegmentType| -> usize { *segment_stats.get(&seg_type).unwrap_or(&0) };
        let as_count = get_segment_count(SegmentType::AdvancedSpanish);
        let ms_count = get_segment_count(SegmentType::ModerateSpanish);
        let id_count = get_segment_count(SegmentType::InverseDiglot);
        // let ss_count = get_segment_count(SegmentType::SimpleSpanish); // <-- REMOVE
        let ed_count = get_segment_count(SegmentType::EnglishDiglot);
        // let en_count = get_segment_count(SegmentType::English);       // <-- REMOVE
        let total_segments_float = total_segments as f32;
        
        // --- Renamed for language agnosticism ---
        writeln!(
            file,
            "    Adv. Target Segments:  {:>5} segments ({:>6.2}%)", // Adv. Spanish
            as_count,
            (as_count as f32 / total_segments_float) * 100.0
        )?;
        writeln!(
            file,
            "    Simpler Target Segs:   {:>5} segments ({:>6.2}%)", // Mod. Spanish
            ms_count,
            (ms_count as f32 / total_segments_float) * 100.0
        )?;
        writeln!(
            file,
            "    Inv. Diglot Segments:  {:>5} segments ({:>6.2}%)", // Inv. Diglot
            id_count,
            (id_count as f32 / total_segments_float) * 100.0
        )?;
        writeln!(
            file,
            "    Base Diglot Segments:  {:>5} segments ({:>6.2}%)", // Eng. Diglot
            ed_count,
            (ed_count as f32 / total_segments_float) * 100.0
        )?;
    }
    
    writeln!(file, "\n  Final Profile State:")?;
    writeln!(
        file,
        "    Activated Lemmas: {}",
        final_profile.vocabulary.len()
    )?;
    writeln!(
        file,
        "----------------------------------------------------------------------\n"
    )?;
    Ok(())
}


#[derive(Debug, Clone)]
pub struct GenerationArgs {
    pub tool_root_dir: PathBuf,
    pub sequence_path: PathBuf,
    pub input_json_dir: PathBuf,
    pub tts_output_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub start_level: u32,
    pub ramp_rate: f32,
    pub words_per_level: u32,
    pub core_vocab_size: u32,
    pub stretch_threshold: f32,
    pub max_compression_ratio: f32,
    pub debug_markers: bool,
    pub inverse_diglot_threshold: f32,
}

struct ProcessingState {
    current_start_level: u32,
    current_ramp_rate: f32,
}

pub fn run_corpus_generation(
    project_config: &Config,
    args: &GenerationArgs,
) -> Result<(), Box<dyn Error>> {
    let freq_list_path = args
        .tool_root_dir
        .join("assets")
        .join("frequency_lists")
        .join("es_master_frequency_list.txt");

    println!(
        "[DEBUG] Attempting to load frequency list from: {:?}",
        &freq_list_path
    );
    frequency_manager::load_master_frequency_list(&freq_list_path)?;
    let ordered_lemmas = frequency_manager::get_ordered_lemmas();
    println!("[DEBUG] Frequency list loaded successfully.");

    fs::create_dir_all(&args.tts_output_dir)?;
    fs::create_dir_all(&args.profiles_dir)?;

    let analysis_log_path = args.profiles_dir.join("corpus_analysis_log.txt");

    let mut state = ProcessingState {
        current_start_level: args.start_level,
        current_ramp_rate: args.ramp_rate,
    };

    println!("[INFO] Starting batch generation job.");
    println!(
        "[INFO] Default Start Level: {}, Default Ramp Rate: {}",
        state.current_start_level, state.current_ramp_rate
    );

    println!(
        "[DEBUG] Attempting to open sequence file at: {:?}",
        &args.sequence_path
    );
    let sequence_file = File::open(&args.sequence_path)?;
    let mut book_stems_found = 0;

    for line_result in BufReader::new(sequence_file).lines() {
        let line = line_result?.trim().to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('%') {
            let parts: Vec<&str> = line[1..].trim().split_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0].to_lowercase().as_str() {
                    "level" => {
                        if let Ok(n) = parts[1].parse::<u32>() {
                            state.current_start_level = n;
                            println!("[CMD] Set Start Level for next book to: {}", n);
                        }
                    }
                    "ramp" => {
                        if let Ok(r) = parts[1].parse::<f32>() {
                            state.current_ramp_rate = r;
                            println!("[CMD] Set Ramp Rate for subsequent books to: {}", r);
                        }
                    }
                    _ => eprintln!("[WARN] Unknown command in sequence file: {}", line),
                }
            }
            continue;
        }

        book_stems_found += 1;
        let book_stem = line;
        println!(
            "\n--- [DEBUG] Processing Book Stem from sequence: {} ---",
            book_stem
        );
        println!(
            "  Using Start Level: {}, Ramp Rate: {}",
            state.current_start_level, state.current_ramp_rate
        );

        let mut learner_profile = NumericalLearnerProfile::new();
        let mut global_lemma_dictionary = GlobalLemmaDictionary::new();

        let start_vocab_size = (state.current_start_level * args.words_per_level) as usize;
        if start_vocab_size > 0 {
            for lemma_str in ordered_lemmas.iter().take(start_vocab_size) {
                let lemma_id = global_lemma_dictionary.get_id_or_insert(lemma_str);
                learner_profile.activate_lemma(lemma_id);
            }
        }

        let json_file_path = PathBuf::from(&project_config.content_project_dir)
            .join(&args.input_json_dir)
            .join(format!("{}.json", book_stem));

        println!(
            "[DEBUG] Attempting to read JSON file from: {:?}",
            &json_file_path
        );

        let json_content_result = fs::read_to_string(&json_file_path);
        if let Err(e) = json_content_result {
            eprintln!(
                "[DEBUG] FAILED to read JSON file '{}': {}. Skipping.",
                book_stem, e
            );
            continue;
        }

        let json_chapter_result = json_parser::parse_chapter_from_json(&json_content_result.unwrap());
        if let Err(e) = json_chapter_result {
            eprintln!(
                "[DEBUG] FAILED to parse JSON file '{}': {}. Skipping.",
                book_stem, e
            );
            continue;
        }

        let json_chapter = json_chapter_result.unwrap();
        println!(
            "[DEBUG] Successfully read and parsed JSON for '{}'.",
            book_stem
        );

        global_lemma_dictionary.populate_from_json_chapter(&json_chapter);
        let (numerical_chapter, _precalculated_english_word_counts) =
            preprocessor::json_chapter_to_numerical(&json_chapter, &mut global_lemma_dictionary);

        if numerical_chapter.sentences_numerical.is_empty() {
            eprintln!("[WARN] No sentences found in {}. Skipping.", book_stem);
            continue;
        }
        println!(
            "[DEBUG] Preprocessed JSON into {} numerical sentences.",
            numerical_chapter.sentences_numerical.len()
        );

        let target_final_level = state.current_start_level;
        println!(
            "  Using STATIC vocabulary size for this run: {} words (Level {})",
            start_vocab_size, state.current_start_level
        );

        let mut final_text_parts = Vec::new();
        let mut book_level_stats = HashMap::new();
        let mut book_segment_stats = HashMap::new();
        let mut total_spanish_words_for_book = 0;
        let mut total_english_words_for_book = 0;

        for (_sentence_idx, n_sentence) in numerical_chapter.sentences_numerical.iter().enumerate()
        {
            let mut n_sentence_clone = n_sentence.clone();
            let output = core_algo::determine_and_annotate_sentence_expression(
                &mut n_sentence_clone,
                &learner_profile,
                &global_lemma_dictionary,
                args.inverse_diglot_threshold,
            );

            total_spanish_words_for_book += output.spanish_word_count;
            total_english_words_for_book += output.english_word_count;

            let s_sentence_json = json_chapter
                .content_blocks
                .iter()
                .find_map(|cb| match cb {
                    JsonContentBlock::Sentence(s) if s.s_id == n_sentence.sentence_id_str => {
                        Some(s)
                    }
                    _ => None,
                })
                .ok_or("Mismatch between numerical and json sentences")?;
            let generated_text = text_generator::generate_raw_text_from_levels(
                &[s_sentence_json],
                &[output.clone()],
                args.debug_markers,
            )?;

            final_text_parts.push(generated_text);

            *book_level_stats.entry(output.level).or_insert(0) += 1;

            //
            match output.level {
                OutputLevel::AdvancedWeave => {
                    if let Some(choices) = &output.l0_segment_choices {
                        for choice in choices {
                            *book_segment_stats
                                .entry(match choice {
                                    L0SegmentChoice::Adv(_) => SegmentType::AdvancedSpanish,
                                    L0SegmentChoice::SimplerAdv(_) => SegmentType::ModerateSpanish,
                                    L0SegmentChoice::InverseDiglot { .. } => {
                                        SegmentType::InverseDiglot
                                    }
                                })
                                .or_insert(0) += 1;
                        }
                    }
                }
                // The `SimpleHybrid` level now represents the holistic English diglot fallback.
                // We'll log all its segments under `EnglishDiglot` for simplicity.
                OutputLevel::SimpleHybrid => {
                    *book_segment_stats
                        .entry(SegmentType::EnglishDiglot)
                        .or_insert(0) += 1;
                }
            }
        }

        let filename = if state.current_ramp_rate == 0.0 {
            format!("{}_L{}.txt", book_stem, state.current_start_level)
        } else if state.current_start_level == target_final_level {
            format!(
                "{}_L{}_{}.txt",
                book_stem, state.current_start_level, target_final_level
            )
        } else {
            format!(
                "{}_L{}_{}_R{}.txt",
                book_stem, state.current_start_level, target_final_level, state.current_ramp_rate
            )
        };

        let final_raw_text = final_text_parts.join("\n\n");
        let final_cleaned_text = text_generator::clean_text_for_tts(&final_raw_text);
        let tts_output_file_path = args.tts_output_dir.join(&filename);
        fs::write(&tts_output_file_path, final_cleaned_text)?;
        println!("  Saved TTS file to: {}", filename);

        log_analysis_to_file(
            &analysis_log_path,
            &filename,
            &book_level_stats,
            &book_segment_stats,
            numerical_chapter.sentences_numerical.len(),
            &learner_profile,
            total_spanish_words_for_book,
            total_english_words_for_book,
        )?;

        state.current_start_level = target_final_level;
    }

    println!(
        "[DEBUG] Finished looping through sequence file. Total book stems found and processed: {}",
        book_stems_found
    );
    println!("\n[INFO] Batch generation job finished.");
    Ok(())
}