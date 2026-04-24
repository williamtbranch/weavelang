// src/simulation/corpus_generator.rs

use crate::config::Config;
use crate::simulation::metrics::TextMetrics;
use crate::simulation::numerical_types::LLevelRecipe;
use crate::simulation::{
    core_algo::{self, L0SegmentChoice, OutputLevel},
    dictionary::GlobalLemmaDictionary,
    frontier::{FrontierConfig as FrontierEngineConfig, FrontierEngine},
    frequency_manager,
    numerical_types::{NumericalChapter, NumericalLearnerProfile, VLevelRecipe},
    preprocessor, text_generator,
};
use crate::{parsing::json_parser, types::json_types::JsonChapter, JsonContentBlock};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SegmentType {
    AdvancedSpanish,
    ModerateSpanish,
    BasicSpanish,
    InverseDiglot,
    EnglishDiglot,
}

#[derive(Debug, Clone, Default)]
pub struct BookGenerationResult {
    pub all_output_lemma_instances: Vec<String>,
    pub total_target_words: usize,
    pub total_base_words: usize,
    pub level_stats: HashMap<OutputLevel, usize>,
    pub segment_stats: HashMap<SegmentType, usize>,
    pub final_text_parts: Vec<String>,
    pub frontier_diagnostics: Option<FrontierDiagnostics>,
}

