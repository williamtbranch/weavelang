use super::{
    core_algo,
    dictionary::GlobalLemmaDictionary,
    frequency_manager,
    metrics::TextMetrics,
    numerical_types::{NumericalChapter, NumericalLearnerProfile, VLevelRecipe},
    preprocessor,
};
use crate::parsing::json_parser;
use std::{collections::HashSet, error::Error, fs, path::Path};

const NEW_LEMMA_DENSITY_TARGET: f64 = 0.02;

/// A private helper function that runs a full in-memory generation and measurement pass.
fn generate_and_measure_at_locked_level(
    numerical_chapter: &NumericalChapter,
    dictionary: &GlobalLemmaDictionary,
    locked_v_level: u32,
) -> (Vec<String>, usize) {
    let mut all_output_lemma_instances: Vec<String> = Vec::new();
    let mut total_english_words = 0;
    let empty_profile = NumericalLearnerProfile::new();

    let v_levels = VLevelRecipe {
        bas: locked_v_level,
        mod_v: locked_v_level,
        adv: locked_v_level,
    };

    for n_sentence in &numerical_chapter.sentences_numerical {
        let mut n_sentence_clone = n_sentence.clone();
        let output = core_algo::determine_and_annotate_sentence_expression(
            &mut n_sentence_clone,
            &empty_profile,
            dictionary,
            &v_levels,
            0.4,
        );

        total_english_words += output.english_word_count;
        for &lemma_id in &output.lemma_ids {
            if let Some(lemma_str) = dictionary.get_str(lemma_id) {
                all_output_lemma_instances.push(lemma_str.clone());
            }
        }
    }
    (all_output_lemma_instances, total_english_words)
}

