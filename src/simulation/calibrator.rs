// In src/simulation/calibrator.rs

use super::{
    dictionary::GlobalLemmaDictionary,
    frequency_manager,
    metrics::TextMetrics,
    numerical_types::{NumericalChapter, VLevelRecipe},
    preprocessor,
};
use crate::{corpus_generator, parsing::json_parser, types::json_types::TierId, JsonChapter};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    error::Error,
    fs::{self, File},
    io::Write,
    path::Path,
};

// --- Tunable Parameters for the Vocabulary Ladder ---
const LADDER_LINEAR_THRESHOLD: u32 = 500;
const LADDER_PERCENTAGE_STEP: f32 = 1.01;

// --- MOVED ALL STRUCT DEFINITIONS TO THE TOP ---
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct LLevelRecipe {
    pub sim: f32,
    pub bas: f32,
    pub mod_v: f32,
    pub adv: f32,
}
#[derive(Serialize, Deserialize, Debug)]
struct ULevelEntry {
    u_level: f32,
    target_avd: f64,
    actual_avd: f64,
    recipe: VLevelRecipe,
    l_level_recipe: LLevelRecipe,
}
#[derive(Serialize, Deserialize, Debug, Default)]
struct ULevelBookData {
    u_level_map: Vec<ULevelEntry>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum CalibrationPhase {
    SimBas,
    BasMod,
    ModAdv,
    AdvOnly,
    Complete,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
struct LLevelRangeEntry {
    l_level: f32,
    target_avd: f64,
    actual_avd: f64,
    v_low: u32,
    v_high: u32,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
struct LLevelTable {
    tier_id: String,
    natural_exhaustion_level: f32,
    levels: Vec<LLevelRangeEntry>,
}
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct BookCalibrationData {
    simple_tier: Option<LLevelTable>,
    basic_tier: Option<LLevelTable>,
    moderate_tier: Option<LLevelTable>,
    advanced_tier: Option<LLevelTable>,
}
// --- END OF STRUCT DEFINITIONS ---


// --- AVD Formula (Unchanged) ---
const A_FIT: f64 = 4.15;
const B_FIT: f64 = 0.02;

fn get_avd_from_user_level(user_level: f32) -> f64 {
    let avd_score = ((user_level as f64 - B_FIT) / A_FIT).exp() - 1.0;
    avd_score.max(0.0)
}

/// Generates the "Logarithmic Vocabulary Ladder" for the search space.
fn generate_vocabulary_ladder() -> Vec<u32> {
    let mut ladder = Vec::new();
    let max_rank = frequency_manager::get_max_rank();

    // Phase 1: Linear start
    for i in 1..=LADDER_LINEAR_THRESHOLD {
        if i > max_rank { break; }
        ladder.push(i);
    }

    // Phase 2: Logarithmic climb
    let mut current_rank = LADDER_LINEAR_THRESHOLD;
    while current_rank < max_rank {
        let next_rank_float = current_rank as f32 * LADDER_PERCENTAGE_STEP;
        let next_rank = (next_rank_float.ceil() as u32).max(current_rank + 1);
        
        if next_rank > max_rank {
            ladder.push(max_rank);
            break;
        }
        ladder.push(next_rank);
        current_rank = next_rank;
    }
    ladder
}

/// The new, single entry point for the entire calibration process.
pub fn run_unified_calibration(
    book_json_path: &Path,
    max_level: u32,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    println!("[INFO] Starting unified calibration process...");
    
    // --- 1. Standard Setup: Load book and frequency data ---
    let json_content = fs::read_to_string(book_json_path)?;
    let json_chapter = json_parser::parse_chapter_from_json(&json_content)?;
    let mut dictionary = GlobalLemmaDictionary::new();
    dictionary.populate_from_json_chapter(&json_chapter);
    let (numerical_chapter, _) =
        preprocessor::json_chapter_to_numerical(&json_chapter, &mut dictionary);

    // --- 2. Pre-computation: Generate the search space and the AVD cache ---
    println!("  -> Generating logarithmic vocabulary ladder...");
    let ladder = generate_vocabulary_ladder();
    println!("     Ladder has {} discrete V-level steps.", ladder.len());

    println!("  -> Pre-computing AVD scores for all tiers and ladder steps...");
    let mut avd_cache: HashMap<TierId, Vec<(u32, f64)>> = HashMap::new();
    let tiers_to_calibrate = [TierId::Simple, TierId::Basic, TierId::Moderate, TierId::Advanced];

    for tier_id in tiers_to_calibrate {
        let mut tier_results = Vec::new();
        for &v_level in &ladder {
            let avd = generate_and_measure(&numerical_chapter, &json_chapter, &dictionary, tier_id, v_level)?;
            tier_results.push((v_level, avd));
        }
        avd_cache.insert(tier_id, tier_results);
        println!("     ...completed pre-computation for {:?} tier.", tier_id);
    }

    // --- 3. Synthesize L-Level Tables from the cache ---
    let l_data = synthesize_l_level_data(&avd_cache, max_level)?;

    // --- 4. Run the U-Level State Machine using the L-Level tables for lookups ---
    println!("  -> Running U-Level mapping state machine...");
    let u_level_map = run_u_level_state_machine(max_level, &l_data, &numerical_chapter, &json_chapter, &dictionary)?;

    // --- 5. Save the final output ---
    let mut file = File::create(output_path)?;
    let json_string = serde_json::to_string_pretty(&u_level_map)?;
    file.write_all(json_string.as_bytes())?;
    
    println!("\n[SUCCESS] Unified calibration complete. Map saved to {}", output_path.display());
    Ok(())
}

/// Generates the L-Level tables by looking up values in the pre-computed AVD cache.
fn synthesize_l_level_data(
    avd_cache: &HashMap<TierId, Vec<(u32, f64)>>,
    max_level: u32,
) -> Result<BookCalibrationData, Box<dyn Error>> {
    let mut calibration_data = BookCalibrationData::default();
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

            levels.push(LLevelRangeEntry { l_level, target_avd, actual_avd, v_low, v_high: 0 });

            if v_low == u32::MAX {
                natural_exhaustion_level = l_level;
                break;
            }
        }

        if natural_exhaustion_level == 0.0 {
            natural_exhaustion_level = max_level as f32;
        }

        let l_level_table = LLevelTable {
            tier_id: format!("{:?}", tier_id),
            natural_exhaustion_level,
            levels,
        };
        
        match tier_id {
            TierId::Simple => calibration_data.simple_tier = Some(l_level_table),
            TierId::Basic => calibration_data.basic_tier = Some(l_level_table),
            TierId::Moderate => calibration_data.moderate_tier = Some(l_level_table),
            TierId::Advanced => calibration_data.advanced_tier = Some(l_level_table),
        }
    }
    Ok(calibration_data)
}

/// Runs the state machine to generate the final U-Level map.
fn run_u_level_state_machine(
    max_level: u32,
    l_data: &BookCalibrationData,
    numerical_chapter: &NumericalChapter,
    json_chapter: &JsonChapter,
    dictionary: &GlobalLemmaDictionary,
) -> Result<ULevelBookData, Box<dyn Error>> {
    let mut u_level_map = ULevelBookData::default();
    let num_u_steps = max_level as usize * 10;
    let mut calculation_cache: HashMap<VLevelRecipe, f64> = HashMap::new();

    let mut l_recipe = LLevelRecipe::default();
    let mut phase = CalibrationPhase::SimBas;
    let mut bas_mod_sequence = Vec::new();
    let mut mod_adv_sequence = Vec::new();
    let mut sequence_idx = 0;
    let mut sim_turn = true;

    let mut last_good_v_recipe = VLevelRecipe::default();
    let mut last_good_l_recipe = LLevelRecipe::default();
    let mut last_good_avd = 0.0;
    
    let simple_table = l_data.simple_tier.as_ref().ok_or("Missing Simple tier data")?;
    let basic_table = l_data.basic_tier.as_ref().ok_or("Missing Basic tier data")?;
    let moderate_table = l_data.moderate_tier.as_ref().ok_or("Missing Moderate tier data")?;
    let advanced_table = l_data.advanced_tier.as_ref().ok_or("Missing Advanced tier data")?;

    for i in 0..=num_u_steps {
        let current_u_level = i as f32 / 10.0;
        let target_avd = get_avd_from_user_level(current_u_level);

        loop {
            if phase == CalibrationPhase::SimBas && l_recipe.sim >= simple_table.natural_exhaustion_level {
                phase = CalibrationPhase::BasMod; sequence_idx = 0;
                bas_mod_sequence = generate_catch_up_sequence(l_recipe.bas, basic_table.natural_exhaustion_level, moderate_table.natural_exhaustion_level);
                println!("  -> Phase Transition: Simple tier exhausted. Now calibrating Basic + Moderate.");
            }
            if phase == CalibrationPhase::BasMod && l_recipe.bas >= basic_table.natural_exhaustion_level {
                phase = CalibrationPhase::ModAdv; sequence_idx = 0;
                mod_adv_sequence = generate_catch_up_sequence(l_recipe.mod_v, moderate_table.natural_exhaustion_level, advanced_table.natural_exhaustion_level);
                println!("  -> Phase Transition: Basic tier exhausted. Now calibrating Moderate + Advanced.");
            }
            if phase == CalibrationPhase::ModAdv && l_recipe.mod_v >= moderate_table.natural_exhaustion_level {
                phase = CalibrationPhase::AdvOnly;
                println!("  -> Phase Transition: Moderate tier exhausted. Now calibrating Advanced tier solo.");
            }
            if phase == CalibrationPhase::AdvOnly && l_recipe.adv >= advanced_table.natural_exhaustion_level {
                phase = CalibrationPhase::Complete;
                println!("  -> Phase Transition: Advanced tier exhausted. Calibration complete for this book.");
            }

            match phase {
                CalibrationPhase::SimBas => {
                    if sim_turn { l_recipe.sim = (l_recipe.sim * 10.0 + 1.0).round() / 10.0; } 
                    else { l_recipe.bas = l_recipe.sim; }
                    sim_turn = !sim_turn;
                }
                CalibrationPhase::BasMod => {
                    if let Some((bas, mod_v)) = bas_mod_sequence.get(sequence_idx) {
                        l_recipe.bas = *bas; l_recipe.mod_v = *mod_v; sequence_idx += 1;
                    } else { phase = CalibrationPhase::Complete; }
                }
                CalibrationPhase::ModAdv => {
                    if let Some((mod_v, adv)) = mod_adv_sequence.get(sequence_idx) {
                        l_recipe.mod_v = *mod_v; l_recipe.adv = *adv; sequence_idx += 1;
                    } else { phase = CalibrationPhase::Complete; }
                }
                CalibrationPhase::AdvOnly => { l_recipe.adv = (l_recipe.adv * 10.0 + 1.0).round() / 10.0; }
                CalibrationPhase::Complete => break,
            }

            let v_recipe = VLevelRecipe {
                sim: find_v_level_for_l_level(simple_table, l_recipe.sim),
                bas: find_v_level_for_l_level(basic_table, l_recipe.bas),
                mod_v: find_v_level_for_l_level(moderate_table, l_recipe.mod_v),
                adv: find_v_level_for_l_level(advanced_table, l_recipe.adv),
            };
            
            let actual_avd = get_avd_for_recipe(numerical_chapter, json_chapter, dictionary, v_recipe.clone(), &mut calculation_cache)?;

            if actual_avd > target_avd { break; } 
            else {
                last_good_v_recipe = v_recipe;
                last_good_l_recipe = l_recipe.clone();
                last_good_avd = actual_avd;
            }
        }

        u_level_map.u_level_map.push(ULevelEntry {
            u_level: current_u_level,
            target_avd,
            actual_avd: last_good_avd,
            recipe: last_good_v_recipe.clone(),
            l_level_recipe: last_good_l_recipe.clone(),
        });

        if i > 0 && i % 10 == 0 {
             println!("  -> Mapped U-Level {}.0: Target AVD = {:.2}, Actual AVD = {:.2}", current_u_level, target_avd, last_good_avd);
        }

        if phase == CalibrationPhase::Complete {
             println!("  -> All tiers exhausted. Filling remaining U-Levels with final recipe.");
            for j in (i + 1)..=num_u_steps {
                let future_u_level = j as f32 / 10.0;
                let future_target_avd = get_avd_from_user_level(future_u_level);
                 u_level_map.u_level_map.push(ULevelEntry {
                    u_level: future_u_level,
                    target_avd: future_target_avd,
                    actual_avd: last_good_avd,
                    recipe: last_good_v_recipe.clone(),
                    l_level_recipe: last_good_l_recipe.clone(),
                });
            }
            break;
        }
    }
    Ok(u_level_map)
}

fn generate_and_measure(
    numerical_chapter: &NumericalChapter,
    json_chapter: &JsonChapter,
    dictionary: &GlobalLemmaDictionary,
    active_tier: TierId,
    v_level: u32,
) -> Result<f64, Box<dyn Error>> {
    let v_levels = match active_tier {
        TierId::Simple => VLevelRecipe { sim: v_level, ..Default::default() },
        TierId::Basic => VLevelRecipe { bas: v_level, ..Default::default() },
        TierId::Moderate => VLevelRecipe { mod_v: v_level, ..Default::default() },
        TierId::Advanced => VLevelRecipe { adv: v_level, ..Default::default() },
    };
    let result = corpus_generator::generate_book_instance(
        numerical_chapter, json_chapter, dictionary,
        v_levels.sim, v_levels.bas, v_levels.mod_v, v_levels.adv,
        0.4, false,
    )?;
    let metrics = TextMetrics::new(&result.all_output_lemma_instances, result.total_base_words);
    Ok(metrics.calculate_avd_score())
}

fn get_avd_for_recipe(
    numerical_chapter: &NumericalChapter,
    json_chapter: &JsonChapter,
    dictionary: &GlobalLemmaDictionary,
    recipe: VLevelRecipe,
    cache: &mut HashMap<VLevelRecipe, f64>,
) -> Result<f64, Box<dyn Error>> {
    if let Some(cached_avd) = cache.get(&recipe) { return Ok(*cached_avd); }
    let result = corpus_generator::generate_book_instance(
        numerical_chapter, json_chapter, dictionary, 
        recipe.sim, recipe.bas, recipe.mod_v, recipe.adv, 0.4, false
    )?;
    let metrics = TextMetrics::new(&result.all_output_lemma_instances, result.total_base_words);
    let actual_avd = metrics.calculate_avd_score();
    cache.insert(recipe, actual_avd);
    Ok(actual_avd)
}

fn generate_catch_up_sequence(slow_start: f32, slow_end: f32, fast_end: f32) -> Vec<(f32, f32)> {
    let slow_steps = ((slow_end - slow_start) * 10.0).round() as u32;
    let fast_steps = (fast_end * 10.0).round() as u32;
    if slow_steps == 0 || fast_steps == 0 { return vec![(slow_end, fast_end)]; }

    let rate = fast_steps as f32 / slow_steps as f32;
    let mut sequence = Vec::new();
    for i in 0..=slow_steps {
        let current_slow = slow_start + (i as f32 / 10.0);
        let mut current_fast = (i as f32 * rate).round() / 10.0;
        current_fast = current_fast.min(fast_end);
        sequence.push((current_slow, current_fast));
    }
    sequence
}

fn find_v_level_for_l_level(table: &LLevelTable, l_level: f32) -> u32 {
    if l_level >= table.natural_exhaustion_level { return u32::MAX; }
    if l_level <= 0.0 { return 0; }
    table.levels.iter()
        .find(|&entry| entry.l_level >= l_level)
        .map_or(u32::MAX, |l| l.v_low)
}