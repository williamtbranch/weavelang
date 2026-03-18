// src/simulation/calibrator.rs
use super::{
    dictionary::GlobalLemmaDictionary,
    frequency_manager,
    metrics::TextMetrics,
    numerical_types::{LLevelRecipe, NumericalChapter, VLevelRecipe},
    preprocessor,
};
use crate::{
    corpus_generator,
    types::json_types::{JsonChapterForParsing, JsonCurriculumMap, JsonCurriculumMapEntry},
    JsonChapter, JsonContentBlock,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    collections::HashMap,
    error::Error,
    fs::{self, File},
    io::Write,
    path::Path,
};

// --- Tunable Parameters ---
const LADDER_LINEAR_THRESHOLD: u32 = 500;
const LADDER_PERCENTAGE_STEP: f32 = 1.01;
const WORDS_PER_HOUR: f64 = 6.0;
const INITIAL_LEARNING_THRESHOLD_WORDS: u32 = 10;
const INITIAL_LEARNING_RATE_MULTIPLIER: f64 = 2.0;

// --- Basic-tier ramp parameters ---
// Below RAMP_START, bas == v (no ramp).
// Between RAMP_START and RAMP_END, bas pulls ahead on a power curve.
// Above RAMP_END, bas is clamped at max_rank.
const RAMP_START: u32 = 2000;
const RAMP_END: u32 = 4000;
const RAMP_EXPONENT: f64 = 2.0;

// --- Structs for Data Serialization ---

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct ULevelAnalysisEntry {
    u_level: f32,
    target_avd: f64,
    actual_avd: f64,
    recipe: VLevelRecipe,
    l_level_recipe: LLevelRecipe,
}
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct ULevelAnalysisData {
    u_level_map: Vec<ULevelAnalysisEntry>,
}

// --- AVD Formula (Unchanged) ---
const A_FIT: f64 = 4.15;
const B_FIT: f64 = 0.02;

fn get_avd_from_user_level(user_level: f32) -> f64 {
    let avd_score = ((user_level as f64 - B_FIT) / A_FIT).exp() - 1.0;
    avd_score.max(0.0)
}

/// Map a unified vocabulary level `v` to the basic tier's V-level.
/// Basic vocabulary is always a superset, so it ramps ahead of mod/adv.
fn ramp_basic_v(v: u32) -> u32 {
    let max_rank = frequency_manager::get_max_rank();
    if v <= RAMP_START {
        return v;
    }
    if v >= RAMP_END {
        return max_rank;
    }
    let t = (v - RAMP_START) as f64 / (RAMP_END - RAMP_START) as f64;
    let ramped = RAMP_START as f64 + t.powf(RAMP_EXPONENT) * (max_rank - RAMP_START) as f64;
    (ramped.round() as u32).min(max_rank)
}

fn generate_vocabulary_ladder() -> Vec<u32> {
    let mut ladder = Vec::new();
    let max_rank = frequency_manager::get_max_rank();
    for i in 1..=LADDER_LINEAR_THRESHOLD {
        if i > max_rank {
            break;
        }
        ladder.push(i);
    }
    let mut current_rank = LADDER_LINEAR_THRESHOLD;
    while current_rank < max_rank {
        let next_rank =
            ((current_rank as f32 * LADDER_PERCENTAGE_STEP).ceil() as u32).max(current_rank + 1);
        if next_rank > max_rank {
            ladder.push(max_rank);
            break;
        }
        ladder.push(next_rank);
        current_rank = next_rank;
    }
    ladder
}

