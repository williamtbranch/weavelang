// src/corpus_generator.rs

use crate::config::Config;
use crate::simulation::metrics::TextMetrics;
use crate::simulation::{
    core_algo::{self, L0SegmentChoice, OutputLevel},
    dictionary::GlobalLemmaDictionary,
    frequency_manager,
    numerical_types::{NumericalChapter, NumericalLearnerProfile, VLevelRecipe},
    preprocessor, text_generator,
};
use crate::{parsing::json_parser, types::json_types::JsonChapter, JsonContentBlock};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path};

// --- NEW ENUM FOR SEGMENT TYPES ---
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SegmentType {
    AdvancedSpanish,
    ModerateSpanish,
    BasicSpanish,      // Represents the full BS sentence
    InverseDiglot,     // Represents a successful ID weave
    EnglishDiglot,     // Represents the final BE diglot fallback
}

// --- UPDATED STRUCT TO HOLD SIMULATION RESULTS ---
#[derive(Debug, Clone, Default)]
pub struct BookGenerationResult {
    pub all_output_lemma_instances: Vec<String>,
    pub total_target_words: usize,
    pub total_base_words: usize,
    pub level_stats: HashMap<OutputLevel, usize>,
    pub segment_stats: HashMap<SegmentType, usize>, // No change here
    pub final_text_parts: Vec<String>,
}

// --- UPDATED SIMULATION FUNCTION ---
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
    
    let mut profile = NumericalLearnerProfile::new();
    let ordered_lemmas = frequency_manager::get_ordered_lemmas();
    // Use `bas_v` for the profile, as it's the level for L1 checks
    if bas_v < u32::MAX {
        for lemma_str in ordered_lemmas.iter().take(bas_v as usize) {
            if let Some(lemma_id) = dictionary.get_id(lemma_str) {
                profile.activate_lemma(lemma_id);
            }
        }
    }

    let v_levels = VLevelRecipe { sim: sim_v, bas: bas_v, mod_v, adv: adv_v };

    for n_sentence in &numerical_chapter.sentences_numerical {
        let mut n_sentence_clone = n_sentence.clone();
        let output = core_algo::determine_and_annotate_sentence_expression(
            &mut n_sentence_clone, &profile, dictionary, &v_levels, inverse_diglot_threshold,
        );
        for &lemma_id in &output.lemma_ids {
            if let Some(lemma_str) = dictionary.get_str(lemma_id) {
                result.all_output_lemma_instances.push(lemma_str.clone());
            }
        }

        result.total_target_words += output.spanish_word_count;
        result.total_base_words += output.english_word_count;

        let s_sentence_json = json_chapter.content_blocks.iter()
            .find_map(|cb| match cb {
                JsonContentBlock::Sentence(s) if s.s_id == n_sentence.sentence_id_str => Some(s),
                _ => None,
            })
            .ok_or("Mismatch between numerical and json sentences")?;

        let generated_text =
            text_generator::generate_raw_text_from_levels(&[s_sentence_json], &[output.clone()], debug_markers)?;
        
        result.final_text_parts.push(generated_text);
        *result.level_stats.entry(output.level).or_insert(0) += 1;

        // --- THIS IS THE UPDATED LOGIC TO POPULATE SEGMENT STATS ---
        match output.level {
            OutputLevel::AdvancedWeave => {
                if let Some(choices) = &output.l0_segment_choices {
                    for choice in choices {
                        *result.segment_stats.entry(match choice {
                            L0SegmentChoice::Adv(_) => SegmentType::AdvancedSpanish,
                            L0SegmentChoice::Mod(_) => SegmentType::ModerateSpanish,
                        }).or_insert(0) += 1;
                    }
                }
            },
            OutputLevel::BasicTarget => {
                *result.segment_stats.entry(SegmentType::BasicSpanish).or_insert(0) += 1;
            },
            OutputLevel::BasicBaseDiglot => {
                *result.segment_stats.entry(SegmentType::EnglishDiglot).or_insert(0) += 1;
            },
            // The InverseDiglot level is new
            OutputLevel::InverseDiglot => {
                 *result.segment_stats.entry(SegmentType::InverseDiglot).or_insert(0) += 1;
            }
        }
        // --- END OF UPDATED LOGIC ---
    }

    Ok(result)
}


