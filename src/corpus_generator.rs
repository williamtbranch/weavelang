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
    BasicSpanish,
    SimpleSpanish,
    InverseDiglot,
    EnglishDiglot,
}

fn log_analysis_to_file(
    log_file_path: &Path,
    book_instance_unique_id: &str,
    level_stats: &HashMap<OutputLevel, usize>,
    segment_stats: &HashMap<SegmentType, usize>,
    total_sentences: usize,
    final_profile: &NumericalLearnerProfile, // Kept for future use, though less relevant now
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
    writeln!(file, "    Total Target Words:  {:>5}", total_spanish_words)?;
    writeln!(file, "    Total Base Words:    {:>5}", total_english_words)?;
    writeln!(file, "    -------------------------")?;
    writeln!(file, "    Total Output Words:  {:>5}", total_output_words)?;
    writeln!(
        file,
        "    Base Lang Pct:       {:>5.2}%",
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
        let bs_count = get_segment_count(SegmentType::BasicSpanish);
        let ss_count = get_segment_count(SegmentType::SimpleSpanish);
        let id_count = get_segment_count(SegmentType::InverseDiglot);
        let ed_count = get_segment_count(SegmentType::EnglishDiglot);
        let total_segments_float = total_segments as f32;
        
        let percentage = |count: usize| (count as f32 / total_segments_float) * 100.0;
        
        writeln!(file, "    Adv. Target Segments:  {:>5} segments ({:>6.2}%)", as_count, percentage(as_count))?;
        writeln!(file, "    Mod. Target Segments:  {:>5} segments ({:>6.2}%)", ms_count, percentage(ms_count))?;
        writeln!(file, "    Bas. Target Segments:  {:>5} segments ({:>6.2}%)", bs_count, percentage(bs_count))?;
        writeln!(file, "    Sim. Target Segments:  {:>5} segments ({:>6.2}%)", ss_count, percentage(ss_count))?;
        writeln!(file, "    Inv. Diglot Segments:  {:>5} segments ({:>6.2}%)", id_count, percentage(id_count))?;
        writeln!(file, "    Base Diglot Segments:  {:>5} segments ({:>6.2}%)", ed_count, percentage(ed_count))?;
    }
    
    writeln!(file, "\n  Final Profile State:")?;
    writeln!(
        file,
        "    Activated Lemmas (for ID): {}",
        final_profile.vocabulary.len()
    )?;
    writeln!(
        file,
        "----------------------------------------------------------------------\n"
    )?;
    Ok(())
}

// --- ARGUMENTS STRUCT IS NO LONGER NEEDED, REMOVED ---
// pub struct GenerationArgs { ... }

// --- NEW STATE STRUCT TO HOLD THE FOUR V-LEVELS ---
#[derive(Debug, Clone)]
struct ProcessingState {
    sim_v: u32,
    bas_v: u32,
    mod_v: u32,
    adv_v: u32,
}

// --- HELPER TO PARSE LEVEL VALUES ('exhausted' or a number) ---
fn parse_level_value(s: &str) -> u32 {
    if s.eq_ignore_ascii_case("exhausted") {
        u32::MAX
    } else {
        s.parse().unwrap_or(0)
    }
}