pub fn run_unified_calibration(
    book_json_path: &Path,
    output_path: &Path,
    output_debug_path: Option<&Path>,
    master_avd_scale_path: &Path,
    max_level: u32,
) -> Result<(), Box<dyn Error>> {
    println!(
        "[INFO] Starting unified calibration process for '{}'...",
        book_json_path.display()
    );

    let json_content = fs::read_to_string(book_json_path)?;

    let json_chapter_for_parsing: JsonChapterForParsing = serde_json::from_str(&json_content)?;

    let json_chapter = JsonChapter {
        book_meta: json_chapter_for_parsing.book_meta,
        content_blocks: json_chapter_for_parsing.content_blocks,
        u_level_maps: HashMap::new(),
    };

    let mut dictionary = GlobalLemmaDictionary::new();
    dictionary.populate_from_json_chapter(&json_chapter);
    let (numerical_chapter, _) =
        preprocessor::json_chapter_to_numerical(&json_chapter, &mut dictionary);

    println!("  -> Phase A: Pre-computing unified AVD cache...");
    let ladder = generate_vocabulary_ladder();
    let avd_cache = build_unified_avd_cache(
        &numerical_chapter, &json_chapter, &dictionary, &ladder,
    )?;

    println!("  -> Phase B: Walking U-Levels...");
    let u_level_analysis = walk_u_levels(max_level, &avd_cache)?;

    if let Some(debug_path) = output_debug_path {
        let mut file = File::create(debug_path)?;
        file.write_all(serde_json::to_string_pretty(&u_level_analysis)?.as_bytes())?;
        println!(
            "  -> Saved detailed analysis file to '{}'",
            debug_path.display()
        );
    }

    println!("  -> Phase C: Generating final curriculum maps...");
    let master_scale = parse_master_avd_scale(master_avd_scale_path)?;
    let curriculum_maps = generate_curriculum_maps_from_scale(
        max_level,
        &u_level_analysis,
        &json_chapter,
        &master_scale,
        None,
    )?;

    println!("  -> Finalizing: Merging curriculum maps into book JSON...");
    let mut book_json_value: JsonValue = serde_json::from_str(&json_content)?;
    if let Some(obj) = book_json_value.as_object_mut() {
        obj.insert(
            "u_level_maps".to_string(),
            serde_json::to_value(curriculum_maps)?,
        );
    }
    let mut file = File::create(output_path)?;
    file.write_all(serde_json::to_string_pretty(&book_json_value)?.as_bytes())?;

    println!(
        "\n[SUCCESS] Unified calibration complete. Final book data saved to '{}'",
        output_path.display()
    );
    Ok(())
}

/// In-memory calibration: takes a pre-built `JsonChapter` and a master AVD
/// scale (Vec of V-level boundaries, one per user level, parsed from CSV),
/// runs the full calibration pipeline, and returns the curriculum maps ready
/// to be stored in `AppState::book_map`.
///
/// This is the entry point used by the GUI / terminal `calibrate` command.
pub fn calibrate_from_chapter(
    json_chapter: &JsonChapter,
    master_scale: &[u32],
    max_level: u32,
    total_sentences_hint: Option<usize>,
) -> Result<HashMap<String, JsonCurriculumMap>, Box<dyn Error>> {
    println!("[INFO] Starting in-memory calibration...");

    let mut dictionary = GlobalLemmaDictionary::new();
    dictionary.populate_from_json_chapter(json_chapter);
    let (numerical_chapter, _) =
        preprocessor::json_chapter_to_numerical(json_chapter, &mut dictionary);

    println!("  -> Phase A: Pre-computing unified AVD cache...");
    let ladder = generate_vocabulary_ladder();
    let avd_cache = build_unified_avd_cache(
        &numerical_chapter, json_chapter, &dictionary, &ladder,
    )?;

    println!("  -> Phase B: Walking U-Levels...");
    let u_level_analysis = walk_u_levels(max_level, &avd_cache)?;

    println!("  -> Phase C: Generating final curriculum maps...");
    let curriculum_maps = generate_curriculum_maps_from_scale(
        max_level,
        &u_level_analysis,
        json_chapter,
        master_scale,
        total_sentences_hint,
    )?;

    let level_count = curriculum_maps.len();
    println!("[SUCCESS] In-memory calibration complete. {} start-level maps generated.", level_count);
    Ok(curriculum_maps)
}

/// Parse a master AVD scale CSV file and return the V-level boundaries (one per user level).
pub fn parse_master_avd_scale(path: &Path) -> Result<Vec<u32>, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let scale: Vec<u32> = content
        .lines()
        .skip(1) // header row
        .map(|line| {
            let parts: Vec<_> = line.split(',').collect();
            parts[1].parse::<u32>().unwrap_or(0)
        })
        .collect();
    if scale.is_empty() {
        return Err("Master AVD scale file is empty or malformed.".into());
    }
    Ok(scale)
}

/// Phase A: Build a single unified AVD cache.
/// For each vocabulary level `v` on the ladder, measure AVD with
/// recipe { bas: ramp(v), mod_v: v, adv: v }.
fn build_unified_avd_cache(
    nc: &NumericalChapter,
    jc: &JsonChapter,
    dict: &GlobalLemmaDictionary,
    ladder: &[u32],
) -> Result<Vec<(u32, f64)>, Box<dyn Error>> {
    let mut cache = Vec::with_capacity(ladder.len());
    for (i, &v) in ladder.iter().enumerate() {
        let bas = ramp_basic_v(v);
        let res = corpus_generator::generate_book_instance(
            nc, jc, dict, bas, v, v, 0.4, false,
        )?;
        let avd = TextMetrics::new(&res.all_output_lemma_instances, res.total_base_words)
            .calculate_avd_score();
        cache.push((v, avd));
        if (i + 1) % 50 == 0 || (i + 1) == ladder.len() {
            print!(
                "\r     ...pre-computing: {:.1}%",
                (i + 1) as f32 / ladder.len() as f32 * 100.0
            );
            std::io::stdout().flush()?;
        }
    }
    println!();
    Ok(cache)
}