#[derive(Debug, Clone)]
pub struct FrontierRunConfig {
    pub enabled: bool,
    pub target_pct: f32,
    pub seed: u64,
    pub test_mode: bool,
    pub familiar_lemma_exclude_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct BoundaryPrepassMetrics {
    pub boundary_index: usize,
    pub sentence_start_1_based: usize,
    pub sentence_end_1_based_inclusive: usize,
    pub total_tokens: usize,
    pub unknown_tokens: usize,
}

#[derive(Debug, Clone, Default)]
pub struct FrontierDiagnostics {
    pub target_pct: f32,
    pub total_tokens: usize,
    pub target_frontier_tokens: usize,
    pub emitted_frontier_tokens: usize,
    pub deck_size: usize,
    pub pass_count: usize,
    pub steering_adjustment_count: usize,
}

#[derive(Debug, Clone)]
pub struct FrontierSliceConfig {
    pub target_pct: f32,
    pub expected_unknown_pct: f32,
    pub total_tokens: usize,
    pub seed: u64,
}

impl BoundaryPrepassMetrics {
    pub fn expected_unknown_pct(&self) -> f32 {
        if self.total_tokens == 0 {
            0.0
        } else {
            (self.unknown_tokens as f32 / self.total_tokens as f32) * 100.0
        }
    }
}

// MODIFIED: 'sim_v' parameter removed from the function signature.
pub fn generate_book_instance(
    numerical_chapter: &NumericalChapter,
    json_chapter: &JsonChapter,
    dictionary: &GlobalLemmaDictionary,
    bas_v: u32,
    mod_v: u32,
    adv_v: u32,
    inverse_diglot_threshold: f32,
    debug_markers: bool,
) -> Result<BookGenerationResult, Box<dyn Error>> {
    generate_book_instance_with_frontier(
        numerical_chapter,
        json_chapter,
        dictionary,
        bas_v,
        mod_v,
        adv_v,
        inverse_diglot_threshold,
        debug_markers,
        None,
    )
}

pub fn generate_book_instance_with_frontier(
    numerical_chapter: &NumericalChapter,
    json_chapter: &JsonChapter,
    dictionary: &GlobalLemmaDictionary,
    bas_v: u32,
    mod_v: u32,
    adv_v: u32,
    inverse_diglot_threshold: f32,
    debug_markers: bool,
    frontier_slice: Option<&FrontierSliceConfig>,
) -> Result<BookGenerationResult, Box<dyn Error>> {
    let mut result = BookGenerationResult::default();

    let mut profile = NumericalLearnerProfile::new();
    let ordered_lemmas = frequency_manager::get_ordered_lemmas();
    if bas_v < u32::MAX {
        for lemma_str in ordered_lemmas.iter().take(bas_v as usize) {
            if let Some(lemma_id) = dictionary.get_id(lemma_str) {
                profile.activate_lemma(lemma_id);
            }
        }
    }

    // MODIFIED: VLevelRecipe no longer includes 'sim'.
    let v_levels = VLevelRecipe {
        bas: bas_v,
        mod_v,
        adv: adv_v,
    };

    let mut frontier_engine: Option<FrontierEngine> = frontier_slice.map(|slice| {
        FrontierEngine::new(FrontierEngineConfig {
            target_pct: slice.target_pct,
            expected_unknown_pct: slice.expected_unknown_pct,
            total_tokens: slice.total_tokens,
            seed: slice.seed,
        })
    });

    for n_sentence in &numerical_chapter.sentences_numerical {
        let mut n_sentence_clone = n_sentence.clone();
        let output = core_algo::determine_and_annotate_sentence_expression_with_frontier(
            &mut n_sentence_clone,
            &profile,
            dictionary,
            &v_levels,
            inverse_diglot_threshold,
            frontier_engine.as_mut(),
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

        let generated_text = text_generator::generate_raw_text_from_levels(
            &[s_sentence_json],
            &[output.clone()],
            debug_markers,
        )?;

        result.final_text_parts.push(generated_text);
        *result.level_stats.entry(output.level).or_insert(0) += 1;

        match output.level {
            OutputLevel::AdvancedWeave => {
                if let Some(choices) = &output.l0_segment_choices {
                    for choice in choices {
                        *result
                            .segment_stats
                            .entry(match choice {
                                L0SegmentChoice::Adv(_) => SegmentType::AdvancedSpanish,
                                L0SegmentChoice::Mod(_) => SegmentType::ModerateSpanish,
                            })
                            .or_insert(0) += 1;
                    }
                }
            }
            OutputLevel::BasicTarget => {
                *result
                    .segment_stats
                    .entry(SegmentType::BasicSpanish)
                    .or_insert(0) += 1;
            }
            OutputLevel::BasicBaseDiglot => {
                *result
                    .segment_stats
                    .entry(SegmentType::EnglishDiglot)
                    .or_insert(0) += 1;
            }
            OutputLevel::InverseDiglot => {
                *result
                    .segment_stats
                    .entry(SegmentType::InverseDiglot)
                    .or_insert(0) += 1;
            }
        }
    }

    result.frontier_diagnostics = if let (Some(ref engine), Some(slice)) = (&frontier_engine, frontier_slice) {
        Some(FrontierDiagnostics {
            target_pct: slice.target_pct,
            total_tokens: slice.total_tokens,
            target_frontier_tokens: engine.target_frontier_tokens(),
            emitted_frontier_tokens: engine.emitted_frontier_tokens(),
            deck_size: engine.deck_size(),
            pass_count: engine.pass_count(),
            steering_adjustment_count: engine.steering_adjustment_count(),
        })
    } else {
        None
    };

    Ok(result)
}

pub fn compute_prepass_metrics_for_slice(
    numerical_chapter: &NumericalChapter,
    json_chapter: &JsonChapter,
    dictionary: &GlobalLemmaDictionary,
    bas_v: u32,
    mod_v: u32,
    adv_v: u32,
    inverse_diglot_threshold: f32,
) -> Result<(usize, usize), Box<dyn Error>> {
    let prepass_result = generate_book_instance(
        numerical_chapter,
        json_chapter,
        dictionary,
        bas_v,
        mod_v,
        adv_v,
        inverse_diglot_threshold,
        false,
    )?;

    let total_tokens = prepass_result.total_target_words + prepass_result.total_base_words;
    let unknown_tokens = prepass_result.total_base_words;
    Ok((total_tokens, unknown_tokens))
}

pub fn log_analysis_to_file(
    log_file_path: &Path,
    book_instance_unique_id: &str,
    result: &BookGenerationResult,
    avd_score: f64,
    start_v_recipe: Option<VLevelRecipe>,
    end_v_recipe: Option<VLevelRecipe>,
    start_l_recipe: Option<LLevelRecipe>,
    end_l_recipe: Option<LLevelRecipe>,
    frontier_config: Option<&FrontierRunConfig>,
    boundary_prepass_metrics: Option<&[BoundaryPrepassMetrics]>,
    frontier_diagnostics_per_boundary: Option<&[FrontierDiagnostics]>,
) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)?;
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let total_output_words = result.total_target_words + result.total_base_words;
    let base_lang_pct = if total_output_words > 0 {
        (result.total_base_words as f32 / total_output_words as f32) * 100.0
    } else {
        0.0
    };

    writeln!(
        file,
        "--- Analysis for Book Instance: {book_instance_unique_id} (at {timestamp}) ---"
    )?;

