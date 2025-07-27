// src/corpus_generator.rs
use crate::config::Config;
use crate::simulation::{
    core_algo::{self, L0SegmentChoice, L1PartChoice, OutputLevel},
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
    SimpleSpanish,
    EnglishDiglot,
    English,
    InverseDiglot, // Added for more detailed analysis
}

fn log_analysis_to_file(
    log_file_path: &Path,
    book_instance_unique_id: &str,
    level_stats: &HashMap<OutputLevel, usize>,
    segment_stats: &HashMap<SegmentType, usize>,
    total_sentences: usize,
    final_profile: &NumericalLearnerProfile,
) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new().create(true).append(true).open(log_file_path)?;
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    writeln!(file, "--- Analysis for Book Instance: {} (at {}) ---", book_instance_unique_id, timestamp)?;
    let total_sentences_float = total_sentences as f32;
    if total_sentences_float > 0.0 {
        writeln!(file, "  Sentence Level Distribution:")?;
        let get_level_count = |level: OutputLevel| -> usize { *level_stats.get(&level).unwrap_or(&0) };
        let l0_count = get_level_count(OutputLevel::AdvancedWeave);
        let l1_count = get_level_count(OutputLevel::SimpleHybrid);
        writeln!(file, "    L0 Advanced Weave: {:>5} sentences ({:>6.2}%)", l0_count, (l0_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L1 Simple Hybrid:  {:>5} sentences ({:>6.2}%)", l1_count, (l1_count as f32 / total_sentences_float) * 100.0)?;
    } else { writeln!(file, "  No sentences processed for this instance.")?; }
    let total_segments = segment_stats.values().sum::<usize>();
    if total_segments > 0 {
        writeln!(file, "  Segment Type Distribution (Total: {} segments):", total_segments)?;
        let get_segment_count = |seg_type: SegmentType| -> usize { *segment_stats.get(&seg_type).unwrap_or(&0) };
        let as_count = get_segment_count(SegmentType::AdvancedSpanish);
        let ms_count = get_segment_count(SegmentType::ModerateSpanish);
        let id_count = get_segment_count(SegmentType::InverseDiglot);
        let ss_count = get_segment_count(SegmentType::SimpleSpanish);
        let ed_count = get_segment_count(SegmentType::EnglishDiglot);
        let en_count = get_segment_count(SegmentType::English);
        let total_segments_float = total_segments as f32;
        writeln!(file, "    AS (Advanced Spanish): {:>5} segments ({:>6.2}%)", as_count, (as_count as f32 / total_segments_float) * 100.0)?;
        writeln!(file, "    MS (Moderate Spanish): {:>5} segments ({:>6.2}%)", ms_count, (ms_count as f32 / total_segments_float) * 100.0)?;
        writeln!(file, "    ID (Inverse Diglot):   {:>5} segments ({:>6.2}%)", id_count, (id_count as f32 / total_segments_float) * 100.0)?;
        writeln!(file, "    SS (Simple Spanish):   {:>5} segments ({:>6.2}%)", ss_count, (ss_count as f32 / total_segments_float) * 100.0)?;
        writeln!(file, "    ED (English Diglot):   {:>5} segments ({:>6.2}%)", ed_count, (ed_count as f32 / total_segments_float) * 100.0)?;
        writeln!(file, "    EN (English):          {:>5} segments ({:>6.2}%)", en_count, (en_count as f32 / total_segments_float) * 100.0)?;
    }
    writeln!(file, "  Final Profile State:")?;
    writeln!(file, "    Activated Lemmas: {}", final_profile.vocabulary.len())?;
    writeln!(file, "---------------------------------------------------------\n")?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct GenerationArgs {
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
    // --- NEW FIELD ---
    pub inverse_diglot_threshold: f32,
}

struct ProcessingState {
    current_start_level: u32,
    current_ramp_rate: f32,
}

fn calculate_words_to_introduce(
    total_words: usize,
    words_per_new_lemma_baseline: f32,
    core_vocab_size: u32,
    start_vocab_idx: u32,
) -> u32 {
    let mut words_remaining = total_words as f32;
    let mut words_introduced = 0;
    let mut current_vocab_idx = start_vocab_idx;

    if words_per_new_lemma_baseline <= 0.0 || words_per_new_lemma_baseline == f32::MAX { return 0; }

    loop {
        let cost_of_next_word = if current_vocab_idx < core_vocab_size {
            let taper_start_multiplier = 3.0;
            let progress_through_core = current_vocab_idx as f32 / core_vocab_size as f32;
            let current_multiplier =
                taper_start_multiplier - (progress_through_core * (taper_start_multiplier - 1.0));
            words_per_new_lemma_baseline * current_multiplier
        } else {
            words_per_new_lemma_baseline
        };

        if words_remaining >= cost_of_next_word {
            words_remaining -= cost_of_next_word;
            words_introduced += 1;
            current_vocab_idx += 1;
        } else {
            break;
        }
    }
    words_introduced
}

pub fn run_corpus_generation(
    project_config: &Config,
    args: &GenerationArgs,
) -> Result<(), Box<dyn Error>> {
    let freq_list_path = PathBuf::from(&project_config.content_project_dir)
        .join("assets")
        .join("es_master_frequency_list.txt");

    // --- DEBUG: Confirming frequency list is loaded ---
    println!("[DEBUG] Attempting to load frequency list from: {:?}", &freq_list_path);
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
    println!("[INFO] Default Start Level: {}, Default Ramp Rate: {}", state.current_start_level, state.current_ramp_rate);
    
    // --- DEBUG: Confirming sequence file path ---
    println!("[DEBUG] Attempting to open sequence file at: {:?}", &args.sequence_path);
    let sequence_file = File::open(&args.sequence_path)?;
    let mut book_stems_found = 0;

    for line_result in BufReader::new(sequence_file).lines() {
        let line = line_result?.trim().to_string();
        if line.is_empty() || line.starts_with('#') { continue; }

        if line.starts_with('%') {
            let parts: Vec<&str> = line[1..].trim().split_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0].to_lowercase().as_str() {
                    "level" => {
                        if let Ok(n) = parts[1].parse::<u32>() {
                            state.current_start_level = n;
                            println!("[CMD] Set Start Level for next book to: {}", n);
                        }
                    },
                    "ramp" => {
                        if let Ok(r) = parts[1].parse::<f32>() {
                            state.current_ramp_rate = r;
                            println!("[CMD] Set Ramp Rate for subsequent books to: {}", r);
                        }
                    },
                    _ => eprintln!("[WARN] Unknown command in sequence file: {}", line),
                }
            }
            continue;
        }
        
        book_stems_found += 1;
        let book_stem = line;
        println!("\n--- [DEBUG] Processing Book Stem from sequence: {} ---", book_stem);
        println!("  Using Start Level: {}, Ramp Rate: {}", state.current_start_level, state.current_ramp_rate);

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
            .join(format!("{}.stage10.json", book_stem));
        
        // --- DEBUG: Confirming JSON file path and read attempt ---
        println!("[DEBUG] Attempting to read JSON file from: {:?}", &json_file_path);
        
        let json_content_result = fs::read_to_string(&json_file_path);
        if let Err(e) = json_content_result {
            eprintln!("[DEBUG] FAILED to read JSON file '{}': {}. Skipping.", book_stem, e);
            continue; // Skip to the next book
        }
        
        let json_chapter_result = json_parser::parse_chapter_from_json(&json_content_result.unwrap());
        if let Err(e) = json_chapter_result {
            eprintln!("[DEBUG] FAILED to parse JSON file '{}': {}. Skipping.", book_stem, e);
            continue; // Skip to the next book
        }
        
        let json_chapter = json_chapter_result.unwrap();
        println!("[DEBUG] Successfully read and parsed JSON for '{}'.", book_stem);

        global_lemma_dictionary.populate_from_json_chapter(&json_chapter);
        let (numerical_chapter, precalculated_english_word_counts) = preprocessor::json_chapter_to_numerical(&json_chapter, &mut global_lemma_dictionary);
        
        if numerical_chapter.sentences_numerical.is_empty() {
            eprintln!("[WARN] No sentences found in {}. Skipping.", book_stem);
            continue;
        }
        println!("[DEBUG] Preprocessed JSON into {} numerical sentences.", numerical_chapter.sentences_numerical.len());
        
        let total_english_word_count: usize = precalculated_english_word_counts.iter().sum();
        let estimated_output_words = (total_english_word_count as f32 * 1.1) as usize;
        let words_per_new_lemma_baseline = if state.current_ramp_rate > 0.0 { 9000.0 / state.current_ramp_rate } else { f32::MAX };

        let naturally_introduced = calculate_words_to_introduce(estimated_output_words, words_per_new_lemma_baseline, args.core_vocab_size, start_vocab_size as u32);
        let natural_final_vocab = start_vocab_size as u32 + naturally_introduced;
        let natural_final_level = natural_final_vocab / args.words_per_level;
        
        let progress_into_next = (natural_final_vocab % args.words_per_level) as f32 / args.words_per_level as f32;

        let mut target_final_level = if state.current_ramp_rate > 0.0 && progress_into_next >= args.stretch_threshold {
            natural_final_level + 1
        } else {
            natural_final_level
        };
        
        let final_target_vocab_size = target_final_level * args.words_per_level;
        let final_words_to_introduce = final_target_vocab_size.saturating_sub(start_vocab_size as u32);
        
        let adjusted_words_per_lemma = if final_words_to_introduce > 0 { total_english_word_count as f32 / final_words_to_introduce as f32 } else { f32::MAX };
        let compression = if words_per_new_lemma_baseline == f32::MAX { 0.0 } else { (words_per_new_lemma_baseline - adjusted_words_per_lemma).abs() / words_per_new_lemma_baseline };

        if compression > args.max_compression_ratio {
            println!("[WARN] Stretch compression ({:.1}%) exceeds max ({}%). Reverting to rounding down.", compression * 100.0, args.max_compression_ratio * 100.0);
            target_final_level = natural_final_level;
        }
        
        let final_target_vocab_size = target_final_level * args.words_per_level;
        let final_words_to_introduce = final_target_vocab_size.saturating_sub(start_vocab_size as u32);

        println!("  Target End Level: {}. Will introduce {} new words.", target_final_level, final_words_to_introduce);
        
        let mut activation_map: HashMap<usize, Vec<u32>> = HashMap::new();
        if final_words_to_introduce > 0 {
            let mut credit: u64 = 0;
            let total_words_u64 = total_english_word_count as u64;
            let words_to_add_u64 = final_words_to_introduce as u64;
            let mut words_activated = 0;

            for (sentence_idx, &eng_word_count) in precalculated_english_word_counts.iter().enumerate() {
                credit += (eng_word_count as u64) * words_to_add_u64;

                while credit >= total_words_u64 && words_activated < final_words_to_introduce {
                    let next_idx = start_vocab_size as u32 + words_activated;
                    if let Some(lemma_str) = ordered_lemmas.get(next_idx as usize) {
                        let lemma_id = global_lemma_dictionary.get_id_or_insert(lemma_str);
                        activation_map.entry(sentence_idx).or_default().push(lemma_id);
                    }
                    words_activated += 1;
                    credit -= total_words_u64;
                }
            }
        }

        let mut final_text_parts = Vec::new();
        let mut book_level_stats = HashMap::new();
        let mut book_segment_stats = HashMap::new();

        for (sentence_idx, n_sentence) in numerical_chapter.sentences_numerical.iter().enumerate() {
            if let Some(lemmas_to_activate) = activation_map.get(&sentence_idx) {
                for &lemma_id in lemmas_to_activate {
                    learner_profile.activate_lemma(lemma_id);
                }
            }

            let mut n_sentence_clone = n_sentence.clone();
            let output = core_algo::determine_and_annotate_sentence_expression(
                &mut n_sentence_clone, 
                &learner_profile, 
                &global_lemma_dictionary,
                args.inverse_diglot_threshold,
            );
            
            let s_sentence_json = json_chapter.content_blocks.iter().find_map(|cb| match cb {
                JsonContentBlock::Sentence(s) if s.original_sentence_s_id == n_sentence.sentence_id_str => Some(s), _ => None,
            }).ok_or("Mismatch between numerical and json sentences")?;
            let generated_text = text_generator::generate_raw_text_from_levels(&[s_sentence_json], &[output.clone()], args.debug_markers)?;

            final_text_parts.push(generated_text);
            
            *book_level_stats.entry(output.level).or_insert(0) += 1;
             match output.level {
                OutputLevel::AdvancedWeave => if let Some(choices) = &output.l0_segment_choices {
                    for choice in choices {
                        *book_segment_stats.entry(match choice {
                            L0SegmentChoice::Adv(_) => SegmentType::AdvancedSpanish,
                            L0SegmentChoice::SimplerAdv(_) => SegmentType::ModerateSpanish,
                            L0SegmentChoice::InverseDiglot { .. } => SegmentType::InverseDiglot,
                        }).or_insert(0) += 1;
                    }
                },
                OutputLevel::SimpleHybrid => if let Some(choices) = &output.l1_part_choices {
                    for choice in choices {
                         *book_segment_stats.entry(match choice {
                            L1PartChoice::Spanish(_) => SegmentType::SimpleSpanish,
                            L1PartChoice::Woven(_, _) => SegmentType::EnglishDiglot,
                            L1PartChoice::English(_) => SegmentType::English,
                        }).or_insert(0) += 1;
                    }
                },
            }
        }
        
        let filename = if state.current_ramp_rate == 0.0 {
            format!("{}_L{}.txt", book_stem, state.current_start_level)
        } else if state.current_start_level == target_final_level {
            format!("{}_L{}_{}.txt", book_stem, state.current_start_level, target_final_level)
        } else {
            format!("{}_L{}_{}_R{}.txt", book_stem, state.current_start_level, target_final_level, state.current_ramp_rate)
        };
        
        let final_raw_text = final_text_parts.join("\n\n");
        let final_cleaned_text = text_generator::clean_text_for_tts(&final_raw_text);
        let tts_output_file_path = args.tts_output_dir.join(&filename);
        fs::write(&tts_output_file_path, final_cleaned_text)?;
        println!("  Saved TTS file to: {}", filename);
        
        log_analysis_to_file(&analysis_log_path, &filename, &book_level_stats, &book_segment_stats, numerical_chapter.sentences_numerical.len(), &learner_profile)?;

        state.current_start_level = target_final_level;
    }

    // --- DEBUG: Confirming loop finished ---
    println!("[DEBUG] Finished looping through sequence file. Total book stems found and processed: {}", book_stems_found);
    println!("\n[INFO] Batch generation job finished.");
    Ok(())
}