// --- UPDATED LOGGING FUNCTION ---
fn log_analysis_to_file(
    log_file_path: &Path,
    book_instance_unique_id: &str,
    result: &BookGenerationResult,
    avd_score: f64,
) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new().create(true).append(true).open(log_file_path)?;
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
        let l1_bt_count = *result.level_stats.get(&OutputLevel::BasicTarget).unwrap_or(&0);
        let l1_id_count = *result.level_stats.get(&OutputLevel::InverseDiglot).unwrap_or(&0);
        let l1_bb_count = *result.level_stats.get(&OutputLevel::BasicBaseDiglot).unwrap_or(&0);

        writeln!(file, "    L0 Advanced Weave: {:>5} sentences ({:>6.2}%)", l0_count, (l0_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L1 Basic Target:   {:>5} sentences ({:>6.2}%)", l1_bt_count, (l1_bt_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L1 Inverse Diglot: {:>5} sentences ({:>6.2}%)", l1_id_count, (l1_id_count as f32 / total_sentences_float) * 100.0)?;
        writeln!(file, "    L1 Basic Diglot:   {:>5} sentences ({:>6.2}%)", l1_bb_count, (l1_bb_count as f32 / total_sentences_float) * 100.0)?;
    }
    
    let total_segments = result.segment_stats.values().sum::<usize>();
    if total_segments > 0 {
        writeln!(file, "\n  Segment/Sentence Type Distribution (Total: {} units):", total_segments)?;
        let total_segments_float = total_segments as f32;
        let ordered_segment_types = [
            (SegmentType::AdvancedSpanish, "Adv. Target Segments"),
            (SegmentType::ModerateSpanish, "Mod. Target Segments"),
            (SegmentType::BasicSpanish,    "Basic Target Sentences"),
            (SegmentType::InverseDiglot,    "Inverse Diglot Sentences"),
            (SegmentType::EnglishDiglot,    "Base Diglot Sentences"),
        ];

        for (seg_type, label) in ordered_segment_types {
            if let Some(count) = result.segment_stats.get(&seg_type) {
                writeln!(file, "    {:<25} {:>5} units ({:>6.2}%)", label, count, (*count as f32 / total_segments_float) * 100.0)?;
            }
        }
    }
    
    writeln!(file, "----------------------------------------------------------------------\n")?;
    Ok(())
}

// --- The rest of the file (command parsing, main loop) is unchanged ---

#[derive(Debug, Clone, Default)]
struct ProcessingState {
    sim_v: u32,
    bas_v: u32,
    mod_v: u32,
    adv_v: u32,
    user_level: Option<u32>,
}

fn parse_level_value(s: &str) -> u32 {
    if s.eq_ignore_ascii_case("exhausted") { u32::MAX } else { s.parse().unwrap_or(0) }
}