/// The main function for the AVD Hunter. Discovers the master AVD scale.
pub fn run_hunt(
    canonical_json_path: &Path,
    max_user_levels: u32,
    output_csv_path: &Path,
) -> Result<(), Box<dyn Error>> {
    println!("[INFO] Starting AVD Hunter process...");
    println!("  -> Canonical JSON: {}", canonical_json_path.display());

    let json_content = fs::read_to_string(canonical_json_path)?;
    let json_chapter = json_parser::parse_chapter_from_json(&json_content)?;
    let mut dictionary = GlobalLemmaDictionary::new();
    dictionary.populate_from_json_chapter(&json_chapter);
    let (numerical_chapter, _) =
        preprocessor::json_chapter_to_numerical(&json_chapter, &mut dictionary);
    println!(
        "  -> Preprocessed {} sentences from canonical text.",
        numerical_chapter.sentences_numerical.len()
    );

    let mut master_scale: Vec<(u32, u32, f64)> = Vec::new();
    let mut previous_v_level: u32 = 0;

    for user_level in 1..=max_user_levels {
        println!("\n--- Hunting for User Level {user_level} ---");

        let mut low_bound = previous_v_level;
        let mut high_bound = low_bound + (low_bound / 2).max(2000);

        println!("  -> Finding search window...");
        loop {
            let (lemmas, eng_words) =
                generate_and_measure_at_locked_level(&numerical_chapter, &dictionary, high_bound);
            let metrics = TextMetrics::new(&lemmas, eng_words);
            let new_density = metrics.calculate_new_lemma_density(previous_v_level);

            if new_density > NEW_LEMMA_DENSITY_TARGET || high_bound > 3_000_000 {
                println!("     Window found: [{low_bound}, {high_bound}]");
                break;
            } else {
                low_bound = high_bound;
                high_bound = (high_bound as f64 * 1.5).ceil() as u32;
                println!("     Window too low, expanding to: [{low_bound}, {high_bound}]");
            }
        }

        println!(
            "  -> Performing binary search for {:.2}% density...",
            NEW_LEMMA_DENSITY_TARGET * 100.0
        );
        while high_bound > low_bound + 1 {
            let trial_v = low_bound + (high_bound - low_bound) / 2;
            let (lemmas, eng_words) =
                generate_and_measure_at_locked_level(&numerical_chapter, &dictionary, trial_v);
            let metrics = TextMetrics::new(&lemmas, eng_words);
            let new_density = metrics.calculate_new_lemma_density(previous_v_level);

            if new_density < NEW_LEMMA_DENSITY_TARGET {
                low_bound = trial_v;
            } else {
                high_bound = trial_v;
            }
            print!(".");
            std::io::Write::flush(&mut std::io::stdout())?;
        }
        println!(" Done.");

        let v_n_plus_1 = high_bound;

        let (lemmas, eng_words) =
            generate_and_measure_at_locked_level(&numerical_chapter, &dictionary, v_n_plus_1);
        let final_metrics = TextMetrics::new(&lemmas, eng_words);
        let avd_score = final_metrics.calculate_avd_score();

        let total_words = final_metrics.total_word_count;
        let new_words_tally = (final_metrics.calculate_new_lemma_density(previous_v_level)
            * total_words as f64)
            .round() as u64;

        println!(
            "  -> Discovered User Level {user_level}: V-Level = {v_n_plus_1}, AVD Score = {avd_score:.2}"
        );
        println!(
            "     Verification: {:.4}% new words ({}/{} total words)",
            final_metrics.calculate_new_lemma_density(previous_v_level) * 100.0,
            new_words_tally,
            total_words
        );

        let mut new_lemmas_found: HashSet<String> = HashSet::new();
        for lemma_str in &lemmas {
            if let Some(rank) = frequency_manager::get_rank_for_lemma(lemma_str) {
                if rank > previous_v_level {
                    new_lemmas_found.insert(format!("'{lemma_str}' (Rank: {rank})"));
                }
            }
        }

        if !new_lemmas_found.is_empty() {
            println!("     New lemmas contributing to density:");
            let mut sorted_new_lemmas: Vec<String> = new_lemmas_found.into_iter().collect();
            sorted_new_lemmas.sort();
            for (i, lemma_info) in sorted_new_lemmas.iter().enumerate() {
                print!(
                    "       {}{}",
                    lemma_info,
                    if (i + 1) % 4 == 0 { "\n" } else { " | " }
                );
            }
            if sorted_new_lemmas.len() % 4 != 0 {
                println!();
            }
        } else {
            println!("     No new lemmas found in this step.");
        }

        master_scale.push((user_level, v_n_plus_1, avd_score));
        previous_v_level = v_n_plus_1;

        if previous_v_level > 3_400_000 {
            println!("[WARN] V-Level has exceeded frequency list bounds. Ending hunt early.");
            break;
        }
    }

    // --- START OF BACK-FILLING FIX ---
    println!("\n[INFO] Post-processing: Back-filling early user levels...");
    for (user_level, v_level, _) in master_scale.iter_mut() {
        if *v_level < *user_level {
            println!(
                "  -> Adjusting UL {}: V-Level was {}, now set to {}.",
                *user_level, *v_level, *user_level
            );
            *v_level = *user_level;
        } else {
            // Once the natural curve takes over, we can stop.
            println!(
                "  -> Natural V-Level ({}) has surpassed User Level ({}), stopping back-fill.",
                *v_level, *user_level
            );
            break;
        }
    }
    // --- END OF BACK-FILLING FIX ---

    println!(
        "\n[INFO] AVD Hunt complete. Writing results to {}",
        output_csv_path.display()
    );
    let mut writer = csv::Writer::from_path(output_csv_path)?;
    writer.write_record(["user_level", "v_level_boundary", "avd_score"])?;
    for (ul, vl, avd) in master_scale {
        writer.write_record(&[ul.to_string(), vl.to_string(), format!("{avd:.4}")])?;
    }
    writer.flush()?;

    println!("[SUCCESS] Master AVD Scale saved successfully.");
    Ok(())
}