pub fn run_corpus_generation(
    project_config: &Config,
    tool_root_dir: &Path,
    sequence_path: &Path,
    input_json_dir: &Path,
    tts_output_dir: &Path,
    profiles_dir: &Path,
    debug_markers: bool,
    inverse_diglot_threshold: f32,
) -> Result<(), Box<dyn Error>> {
    let freq_list_path = tool_root_dir
        .join("assets")
        .join("frequency_lists")
        .join("es_master_frequency_list.txt");

    frequency_manager::load_master_frequency_list(&freq_list_path)?;
    
    fs::create_dir_all(&tts_output_dir)?;
    fs::create_dir_all(&profiles_dir)?;

    let analysis_log_path = profiles_dir.join("corpus_analysis_log.txt");

    let mut state = ProcessingState {
        sim_v: 0,
        bas_v: 0,
        mod_v: 0,
        adv_v: 0,
    };

    println!("[INFO] Starting batch generation job using V2 sequence format.");

    let sequence_file = File::open(&sequence_path)?;
    
    for line_result in BufReader::new(sequence_file).lines() {
        let line = line_result?.trim().to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('%') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // --- THIS IS THE FIX ---
            let command = parts.get(0).cloned().unwrap_or("");
            if (command == "%levels" || command == "%level") && parts.len() == 5 {
            // --- END OF FIX ---
                state.sim_v = parse_level_value(parts[1]);
                state.bas_v = parse_level_value(parts[2]);
                state.mod_v = parse_level_value(parts[3]);
                state.adv_v = parse_level_value(parts[4]);
                println!(
                    "[CMD] Set Levels to: sim={}, bas={}, mod={}, adv={}",
                    state.sim_v, state.bas_v, state.mod_v, state.adv_v
                );
            } else {
                eprintln!("[WARN] Unknown or malformed command in sequence file: {}", line);
            }
            continue;
        }

        let book_stem = line;
        println!("\n--- Processing Book: {} ---", book_stem);

        let mut learner_profile = NumericalLearnerProfile::new();
        let mut global_lemma_dictionary = GlobalLemmaDictionary::new();
        let ordered_lemmas = frequency_manager::get_ordered_lemmas();
        let max_v_level = *[state.sim_v, state.bas_v, state.mod_v, state.adv_v].iter().max().unwrap_or(&0);
        if max_v_level < u32::MAX {
            for lemma_str in ordered_lemmas.iter().take(max_v_level as usize) {
                let lemma_id = global_lemma_dictionary.get_id_or_insert(lemma_str);
                learner_profile.activate_lemma(lemma_id);
            }
        }

        let json_file_path = PathBuf::from(&project_config.content_project_dir)
            .join(input_json_dir)
            .join(format!("{}.json", book_stem));

        let json_content = match fs::read_to_string(&json_file_path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("[ERROR] FAILED to read JSON file for '{}': {}. Skipping.", book_stem, e);
                continue;
            }
        };

        let json_chapter = match json_parser::parse_chapter_from_json(&json_content) {
            Ok(chapter) => chapter,
            Err(e) => {
                eprintln!("[ERROR] FAILED to parse JSON for '{}': {}. Skipping.", book_stem, e);
                continue;
            }
        };

        global_lemma_dictionary.populate_from_json_chapter(&json_chapter);
        let (numerical_chapter, _) = preprocessor::json_chapter_to_numerical(&json_chapter, &mut global_lemma_dictionary);

        if numerical_chapter.sentences_numerical.is_empty() {
            eprintln!("[WARN] No sentences found in {}. Skipping.", book_stem);
            continue;
        }

        let mut final_text_parts = Vec::new();
        let mut book_level_stats = HashMap::new();
        let mut book_segment_stats = HashMap::new();
        let mut total_spanish_words_for_book = 0;
        let mut total_english_words_for_book = 0;

        for n_sentence in &numerical_chapter.sentences_numerical {
            let mut n_sentence_clone = n_sentence.clone();
            let output = core_algo::determine_and_annotate_sentence_expression(
                &mut n_sentence_clone,
                &learner_profile,
                &global_lemma_dictionary,
                state.sim_v,
                state.bas_v,
                state.mod_v,
                state.adv_v,
                inverse_diglot_threshold,
            );

            total_spanish_words_for_book += output.spanish_word_count;
            total_english_words_for_book += output.english_word_count;

            let s_sentence_json = json_chapter
                .content_blocks
                .iter()
                .find_map(|cb| match cb {
                    JsonContentBlock::Sentence(s) if s.s_id == n_sentence.sentence_id_str => Some(s),
                    _ => None,
                })
                .ok_or("Mismatch between numerical and json sentences")?;
            
            let generated_text = text_generator::generate_raw_text_from_levels(
                &[s_sentence_json],
                &[output.clone()],
                debug_markers,
            )?;

            final_text_parts.push(generated_text);
            *book_level_stats.entry(output.level).or_insert(0) += 1;

            match output.level {
                OutputLevel::AdvancedWeave => {
                    if let Some(choices) = &output.l0_segment_choices {
                        for choice in choices {
                            *book_segment_stats
                                .entry(match choice {
                                    L0SegmentChoice::Adv(_) => SegmentType::AdvancedSpanish,
                                    L0SegmentChoice::Mod(_) => SegmentType::ModerateSpanish,
                                    L0SegmentChoice::Bas(_) => SegmentType::BasicSpanish,
                                    L0SegmentChoice::Sim(_) => SegmentType::SimpleSpanish,
                                    L0SegmentChoice::InverseDiglot { .. } => SegmentType::InverseDiglot,
                                })
                                .or_insert(0) += 1;
                        }
                    }
                }
                OutputLevel::SimpleHybrid => {
                    *book_segment_stats.entry(SegmentType::EnglishDiglot).or_insert(0) += 1;
                }
            }
        }

        let filename = format!(
            "{}_S{}_B{}_M{}_A{}.txt",
            book_stem, state.sim_v, state.bas_v, state.mod_v, state.adv_v
        ).replace(&u32::MAX.to_string(), "EX");

        let final_raw_text = final_text_parts.join("\n\n");
        let final_cleaned_text = text_generator::clean_text_for_tts(&final_raw_text);
        let tts_output_file_path = tts_output_dir.join(&filename);
        fs::write(&tts_output_file_path, final_cleaned_text)?;
        println!("  -> Saved TTS file to: {}", filename);

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
    }
    
    println!("\n[INFO] Batch generation job finished.");
    Ok(())
}