pub fn run_corpus_generation(
    project_config: &Config,
    _tool_root_dir: &Path,
    sequence_path: &Path,
    input_json_dir: &Path,
    tts_output_dir: &Path,
    profiles_dir: &Path,
    debug_markers: bool,
    inverse_diglot_threshold: f32,
) -> Result<(), Box<dyn Error>> {
    let analysis_log_path = profiles_dir.join("corpus_analysis_log.txt");
    let mut state = ProcessingState::default();

    println!("[INFO] Starting batch generation job using progressive curriculum maps.");
    let absolute_path = fs::canonicalize(&sequence_path).unwrap_or_else(|_| sequence_path.to_path_buf());
    println!("[DEBUG] Attempting to open sequence file at absolute path: {}", absolute_path.display());
    let sequence_file = File::open(&sequence_path)?;

    for line_result in BufReader::new(sequence_file).lines() {
        let line = line_result?.trim().to_string();
        if line.is_empty() || line.starts_with('#') { continue; }

        if line.starts_with('%') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let command = parts.get(0).cloned().unwrap_or("");
            if command == "%u-level" && parts.len() == 2 {
                state.user_level = Some(parts[1].parse()?);
                state.sim_v = 0; state.bas_v = 0; state.mod_v = 0; state.adv_v = 0;
                println!("[CMD] Set User Level to: {}", state.user_level.unwrap());
            } else if (command == "%levels" || command == "%level") && parts.len() == 5 {
                state.sim_v = parse_level_value(parts[1]); state.bas_v = parse_level_value(parts[2]);
                state.mod_v = parse_level_value(parts[3]); state.adv_v = parse_level_value(parts[4]);
                state.user_level = None;
                println!("[CMD] Set Manual Levels to: sim={}, bas={}, mod={}, adv={}", state.sim_v, state.bas_v, state.mod_v, state.adv_v);
            } else {
                eprintln!("[WARN] Unknown or malformed command: {}", line);
            }
            continue;
        }

        let book_stem = line;
        println!("\n--- Processing Book: {} ---", book_stem);

        let json_file_path = project_config.content_project_dir_path().join(input_json_dir).join(format!("{}.json", book_stem));
        let json_content = fs::read_to_string(&json_file_path)?;
        let json_chapter = json_parser::parse_chapter_from_json(&json_content)?;
        
        let mut dictionary = GlobalLemmaDictionary::new();
        dictionary.populate_from_json_chapter(&json_chapter);
        let (numerical_chapter, _) = preprocessor::json_chapter_to_numerical(&json_chapter, &mut dictionary);
        
        let mut full_book_result = BookGenerationResult::default();
        let filename: String;

        if let Some(u_level) = state.user_level {
            let u_level_map = json_chapter.u_level_maps.get(&u_level.to_string())
                .ok_or(format!("U-Level '{}' not found in map for book '{}'", u_level, book_stem))?;

            let end_level_for_range = (u_level_map.end_level - 1.0).floor() as u32;
            filename = if end_level_for_range > u_level {
                format!("{}_UL{}-{}.txt", book_stem, u_level, end_level_for_range)
            } else {
                format!("{}_UL{}.txt", book_stem, u_level)
            };

            for (i, entry) in u_level_map.map.iter().enumerate() {
                let start_idx = entry.start_sentence_idx;
                let end_idx = if i + 1 < u_level_map.map.len() {
                    u_level_map.map[i+1].start_sentence_idx
                } else {
                    numerical_chapter.sentences_numerical.len()
                };

                if start_idx >= end_idx { continue; }

                let mut numerical_slice = numerical_chapter.clone();
                numerical_slice.sentences_numerical = numerical_chapter.sentences_numerical[start_idx..end_idx].to_vec();
                
                let mut json_slice = json_chapter.clone();
                json_slice.content_blocks = json_chapter.content_blocks.iter().filter_map(|cb| match cb {
                    JsonContentBlock::Sentence(s) => {
                        if numerical_slice.sentences_numerical.iter().any(|ns| ns.sentence_id_str == s.s_id) {
                            Some(JsonContentBlock::Sentence(s.clone()))
                        } else { None }
                    }
                    _ => None
                }).collect();
                
                let recipe = &entry.recipe;
                
                let slice_result = generate_book_instance(
                    &numerical_slice, &json_slice, &dictionary,
                    recipe.sim, recipe.bas, recipe.mod_v, recipe.adv,
                    inverse_diglot_threshold, debug_markers,
                )?;

                full_book_result.final_text_parts.extend(slice_result.final_text_parts);
                full_book_result.all_output_lemma_instances.extend(slice_result.all_output_lemma_instances);
                full_book_result.total_target_words += slice_result.total_target_words;
                full_book_result.total_base_words += slice_result.total_base_words;
                for (level, count) in slice_result.level_stats { *full_book_result.level_stats.entry(level).or_insert(0) += count; }
                for (seg_type, count) in slice_result.segment_stats { *full_book_result.segment_stats.entry(seg_type).or_insert(0) += count; }
            }
        } else {
            filename = format!("{}_S{}_B{}_M{}_A{}.txt", book_stem, state.sim_v, state.bas_v, state.mod_v, state.adv_v)
                .replace(&u32::MAX.to_string(), "EX");
            
            full_book_result = generate_book_instance(
                &numerical_chapter, &json_chapter, &dictionary,
                state.sim_v, state.bas_v, state.mod_v, state.adv_v,
                inverse_diglot_threshold, debug_markers,
            )?;
        }
        
        let metrics = TextMetrics::new(&full_book_result.all_output_lemma_instances, full_book_result.total_base_words);
        let avd_score = metrics.calculate_avd_score();
        
        let final_raw_text = full_book_result.final_text_parts.join("\n\n");
        let final_cleaned_text = text_generator::clean_text_for_tts(&final_raw_text);
        fs::write(tts_output_dir.join(&filename), final_cleaned_text)?;
        println!("  -> Saved TTS file to: {}", filename);
        
        log_analysis_to_file(&analysis_log_path, &filename, &full_book_result, avd_score)?;
    }
    
    println!("\n[INFO] Batch generation job finished.");
    Ok(())
}