/// Phase B: For each whole user-level 0..=max_level, find the smallest
/// unified V-level whose AVD meets or exceeds the target.
///
/// Post-processing:
///  - Enforce a minimum vocab floor: level N gets at least N for each tier.
///  - Detect the "natural level": the first level where all tiers reach
///    max_rank.  Pull that max recipe back one level and trim the rest.
fn walk_u_levels(
    max_level: u32,
    avd_cache: &[(u32, f64)],
) -> Result<ULevelAnalysisData, Box<dyn Error>> {
    let max_rank = frequency_manager::get_max_rank();
    let mut analysis = ULevelAnalysisData::default();

    for ul in 0..=max_level {
        let target_avd = get_avd_from_user_level(ul as f32);

        // Find the first cache entry whose AVD meets the target.
        let (v, actual_avd) = avd_cache
            .iter()
            .find(|&&(_, avd)| avd >= target_avd)
            .map_or_else(
                || {
                    // Vocabulary exhausted — use the last entry.
                    let last = avd_cache.last().unwrap();
                    (last.0, last.1)
                },
                |&(v, avd)| (v, avd),
            );

        let bas = ramp_basic_v(v);
        // Enforce minimum: level N must have at least N for each tier.
        let recipe = VLevelRecipe {
            bas: bas.max(ul),
            mod_v: v.max(ul),
            adv: v.max(ul),
        };
        let l_level_recipe = LLevelRecipe {
            bas: ul as f32,
            mod_v: ul as f32,
            adv: ul as f32,
        };

        analysis.u_level_map.push(ULevelAnalysisEntry {
            u_level: ul as f32,
            target_avd,
            actual_avd,
            recipe,
            l_level_recipe,
        });

        print!("\r     ...walking U-Level {ul}");
        std::io::stdout().flush()?;
    }
    println!();

    // --- Natural level detection ---
    // Find the first level where all three tiers are at max_rank.
    // Pull that max recipe back one level and trim everything after.
    let first_all_max = analysis.u_level_map.iter().position(|e| {
        e.recipe.bas >= max_rank && e.recipe.mod_v >= max_rank && e.recipe.adv >= max_rank
    });
    if let Some(max_idx) = first_all_max {
        // The natural level is one before the first all-max.
        // Promote the previous level to all-max so it becomes the ceiling.
        let natural_idx = if max_idx > 0 { max_idx - 1 } else { max_idx };
        let max_recipe = VLevelRecipe {
            bas: max_rank,
            mod_v: max_rank,
            adv: max_rank,
        };
        // Copy AVD from the all-max entry for accurate diagnostics.
        let max_avd = analysis.u_level_map[max_idx].actual_avd;
        analysis.u_level_map[natural_idx].recipe = max_recipe;
        analysis.u_level_map[natural_idx].actual_avd = max_avd;
        // Trim everything after the natural level.
        analysis.u_level_map.truncate(natural_idx + 1);
        println!(
            "  -> Natural level: UL{} (all tiers at max_rank {})",
            analysis.u_level_map[natural_idx].u_level as u32, max_rank
        );
    }

    Ok(analysis)
}