    // --- NEW SECTION START ---
    if let (Some(start_v), Some(end_v), Some(start_l), Some(end_l)) =
        (start_v_recipe, end_v_recipe, start_l_recipe, end_l_recipe)
    {
        writeln!(file, "  Recipe Dynamics:")?;
        writeln!(
            file,
            "    Start L-Levels (bas/mod/adv): {:.1} / {:.1} / {:.1}",
            start_l.bas, start_l.mod_v, start_l.adv
        )?;
        writeln!(
            file,
            "    Start V-Levels (bas/mod/adv): {} / {} / {}",
            start_v.bas, start_v.mod_v, start_v.adv
        )?;
        writeln!(
            file,
            "    End   L-Levels (bas/mod/adv): {:.1} / {:.1} / {:.1}",
            end_l.bas, end_l.mod_v, end_l.adv
        )?;
        writeln!(
            file,
            "    End   V-Levels (bas/mod/adv): {} / {} / {}",
            end_v.bas, end_v.mod_v, end_v.adv
        )?;
    }
    // --- NEW SECTION END ---

    writeln!(file, "  AVD Score (Density-Weighted): {avd_score:.2}")?;
    writeln!(file, "  Output Word Count Summary:")?;
    writeln!(
        file,
        "    Total Target Words:  {:>5}",
        result.total_target_words
    )?;
    writeln!(
        file,
        "    Total Base Words:    {:>5}",
        result.total_base_words
    )?;
    writeln!(file, "    -------------------------")?;
    writeln!(file, "    Total Output Words:  {total_output_words:>5}")?;
    writeln!(file, "    Base Lang Pct:       {base_lang_pct:>5.2}%")?;

    if let Some(cfg) = frontier_config {
        writeln!(file, "\n  Frontier Settings:")?;
        writeln!(file, "    Enabled:             {}", cfg.enabled)?;
        writeln!(file, "    Target Pct:          {:>5.2}%", cfg.target_pct)?;
        writeln!(file, "    Seed:                {}", cfg.seed)?;
        writeln!(file, "    Test Mode:           {}", cfg.test_mode)?;
        writeln!(
            file,
            "    Familiar Exclude N:  {}",
            cfg.familiar_lemma_exclude_count
        )?;
    }

    if let Some(boundaries) = boundary_prepass_metrics {
        if !boundaries.is_empty() {
            writeln!(file, "\n  Per-Boundary Pre-Pass Calibration:")?;
            for (i, b) in boundaries.iter().enumerate() {
                let low_sample = if b.total_tokens < 100 { " [LOW SAMPLE]" } else { "" };
                writeln!(
                    file,
                    "    B#{:02} S{}-S{} | total={} unknown={} expected_unknown={:.2}%{}",
                    b.boundary_index,
                    b.sentence_start_1_based,
                    b.sentence_end_1_based_inclusive,
                    b.total_tokens,
                    b.unknown_tokens,
                    b.expected_unknown_pct(),
                    low_sample,
                )?;
                if let Some(diags) = frontier_diagnostics_per_boundary.and_then(|v| v.get(i)) {
                    let realized_pct = if diags.total_tokens > 0 {
                        (diags.emitted_frontier_tokens as f32 / diags.total_tokens as f32) * 100.0
                    } else {
                        0.0
                    };
                    writeln!(
                        file,
                        "    B#{:02} Frontier | target={:.2}% realized={:.2}% emitted={}/{} deck={}/pass={} steered={}",
                        b.boundary_index,
                        diags.target_pct,
                        realized_pct,
                        diags.emitted_frontier_tokens,
                        diags.target_frontier_tokens,
                        diags.deck_size,
                        diags.pass_count,
                        diags.steering_adjustment_count,
                    )?;
                }
            }
        }
    }

