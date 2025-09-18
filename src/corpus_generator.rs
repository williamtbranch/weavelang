// In src/corpus_generator.rs

use crate::config::Config;
use crate::simulation::metrics::TextMetrics;
use crate::simulation::{
    core_algo::{self, ChosenLevelOutput, L0SegmentChoice, OutputLevel},
    dictionary::GlobalLemmaDictionary,
    frequency_manager,
    numerical_types::{NumericalChapter, NumericalLearnerProfile, NumericalProcessedSentence, VLevelRecipe},
    preprocessor, text_generator,
};
use crate::{parsing::json_parser, types::json_types::JsonChapter, JsonContentBlock}; // Added JsonChapter
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path}; // Removed PathBuf as it's not needed directly here

// --- NEW PUBLIC STRUCT TO HOLD SIMULATION RESULTS ---
#[derive(Debug, Clone, Default)]
pub struct BookGenerationResult {
    pub all_output_lemma_instances: Vec<String>,
    pub total_target_words: usize,
    pub total_base_words: usize,
    pub level_stats: HashMap<OutputLevel, usize>,
    pub segment_stats: HashMap<SegmentType, usize>,
    pub final_text_parts: Vec<String>,
}

// --- NEW PUBLIC, REUSABLE SIMULATION FUNCTION ---
pub fn generate_book_instance(
    numerical_chapter: &NumericalChapter,
    json_chapter: &JsonChapter,
    dictionary: &GlobalLemmaDictionary,
    sim_v: u32,
    bas_v: u32,
    mod_v: u32,
    adv_v: u32,
    inverse_diglot_threshold: f32,
    debug_markers: bool,
) -> Result<BookGenerationResult, Box<dyn Error>> {
    let mut result = BookGenerationResult::default();
    
    // The profile is for L1 fallbacks in a real curriculum run.
    let mut profile = NumericalLearnerProfile::new();
    let ordered_lemmas = frequency_manager::get_ordered_lemmas();
    // In a real run, the profile is based on the highest V-level.
    let max_v_level = *[sim_v, bas_v, mod_v, adv_v].iter().max().unwrap_or(&0);
    if max_v_level < u32::MAX {
        for lemma_str in ordered_lemmas.iter().take(max_v_level as usize) {
            if let Some(lemma_id) = dictionary.get_id(lemma_str) {
                profile.activate_lemma(lemma_id);
            }
        }
    }

    // --- CONSTRUCT THE RECIPE HERE ---
    let v_levels = VLevelRecipe {
        sim: sim_v,
        bas: bas_v,
        mod_v: mod_v,
        adv: adv_v,
    };

    for n_sentence in &numerical_chapter.sentences_numerical {
        let mut n_sentence_clone = n_sentence.clone();
        let output = core_algo::determine_and_annotate_sentence_expression(
            &mut n_sentence_clone,
            &profile, // The profile is passed for L1 logic
            dictionary,
            &v_levels, // The recipe is passed for L0 logic
            inverse_diglot_threshold,
        );
        for &lemma_id in &output.lemma_ids {
            if let Some(lemma_str) = dictionary.get_str(lemma_id) {
                result.all_output_lemma_instances.push(lemma_str.clone());
            }
        }

        result.total_target_words += output.spanish_word_count;
        result.total_base_words += output.english_word_count;

        let s_sentence_json = json_chapter
            .content_blocks
            .iter()
            .find_map(|cb| match cb {
                JsonContentBlock::Sentence(s) if s.s_id == n_sentence.sentence_id_str => Some(s),
                _ => None,
            })
            .ok_or("Mismatch between numerical and json sentences")?;

        let generated_text =
            text_generator::generate_raw_text_from_levels(&[s_sentence_json], &[output.clone()], debug_markers)?;
        
        result.final_text_parts.push(generated_text);
        *result.level_stats.entry(output.level).or_insert(0) += 1;

        if let OutputLevel::AdvancedWeave = output.level {
            if let Some(choices) = &output.l0_segment_choices {
                for choice in choices {
                    *result.segment_stats
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
        } else {
            *result.segment_stats.entry(SegmentType::EnglishDiglot).or_insert(0) += 1;
        }
    }

    Ok(result)
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SegmentType {
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
    result: &BookGenerationResult, // <-- Simplified to take the result struct
    avd_score: f64,
) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)?;
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    
    let total_output_words = result.total_target_words + result.total_base_words;
    let base_lang_pct = if total_output_words > 0 {
        (result.total_base_words as f32 / total_output_words as f32) * 100.0
    } else { 0.0 };

    writeln!(file, "--- Analysis for Book Instance: {} (at {}) ---", book_instance_unique_id, timestamp)?;
    writeln!(file, "  AVD Score (Density-Weighted): {:.2}", avd_score)?;
    writeln!(file, "  Output Word Count Summary:")?;
    writeln!(file, "    Total Target Words:  {:>5}", result.total_target_words)?;
    writeln!(file, "    Total Base Words:    {:>5}", result.total_base_words)?;
    writeln!(file, "    -------------------------")?;
    writeln!(file, "    Total Output Words:  {:>5}", total_output_words)?;
    writeln!(file, "    Base Lang Pct:       {:>5.2}%", base_lang_pct)?;

    let total_sentences = result.level_stats.values().sum::<usize>();
    if total_sentences > 0 {
        let total_sentences_float = total_sentences as f32;
        writeln!(file, "\n  Sentence Level Distribution:")?;
        let l0_count = *result.level_stats.get(&OutputLevel::AdvancedWeave).unwrap_or(&0);
        let l1_count = *result.level_stats.get(&OutputLevel::SimpleHybrid).unwrap_or(&0);
        writeln!(file, "    L0 Advanced Weave: {:>5} sentences ({:>6.2}%)", l0_count, (l0_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L1 Simple Hybrid:  {:>5} sentences ({:>6.2}%)", l1_count, (l1_count as f32 / total_sentences_float) * 100.0)?;
    }
    // ... (rest of logging logic remains similar, but pulls from `result` struct)
    writeln!(file, "----------------------------------------------------------------------\n")?;

    Ok(())
}

#[derive(Debug, Clone)]
struct ProcessingState {
    sim_v: u32,
    bas_v: u32,
    mod_v: u32,
    adv_v: u32,
}

fn parse_level_value(s: &str) -> u32 {
    if s.eq_ignore_ascii_case("exhausted") {
        u32::MAX
    } else {
        s.parse().unwrap_or(0)
    }
}

// --- REFACTORED run_corpus_generation ---
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
    let analysis_log_path = profiles_dir.join("corpus_analysis_log.txt");

    let mut state = ProcessingState { sim_v: 0, bas_v: 0, mod_v: 0, adv_v: 0 };

    println!("[INFO] Starting batch generation job using V2 sequence format.");
    let sequence_file = File::open(&sequence_path)?;

    for line_result in BufReader::new(sequence_file).lines() {
        let line = line_result?.trim().to_string();
        if line.is_empty() || line.starts_with('#') { continue; }

        if line.starts_with('%') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let command = parts.get(0).cloned().unwrap_or("");
            if (command == "%levels" || command == "%level") && parts.len() == 5 {
                state.sim_v = parse_level_value(parts[1]);
                state.bas_v = parse_level_value(parts[2]);
                state.mod_v = parse_level_value(parts[3]);
                state.adv_v = parse_level_value(parts[4]);
                println!("[CMD] Set Levels to: sim={}, bas={}, mod={}, adv={}", state.sim_v, state.bas_v, state.mod_v, state.adv_v);
            } else {
                eprintln!("[WARN] Unknown or malformed command: {}", line);
            }
            continue;
        }

        let book_stem = line;
        println!("\n--- Processing Book: {} ---", book_stem);

        // --- Data Loading (done once per book) ---
        let json_file_path = project_config.content_project_dir_path().join(input_json_dir).join(format!("{}.json", book_stem));
        let json_content = fs::read_to_string(&json_file_path)?;
        let json_chapter = json_parser::parse_chapter_from_json(&json_content)?;
        let mut dictionary = GlobalLemmaDictionary::new();
        dictionary.populate_from_json_chapter(&json_chapter);
        let (numerical_chapter, _) = preprocessor::json_chapter_to_numerical(&json_chapter, &mut dictionary);

        // --- Call the core simulation engine ---
        let result = generate_book_instance(
            &numerical_chapter, &json_chapter, &dictionary,
            state.sim_v, state.bas_v, state.mod_v, state.adv_v,
            inverse_diglot_threshold, debug_markers,
        )?;

        // --- Handle Results (File I/O) ---
        let metrics = TextMetrics::new(&result.all_output_lemma_instances, result.total_base_words);
        let avd_score = metrics.calculate_avd_score();

        let filename = format!("{}_S{}_B{}_M{}_A{}.txt", book_stem, state.sim_v, state.bas_v, state.mod_v, state.adv_v)
            .replace(&u32::MAX.to_string(), "EX");

        let final_raw_text = result.final_text_parts.join("\n\n");
        let final_cleaned_text = text_generator::clean_text_for_tts(&final_raw_text);
        fs::write(tts_output_dir.join(&filename), final_cleaned_text)?;
        println!("  -> Saved TTS file to: {}", filename);
        
        log_analysis_to_file(&analysis_log_path, &filename, &result, avd_score)?;
    }
    
    println!("\n[INFO] Batch generation job finished.");
    Ok(())
}