fn generate_curriculum_maps_from_scale(
    max_level: u32,
    u_level_analysis: &ULevelAnalysisData,
    json_chapter: &JsonChapter,
    master_scale: &[u32],
    total_sentences_hint: Option<usize>,
) -> Result<HashMap<String, JsonCurriculumMap>, Box<dyn Error>> {
    let mut curriculum_maps = HashMap::new();

    // The analysis is already trimmed to the natural level by walk_u_levels.
    // Use the last entry as the effective max.
    let natural_peak = u_level_analysis
        .u_level_map
        .last()
        .map(|e| e.u_level.floor() as u32)
        .unwrap_or(1);

    // When extrapolating (chapter mode), the vocabulary-based peak may be
    // artificially low.  Trust the master_scale length as a lower bound.
    let effective_max = if total_sentences_hint.is_some() {
        let scale_max = master_scale.len().saturating_sub(1) as u32;
        natural_peak.max(scale_max).min(max_level)
    } else {
        natural_peak.min(max_level)
    };
    println!(
        "  -> Effective max: UL{} (formula max {}{})",
        effective_max,
        max_level,
        if total_sentences_hint.is_some() {
            format!(", natural peak {}, extrapolating", natural_peak)
        } else {
            String::new()
        }
    );

    // Count actual (non-placeholder) words and sentences in the input.
    let actual_words: f64 = json_chapter
        .content_blocks
        .iter()
        .map(|cb| match cb {
            JsonContentBlock::Sentence(s) => s
                .tiers
                .iter()
                .find(|t| t.tier_id == "basic_base")
                .map_or(0, |t| t.full_text.split_whitespace().count()),
            _ => 0,
        })
        .sum::<usize>() as f64;
    let actual_sentences = json_chapter
        .content_blocks
        .iter()
        .filter(|cb| match cb {
            JsonContentBlock::Sentence(s) => s
                .tiers
                .iter()
                .any(|t| t.tier_id == "basic_base" && !t.full_text.trim().is_empty()),
            _ => false,
        })
        .count();

    let (total_words_in_book, total_sentences_in_book) = if let Some(hint) = total_sentences_hint {
        let avg_words_per_sentence = if actual_sentences > 0 {
            actual_words / actual_sentences as f64
        } else {
            15.0
        };
        let extrapolated_words = avg_words_per_sentence * hint as f64;
        println!(
            "  -> Extrapolating: {:.0} actual words in {} sentences -> avg {:.1} w/s -> {:.0} estimated total ({} sentences)",
            actual_words, actual_sentences, avg_words_per_sentence, extrapolated_words, hint
        );
        (extrapolated_words, hint)
    } else {
        (actual_words, actual_sentences)
    };

    for start_level in 1..=effective_max {
        let mut time_costs: Vec<(u32, f64)> = Vec::new();
        let mut cumulative_time_cost = 0.0;
        let mut end_level = start_level;

        for level in start_level..effective_max {
            let v_start = *master_scale.get(level as usize - 1).unwrap_or(&0);
            let v_end = *master_scale.get(level as usize).unwrap_or(&0);
            let new_words = v_end.saturating_sub(v_start);
            let mut time_per_word = 1.0 / WORDS_PER_HOUR * 60.0;
            if v_end <= INITIAL_LEARNING_THRESHOLD_WORDS {
                time_per_word *= INITIAL_LEARNING_RATE_MULTIPLIER;
            }
            let level_time_cost = new_words as f64 * time_per_word * 150.0;

            if cumulative_time_cost + level_time_cost > total_words_in_book {
                break;
            }

            cumulative_time_cost += level_time_cost;
            time_costs.push((level, level_time_cost));
            end_level = level + 1;
        }

        if end_level == start_level {
            end_level = start_level + 1;
        }

        // Whole-level steps: one entry per UL (no micro-levels).
        let num_steps = (end_level - start_level) as usize;
        let mut map = Vec::new();
        let mut sentence_cursor = 0;
        for step in 0..num_steps {
            let level = (start_level + step as u32) as f32;

            let analysis_entry = u_level_analysis
                .u_level_map
                .iter()
                .min_by(|a, b| {
                    (a.u_level - level)
                        .abs()
                        .partial_cmp(&(b.u_level - level).abs())
                        .unwrap()
                })
                .ok_or("Could not find closest analysis entry for level")?;

            map.push(JsonCurriculumMapEntry {
                level,
                start_sentence_idx: sentence_cursor,
                recipe: analysis_entry.recipe.clone(),
                l_level_recipe: analysis_entry.l_level_recipe.clone(),
                target_avd: analysis_entry.target_avd,
                actual_avd: analysis_entry.actual_avd,
            });

            let proportion_of_book = if cumulative_time_cost > 0.0 {
                time_costs
                    .iter()
                    .find(|(l, _)| *l == start_level + step as u32)
                    .map_or(0.0, |(_, cost)| *cost)
                    / cumulative_time_cost
            } else {
                1.0 / num_steps as f64
            };

            sentence_cursor = (sentence_cursor as f64
                + total_sentences_in_book as f64 * proportion_of_book)
                .round() as usize;
            if sentence_cursor >= total_sentences_in_book {
                sentence_cursor = total_sentences_in_book - 1;
            }
        }

        curriculum_maps.insert(
            start_level.to_string(),
            JsonCurriculumMap {
                end_level: end_level as f32,
                map,
            },
        );
    }
    Ok(curriculum_maps)
}