    let total_sentences = result.level_stats.values().sum::<usize>();
    if total_sentences > 0 {
        let total_sentences_float = total_sentences as f32;
        writeln!(file, "\n  Sentence Level Distribution:")?;
        let l0_count = *result
            .level_stats
            .get(&OutputLevel::AdvancedWeave)
            .unwrap_or(&0);
        let l1_bt_count = *result
            .level_stats
            .get(&OutputLevel::BasicTarget)
            .unwrap_or(&0);
        let l1_id_count = *result
            .level_stats
            .get(&OutputLevel::InverseDiglot)
            .unwrap_or(&0);
        let l1_bb_count = *result
            .level_stats
            .get(&OutputLevel::BasicBaseDiglot)
            .unwrap_or(&0);

        writeln!(
            file,
            "    L0 Advanced Weave: {:>5} sentences ({:>6.2}%)",
            l0_count,
            (l0_count as f32 / total_sentences_float) * 100.0
        )?;
        writeln!(
            file,
            "    L1 Basic Target:   {:>5} sentences ({:>6.2}%)",
            l1_bt_count,
            (l1_bt_count as f32 / total_sentences_float) * 100.0
        )?;
        writeln!(
            file,
            "    L1 Inverse Diglot: {:>5} sentences ({:>6.2}%)",
            l1_id_count,
            (l1_id_count as f32 / total_sentences_float) * 100.0
        )?;
        writeln!(
            file,
            "    L1 Basic Diglot:   {:>5} sentences ({:>6.2}%)",
            l1_bb_count,
            (l1_bb_count as f32 / total_sentences_float) * 100.0
        )?;
    }

    let total_segments = result.segment_stats.values().sum::<usize>();
    if total_segments > 0 {
        writeln!(
            file,
            "\n  Segment/Sentence Type Distribution (Total: {total_segments} units):"
        )?;
        let total_segments_float = total_segments as f32;
        let ordered_segment_types = [
            (SegmentType::AdvancedSpanish, "Adv. Target Segments"),
            (SegmentType::ModerateSpanish, "Mod. Target Segments"),
            (SegmentType::BasicSpanish, "Basic Target Sentences"),
            (SegmentType::InverseDiglot, "Inverse Diglot Sentences"),
            (SegmentType::EnglishDiglot, "Base Diglot Sentences"),
        ];

        for (seg_type, label) in ordered_segment_types {
            if let Some(count) = result.segment_stats.get(&seg_type) {
                writeln!(
                    file,
                    "    {:<25} {:>5} units ({:>6.2}%)",
                    label,
                    count,
                    (*count as f32 / total_segments_float) * 100.0
                )?;
            }
        }
    }

    writeln!(
        file,
        "----------------------------------------------------------------------\n"
    )?;
    Ok(())
}

// MODIFIED: Removed 'sim_v' field.
#[derive(Debug, Clone, Default)]
struct ProcessingState {
    bas_v: u32,
    mod_v: u32,
    adv_v: u32,
    user_level: Option<u32>,
}

