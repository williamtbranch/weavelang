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
    types::json_types::{JsonChapterForParsing, JsonCurriculumMap, JsonCurriculumMapEntry, TierId},
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
struct ULevelAnalysisData { u_level_map: Vec<ULevelAnalysisEntry> }

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct LLevelRangeEntry { l_level: f32, target_avd: f64, actual_avd: f64, v_low: u32 }

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct LLevelTable { tier_id: String, natural_exhaustion_level: f32, levels: Vec<LLevelRangeEntry> }

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct BookAnalysisData {
    l_level_tables: BookLLevelTables,
    u_level_analysis: ULevelAnalysisData,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct BookLLevelTables { basic: LLevelTable, moderate: LLevelTable, advanced: LLevelTable }

#[derive(Debug, Clone, Copy, PartialEq)]
enum CalibrationPhase { BasMod, ModAdv, AdvOnly, Complete }

// --- AVD Formula (Unchanged) ---
const A_FIT: f64 = 4.15;
const B_FIT: f64 = 0.02;

fn get_avd_from_user_level(user_level: f32) -> f64 {
    let avd_score = ((user_level as f64 - B_FIT) / A_FIT).exp() - 1.0;
    avd_score.max(0.0)
}

fn generate_vocabulary_ladder() -> Vec<u32> {
    let mut ladder = Vec::new();
    let max_rank = frequency_manager::get_max_rank();
    for i in 1..=LADDER_LINEAR_THRESHOLD { if i > max_rank { break; } ladder.push(i); }
    let mut current_rank = LADDER_LINEAR_THRESHOLD;
    while current_rank < max_rank {
        let next_rank = ((current_rank as f32 * LADDER_PERCENTAGE_STEP).ceil() as u32).max(current_rank + 1);
        if next_rank > max_rank { ladder.push(max_rank); break; }
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
    println!("[INFO] Starting unified calibration process for '{}'...", book_json_path.display());
    
    let json_content = fs::read_to_string(book_json_path)?;
    
    let json_chapter_for_parsing: JsonChapterForParsing = serde_json::from_str(&json_content)?;
    
    let json_chapter = JsonChapter {
        book_meta: json_chapter_for_parsing.book_meta,
        content_blocks: json_chapter_for_parsing.content_blocks,
        u_level_maps: HashMap::new(), 
    };

    let mut dictionary = GlobalLemmaDictionary::new();
    dictionary.populate_from_json_chapter(&json_chapter);
    let (numerical_chapter, _) = preprocessor::json_chapter_to_numerical(&json_chapter, &mut dictionary);

    println!("  -> Phase A: Pre-computing AVD cache...");
    let ladder = generate_vocabulary_ladder();
    let mut avd_cache: HashMap<TierId, Vec<(u32, f64)>> = HashMap::new();
    
    for tier_id in [TierId::Basic, TierId::Moderate, TierId::Advanced] {
        let mut tier_results = Vec::new();
        for (i, &v_level) in ladder.iter().enumerate() {
            let avd = generate_and_measure(&numerical_chapter, &json_chapter, &dictionary, tier_id, v_level)?;
            tier_results.push((v_level, avd));
            if (i + 1) % 50 == 0 || (i + 1) == ladder.len() {
                print!("\r     ...pre-computing for {:?}: {:.1}%", tier_id, (i + 1) as f32 / ladder.len() as f32 * 100.0);
                std::io::stdout().flush()?;
            }
        }
        println!();
        avd_cache.insert(tier_id, tier_results);
    }

    println!("  -> Phase B: Synthesizing L-Level tables and running U-Level state machine...");
    let l_tables = synthesize_l_level_tables(&avd_cache, max_level)?;
    let u_level_analysis = run_u_level_state_machine(max_level, &l_tables, &numerical_chapter, &json_chapter, &dictionary)?;

    if let Some(debug_path) = output_debug_path {
        let debug_data = BookAnalysisData { l_level_tables: l_tables, u_level_analysis: u_level_analysis.clone() };
        let mut file = File::create(debug_path)?;
        file.write_all(serde_json::to_string_pretty(&debug_data)?.as_bytes())?;
        println!("  -> Saved detailed analysis file to '{}'", debug_path.display());
    }

    println!("  -> Phase C: Generating final curriculum maps...");
    let curriculum_maps = generate_curriculum_maps(max_level, &u_level_analysis, &json_chapter, master_avd_scale_path)?;

    println!("  -> Finalizing: Merging curriculum maps into book JSON...");
    let mut book_json_value: JsonValue = serde_json::from_str(&json_content)?;
    if let Some(obj) = book_json_value.as_object_mut() {
        obj.insert("u_level_maps".to_string(), serde_json::to_value(curriculum_maps)?);
    }
    let mut file = File::create(output_path)?;
    file.write_all(serde_json::to_string_pretty(&book_json_value)?.as_bytes())?;
    
    println!("\n[SUCCESS] Unified calibration complete. Final book data saved to '{}'", output_path.display());
    Ok(())
}

fn synthesize_l_level_tables(
    avd_cache: &HashMap<TierId, Vec<(u32, f64)>>,
    max_level: u32,
) -> Result<BookLLevelTables, Box<dyn Error>> {
    let mut tables = BookLLevelTables::default();
    let num_l_steps = max_level as usize * 10;
    
    for (tier_id, tier_cache) in avd_cache {
        let mut levels = Vec::new();
        let mut natural_exhaustion_level = 0.0;
        
        for i in 0..=num_l_steps {
            let l_level = i as f32 / 10.0;
            let target_avd = get_avd_from_user_level(l_level);
            let (v_low, actual_avd) = tier_cache.iter()
                .find(|&&(_, avd)| avd >= target_avd)
                .map_or((u32::MAX, tier_cache.last().unwrap().1), |&(v, avd)| (v, avd));

            levels.push(LLevelRangeEntry { l_level, target_avd, actual_avd, v_low });

            if v_low == u32::MAX { natural_exhaustion_level = l_level; break; }
        }
        if natural_exhaustion_level == 0.0 { natural_exhaustion_level = max_level as f32; }

        let table = LLevelTable { tier_id: format!("{:?}", tier_id), natural_exhaustion_level, levels };
        match tier_id {
            TierId::Basic => tables.basic = table,
            TierId::Moderate => tables.moderate = table,
            TierId::Advanced => tables.advanced = table,
        }
    }
    Ok(tables)
}

fn run_u_level_state_machine(
    max_level: u32,
    l_tables: &BookLLevelTables,
    numerical_chapter: &NumericalChapter,
    json_chapter: &JsonChapter,
    dictionary: &GlobalLemmaDictionary,
) -> Result<ULevelAnalysisData, Box<dyn Error>> {
    let mut u_level_analysis = ULevelAnalysisData::default();
    let num_u_steps = max_level as usize * 10;
    let mut calculation_cache: HashMap<VLevelRecipe, f64> = HashMap::new();
    
    let mut last_best_l_recipe = LLevelRecipe::default();
    let mut last_best_v_recipe = VLevelRecipe::default();
    let mut last_best_avd = 0.0;
    
    let mut is_maxed_out = false;

    for i in 0..=num_u_steps {
        let current_u_level = i as f32 / 10.0;
        let target_avd = get_avd_from_user_level(current_u_level);

        if is_maxed_out {
            u_level_analysis.u_level_map.push(ULevelAnalysisEntry {
                u_level: current_u_level,
                target_avd,
                actual_avd: last_best_avd,
                recipe: last_best_v_recipe.clone(),
                l_level_recipe: last_best_l_recipe.clone(),
            });
            continue;
        }

        let mut candidate_l_recipes = Vec::new();
        let mut current_l_recipe = last_best_l_recipe.clone();
        
        for _ in 0..100 { 
            let (next_l_recipe, phase_complete) = advance_l_recipe(current_l_recipe, l_tables);
            if phase_complete { break; }
            candidate_l_recipes.push(next_l_recipe.clone());
            current_l_recipe = next_l_recipe;
        }

        let mut found_improvement = false;
        for candidate_l in candidate_l_recipes {
            let mut candidate_v = VLevelRecipe {
                bas: find_v_level_for_l_level(&l_tables.basic, candidate_l.bas),
                mod_v: find_v_level_for_l_level(&l_tables.moderate, candidate_l.mod_v),
                adv: find_v_level_for_l_level(&l_tables.advanced, candidate_l.adv),
            };

            let u_level_floor = current_u_level.floor() as u32;
            if candidate_v.bas < u_level_floor {
                candidate_v.bas = u_level_floor;
            }

            let candidate_avd = get_avd_for_recipe(numerical_chapter, json_chapter, dictionary, candidate_v.clone(), &mut calculation_cache)?;
            
            if candidate_avd <= target_avd {
                last_best_l_recipe = candidate_l;
                last_best_v_recipe = candidate_v;
                last_best_avd = candidate_avd;
                found_improvement = true;
            } else {
                break;
            }
        }
        
        if !found_improvement {
            let (_, phase_complete) = advance_l_recipe(last_best_l_recipe.clone(), l_tables);
            if phase_complete {
                 println!("\n[INFO] Book has reached its natural maximum AVD of {:.2} at U-Level {:.1}. Filling remaining levels.", last_best_avd, current_u_level);
                 is_maxed_out = true;
            }
        }

        u_level_analysis.u_level_map.push(ULevelAnalysisEntry {
            u_level: current_u_level,
            target_avd,
            actual_avd: last_best_avd,
            recipe: last_best_v_recipe.clone(),
            l_level_recipe: last_best_l_recipe.clone(), // <-- THIS IS THE KEY CHANGE
        });
        
        print!("\r     ...calibrating U-Level {:.1}", current_u_level);
        std::io::stdout().flush()?;
    }
    
    println!();
    Ok(u_level_analysis)
}

fn generate_curriculum_maps(
    max_level: u32,
    u_level_analysis: &ULevelAnalysisData,
    json_chapter: &JsonChapter,
    master_avd_scale_path: &Path,
) -> Result<HashMap<String, JsonCurriculumMap>, Box<dyn Error>> {
    let mut curriculum_maps = HashMap::new();
    let master_scale: Vec<_> = fs::read_to_string(master_avd_scale_path)?
        .lines().skip(1).map(|line| {
            let parts: Vec<_> = line.split(',').collect();
            parts[1].parse::<u32>().unwrap_or(0)
        }).collect();
    
    let total_words_in_book: f64 = json_chapter.content_blocks.iter().map(|cb| match cb {
        JsonContentBlock::Sentence(s) => s.tiers.iter().find(|t| t.tier_id == "basic_base").map_or(0, |t| t.full_text.split_whitespace().count()),
        _ => 0,
    }) .sum::<usize>() as f64;
    let total_sentences_in_book = json_chapter.content_blocks.iter().filter(|cb| matches!(cb, JsonContentBlock::Sentence(_))).count();

    for start_level in 1..=max_level {
        let mut time_costs: Vec<(u32, f64)> = Vec::new();
        let mut cumulative_time_cost = 0.0;
        let mut end_level = start_level;
        
        for level in start_level..max_level {
            let v_start = *master_scale.get(level as usize - 1).unwrap_or(&0);
            let v_end = *master_scale.get(level as usize).unwrap_or(&0);
            let new_words = v_end - v_start;
            let mut time_per_word = 1.0 / WORDS_PER_HOUR * 60.0;
            if v_end <= INITIAL_LEARNING_THRESHOLD_WORDS { time_per_word *= INITIAL_LEARNING_RATE_MULTIPLIER; }
            let level_time_cost = new_words as f64 * time_per_word * 150.0;
            
            if cumulative_time_cost + level_time_cost > total_words_in_book { break; }
            
            cumulative_time_cost += level_time_cost;
            time_costs.push((level, level_time_cost));
            end_level = level + 1;
        }

        if end_level == start_level { end_level = start_level + 1; }

        let mut map = Vec::new();
        let mut sentence_cursor = 0;
        for micro_level_step in 0..((end_level - start_level) * 10) {
            let micro_level = start_level as f32 + micro_level_step as f32 / 10.0;
            
            let analysis_entry = u_level_analysis
                .u_level_map
                .iter()
                .min_by(|a, b| (a.u_level - micro_level).abs().partial_cmp(&(b.u_level - micro_level).abs()).unwrap())
                .ok_or("Could not find closest analysis entry for micro-level")?;

            // --- THIS BLOCK IS THE KEY CHANGE ---
            map.push(JsonCurriculumMapEntry {
                level: micro_level,
                start_sentence_idx: sentence_cursor,
                recipe: analysis_entry.recipe.clone(),
                l_level_recipe: analysis_entry.l_level_recipe.clone(), // <-- ADD THIS LINE
            });
            
            let current_level_in_costs = start_level + (micro_level_step / 10);
            let proportion_of_book = if cumulative_time_cost > 0.0 {
                time_costs.iter().find(|(l, _)| *l == current_level_in_costs).map_or(0.0, |(_, cost)| *cost) / cumulative_time_cost / 10.0
            } else { 1.0 / ((end_level - start_level) * 10) as f64 };
            
            sentence_cursor = (sentence_cursor as f64 + total_sentences_in_book as f64 * proportion_of_book).round() as usize;
            if sentence_cursor >= total_sentences_in_book { sentence_cursor = total_sentences_in_book -1; }
        }
        
        curriculum_maps.insert(start_level.to_string(), JsonCurriculumMap { end_level: end_level as f32, map });
    }
    Ok(curriculum_maps)
}

fn find_v_level_for_l_level(table: &LLevelTable, l_level: f32) -> u32 {
    if l_level >= table.natural_exhaustion_level { return u32::MAX; }
    if l_level <= 0.0 { return 0; }
    table.levels.iter().find(|&entry| entry.l_level >= l_level).map_or(u32::MAX, |l| l.v_low)
}

// MODIFIED: 'sim' field removed from initializers.
fn generate_and_measure(nc: &NumericalChapter, jc: &JsonChapter, dict: &GlobalLemmaDictionary, tier: TierId, v: u32) -> Result<f64, Box<dyn Error>> {
    let vs = match tier {
        TierId::Basic => VLevelRecipe { bas: v, mod_v: 0, adv: 0 },
        TierId::Moderate => VLevelRecipe { bas: v, mod_v: v, adv: 0 },
        TierId::Advanced => VLevelRecipe { bas: v, mod_v: v, adv: v },
    };
    let r = corpus_generator::generate_book_instance(nc, jc, dict, vs.bas, vs.mod_v, vs.adv, 0.4, false)?;
    Ok(TextMetrics::new(&r.all_output_lemma_instances, r.total_base_words).calculate_avd_score())
}

// MODIFIED: Call to generate_book_instance no longer includes 'r.sim'.
fn get_avd_for_recipe(nc: &NumericalChapter, jc: &JsonChapter, dict: &GlobalLemmaDictionary, r: VLevelRecipe, c: &mut HashMap<VLevelRecipe, f64>) -> Result<f64, Box<dyn Error>> {
    if let Some(avd) = c.get(&r) { return Ok(*avd); }
    let res = corpus_generator::generate_book_instance(nc, jc, dict, r.bas, r.mod_v, r.adv, 0.4, false)?;
    let avd = TextMetrics::new(&res.all_output_lemma_instances, res.total_base_words).calculate_avd_score();
    c.insert(r, avd); Ok(avd)
}

fn advance_l_recipe(mut recipe: LLevelRecipe, l_tables: &BookLLevelTables) -> (LLevelRecipe, bool) {
    let phase = if recipe.adv >= l_tables.advanced.natural_exhaustion_level {
        CalibrationPhase::Complete
    } else if recipe.mod_v >= l_tables.moderate.natural_exhaustion_level {
        CalibrationPhase::AdvOnly
    } else if recipe.bas >= l_tables.basic.natural_exhaustion_level {
        CalibrationPhase::ModAdv
    } else {
        CalibrationPhase::BasMod
    };

    match phase {
        CalibrationPhase::BasMod => {
            recipe.bas = (recipe.bas * 10.0 + 1.0).round() / 10.0;
            recipe.mod_v = recipe.mod_v.max(recipe.bas);
        }
        CalibrationPhase::ModAdv => {
            recipe.mod_v = (recipe.mod_v * 10.0 + 1.0).round() / 10.0;
            recipe.adv = recipe.adv.max(recipe.mod_v);
        }
        CalibrationPhase::AdvOnly => {
            recipe.adv = (recipe.adv * 10.0 + 1.0).round() / 10.0;
        }
        CalibrationPhase::Complete => {
            return (recipe, true);
        }
    }
    
    recipe.bas = recipe.bas.min(l_tables.basic.natural_exhaustion_level);
    recipe.mod_v = recipe.mod_v.min(l_tables.moderate.natural_exhaustion_level);
    recipe.adv = recipe.adv.min(l_tables.advanced.natural_exhaustion_level);

    (recipe, false)
}