fn parse_level_value(s: &str) -> u32 {
    if s.eq_ignore_ascii_case("exhausted") {
        u32::MAX
    } else {
        s.parse().unwrap_or(0)
    }
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
    let absolute_path =
        fs::canonicalize(sequence_path).unwrap_or_else(|_| sequence_path.to_path_buf());
    println!(
        "[DEBUG] Attempting to open sequence file at absolute path: {}",
        absolute_path.display()
    );
    let sequence_file = File::open(sequence_path)?;

    for line_result in BufReader::new(sequence_file).lines() {
        let line = line_result?.trim().to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('%') {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let command = parts.first().cloned().unwrap_or("");
            if command == "%u-level" && parts.len() == 2 {
                state.user_level = Some(parts[1].parse()?);
                state.bas_v = 0;
                state.mod_v = 0;
                state.adv_v = 0;
                println!("[CMD] Set User Level to: {}", state.user_level.unwrap());
            } else if (command == "%levels" || command == "%level") && parts.len() == 4 {
                state.bas_v = parse_level_value(parts[1]);
                state.mod_v = parse_level_value(parts[2]);
                state.adv_v = parse_level_value(parts[3]);
                state.user_level = None;
                println!(
                    "[CMD] Set Manual Levels to: bas={}, mod={}, adv={}",
                    state.bas_v, state.mod_v, state.adv_v
                );
            } else {
                eprintln!("[WARN] Unknown or malformed command: {line}");
            }
            continue;
        }

        let book_stem = line;
        println!("\n--- Processing Book: {book_stem} ---");

        let json_file_path = project_config
            .content_project_dir_path()
            .join(input_json_dir)
            .join(format!("{book_stem}.json"));
        let json_content = fs::read_to_string(&json_file_path)?;
        let json_chapter = json_parser::parse_chapter_from_json(&json_content)?;

        let mut dictionary = GlobalLemmaDictionary::new();
        dictionary.populate_from_json_chapter(&json_chapter);
        let (numerical_chapter, _) =
            preprocessor::json_chapter_to_numerical(&json_chapter, &mut dictionary);

        let mut full_book_result = BookGenerationResult::default();
        let filename: String;

        // --- MODIFIED SECTION START ---
        // Variables to hold the recipes for logging
        let mut start_v_recipe = None;
        let mut end_v_recipe = None;
        let mut start_l_recipe = None;
        let mut end_l_recipe = None;

        if let Some(u_level) = state.user_level {
            let u_level_map =
                json_chapter
                    .u_level_maps
                    .get(&u_level.to_string())
                    .ok_or(format!(
                        "U-Level '{u_level}' not found in map for book '{book_stem}'"
                    ))?;

            // Capture the start and end recipes
            start_v_recipe = u_level_map.map.first().map(|e| e.recipe.clone());
            end_v_recipe = u_level_map.map.last().map(|e| e.recipe.clone());
            start_l_recipe = u_level_map.map.first().map(|e| e.l_level_recipe.clone());
            end_l_recipe = u_level_map.map.last().map(|e| e.l_level_recipe.clone());

            let end_level_for_range = (u_level_map.end_level - 1.0).floor() as u32;
            filename = if end_level_for_range > u_level {
                format!("{book_stem}_UL{u_level}-{end_level_for_range}.txt")
            } else {
                format!("{book_stem}_UL{u_level}.txt")
            };

            for (i, entry) in u_level_map.map.iter().enumerate() {
                let start_idx = entry.start_sentence_idx;
                let end_idx = if i + 1 < u_level_map.map.len() {
                    u_level_map.map[i + 1].start_sentence_idx
                } else {
                    numerical_chapter.sentences_numerical.len()
                };

                if start_idx >= end_idx {
                    continue;
                }

                let mut numerical_slice = numerical_chapter.clone();
                numerical_slice.sentences_numerical =
                    numerical_chapter.sentences_numerical[start_idx..end_idx].to_vec();

                let mut json_slice = json_chapter.clone();
                json_slice.content_blocks = json_chapter
                    .content_blocks
                    .iter()
                    .filter_map(|cb| match cb {
                        JsonContentBlock::Sentence(s) => {
                            if numerical_slice
                                .sentences_numerical
                                .iter()
                                .any(|ns| ns.sentence_id_str == s.s_id)
                            {
                                Some(JsonContentBlock::Sentence(s.clone()))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    })
                    .collect();

                let recipe = &entry.recipe;

                let slice_result = generate_book_instance(
                    &numerical_slice,
                    &json_slice,
                    &dictionary,
                    recipe.bas,
                    recipe.mod_v,
                    recipe.adv,
                    inverse_diglot_threshold,
                    debug_markers,
                )?;

                full_book_result
                    .final_text_parts
                    .extend(slice_result.final_text_parts);
                full_book_result
                    .all_output_lemma_instances
                    .extend(slice_result.all_output_lemma_instances);
                full_book_result.total_target_words += slice_result.total_target_words;
                full_book_result.total_base_words += slice_result.total_base_words;
                for (level, count) in slice_result.level_stats {
                    *full_book_result.level_stats.entry(level).or_insert(0) += count;
                }
                for (seg_type, count) in slice_result.segment_stats {
                    *full_book_result.segment_stats.entry(seg_type).or_insert(0) += count;
                }
            }
        } else {
            filename = format!(
                "{}_B{}_M{}_A{}.txt",
                book_stem, state.bas_v, state.mod_v, state.adv_v
            )
            .replace(&u32::MAX.to_string(), "EX");

            full_book_result = generate_book_instance(
                &numerical_chapter,
                &json_chapter,
                &dictionary,
                state.bas_v,
                state.mod_v,
                state.adv_v,
                inverse_diglot_threshold,
                debug_markers,
            )?;
        }

        let metrics = TextMetrics::new(
            &full_book_result.all_output_lemma_instances,
            full_book_result.total_base_words,
        );
        let avd_score = metrics.calculate_avd_score();

        let final_raw_text = full_book_result.final_text_parts.join("\n\n");
        let final_cleaned_text = text_generator::clean_text_for_tts(&final_raw_text);
        fs::write(tts_output_dir.join(&filename), final_cleaned_text)?;
        println!("  -> Saved TTS file to: {filename}");

        // Pass the optional recipes to the logger
        log_analysis_to_file(
            &analysis_log_path,
            &filename,
            &full_book_result,
            avd_score,
            start_v_recipe,
            end_v_recipe,
            start_l_recipe,
            end_l_recipe,
            None,
            None,
            None,
        )?;
        // --- MODIFIED SECTION END ---
    }

    println!("\n[INFO] Batch generation job finished.");
    Ok(())
}
