// src/simulation/tests/test_frontier.rs
//
// Phase 8 validation tests for the Context Diffusion Frontier Filter.
// Tests: repeatability, sentence consistency, boundary independence.

use crate::corpus_generator::{
    generate_book_instance_with_frontier, FrontierSliceConfig,
};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;
use crate::simulation::preprocessor;
use crate::types::json_types::{
    JsonBookMetaV2, JsonChapter, JsonContentBlock, JsonSegmentV2, JsonSentenceBlock, JsonTierV2,
};
use once_cell::sync::Lazy;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Shared test fixture setup (frequency list)
// ---------------------------------------------------------------------------

static FRONTIER_TEST_SETUP: Lazy<Mutex<()>> = Lazy::new(|| {
    let guard = Mutex::new(());
    let test_assets_dir = PathBuf::from("target/test_assets");
    fs::create_dir_all(&test_assets_dir).expect("Failed to create test assets dir");
    let test_freq_list_path = test_assets_dir.join("test_frequency_list.txt");
    let mut file =
        File::create(&test_freq_list_path).expect("Failed to create test frequency list");
    writeln!(file, "lemma\trank\toccurrences").expect("Failed to write header");
    for i in 1..=20000 {
        writeln!(file, "lem{}\t{}\t100", i, i).expect("Failed to write to test frequency list");
    }
    frequency_manager::load_master_frequency_list(&test_freq_list_path)
        .expect("Failed to load test frequency list");
    guard
});

/// Acquire the setup mutex, tolerating poison from a previous panicking test.
fn setup() {
    let _guard = FRONTIER_TEST_SETUP
        .lock()
        .unwrap_or_else(|e| e.into_inner());
}

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

/// Build a JsonTierV2 with one segment (used internally by make_single_lemma_chapter).
#[allow(dead_code)]
fn make_tier(tier_id: &str, tokens: &[&str]) -> JsonTierV2 {
    use crate::types::json_types::JsonTokenV2;
    let seg_text: String = tokens.join(" ");
    let tokenized: Vec<JsonTokenV2> = tokens
        .iter()
        .map(|t| JsonTokenV2 {
            value: t.to_string(),
            ..Default::default()
        })
        .collect();
    JsonTierV2 {
        tier_id: tier_id.to_string(),
        full_text: seg_text.clone(),
        lemmas: tokens.iter().map(|s| s.to_string()).collect(),
        segments: vec![JsonSegmentV2 {
            seg_id: "S1".to_string(),
            text: seg_text,
            tokenized_text: tokenized,
            lemmas: tokens.iter().map(|s| s.to_string()).collect(),
        }],
        ..Default::default()
    }
}

/// Build a minimal `JsonChapter` with `n_sentences` sentences.
/// Each sentence has ONE unique lemma so the frontier can independently promote
/// each sentence (frontier-pass â†’ BasicTarget, frontier-fail â†’ BasicBaseDiglot).
/// The basic_target and basic_base tiers use DISTINCT text so that the chosen
/// tier is visible in the output.
fn make_single_lemma_chapter(n_sentences: usize, lemma_offset: usize) -> JsonChapter {
    use crate::types::json_types::JsonTokenV2;

    let mut content_blocks: Vec<JsonContentBlock> = Vec::new();
    for i in 0..n_sentences {
        let lemma = format!("lem{}", lemma_offset + i + 1);

        let make_tier_local = |tier_id: &str, text: &str| -> JsonTierV2 {
            let tokenized = vec![JsonTokenV2 {
                value: text.to_string(),
                ..Default::default()
            }];
            JsonTierV2 {
                tier_id: tier_id.to_string(),
                full_text: text.to_string(),
                lemmas: vec![lemma.clone()],
                segments: vec![JsonSegmentV2 {
                    seg_id: "S1".to_string(),
                    text: text.to_string(),
                    tokenized_text: tokenized,
                    lemmas: vec![lemma.clone()],
                }],
                ..Default::default()
            }
        };

        let mut sentence = JsonSentenceBlock::default();
        sentence.s_id = format!("s{}", i + 1);
        sentence.tiers = vec![
            make_tier_local("advanced_target", &format!("tgt-adv-{}", i + 1)),
            make_tier_local("moderate_target", &format!("tgt-mod-{}", i + 1)),
            make_tier_local("basic_target",    &format!("tgt-{}", i + 1)),
            make_tier_local("basic_base",      &format!("base-{}", i + 1)),
            make_tier_local("base",            &format!("base-{}", i + 1)),
        ];
        content_blocks.push(JsonContentBlock::Sentence(sentence));
    }
    JsonChapter {
        book_meta: JsonBookMetaV2 {
            book_name: "FrontierTestBook".to_string(),
            ..Default::default()
        },
        content_blocks,
        ..Default::default()
    }
}

/// Run frontier generation on a chapter and return the final text parts.
fn run_with_frontier(
    json_chapter: &JsonChapter,
    slice: &FrontierSliceConfig,
) -> Vec<String> {
    let mut dict = GlobalLemmaDictionary::new();
    let (numerical_chapter, _) = preprocessor::json_chapter_to_numerical(json_chapter, &mut dict);
    let result = generate_book_instance_with_frontier(
        &numerical_chapter,
        json_chapter,
        &dict,
        0,   // bas_v  â€” no pre-known lemmas; all lemmas are unknown at level 0
        0,   // mod_v
        0,   // adv_v
        0.5, // inverse_diglot_threshold
        false,
        Some(slice),
    )
    .expect("generate_book_instance_with_frontier failed in test");
    result.final_text_parts
}

// ---------------------------------------------------------------------------
// Test 1 â€” Repeatability: same seed â†’ same output
// ---------------------------------------------------------------------------

#[test]
fn frontier_repeatability_same_seed_same_output() {
    setup();

    // 20 single-lemma sentences, 50% frontier rate â†’ ~10 promoted; two runs must match
    let chapter = make_single_lemma_chapter(20, 0);
    let slice = FrontierSliceConfig {
        target_pct: 50.0,
        expected_unknown_pct: 100.0,
        total_tokens: 20,
        seed: 42,
    };

    let run_a = run_with_frontier(&chapter, &slice);
    let run_b = run_with_frontier(&chapter, &slice);

    assert_eq!(run_a, run_b,
        "Same seed must produce identical output (repeatability invariant broken)");
    println!("[frontier test 1] Repeatability: PASS â€” {} sentences, runs match", run_a.len());
}

// ---------------------------------------------------------------------------
// Test 2 â€” Different seeds produce distinct outputs
// ---------------------------------------------------------------------------

#[test]
fn frontier_different_seeds_different_output() {
    setup();

    // 40 single-lemma sentences, 50% frontier rate.
    // Two different seeds will shuffle the deck differently â†’ different set of promoted sentences.
    let chapter = make_single_lemma_chapter(40, 100);
    let base_slice = FrontierSliceConfig {
        target_pct: 50.0,
        expected_unknown_pct: 100.0,
        total_tokens: 40,
        seed: 100,
    };
    let alt_slice = FrontierSliceConfig { seed: 999, ..base_slice.clone() };

    let run_a = run_with_frontier(&chapter, &base_slice);
    let run_b = run_with_frontier(&chapter, &alt_slice);

    // At 50% rate over 40 sentences, the probability that both seeds promote
    // exactly the same set is astronomically small.
    assert_ne!(run_a, run_b,
        "Different seeds should produce different frontier decisions over 40 sentences");
    println!("[frontier test 2] Different seeds produce different output: PASS");
}

// ---------------------------------------------------------------------------
// Test 3 â€” Frontier disabled path is deterministic without seed
// ---------------------------------------------------------------------------

#[test]
fn frontier_off_is_deterministic_without_seed() {
    setup();

    let chapter = make_single_lemma_chapter(10, 200);
    let mut dict = GlobalLemmaDictionary::new();
    let (numerical_chapter, _) = preprocessor::json_chapter_to_numerical(&chapter, &mut dict);

    let run_a = generate_book_instance_with_frontier(
        &numerical_chapter, &chapter, &dict, 0, 0, 0, 0.5, false, None,
    ).expect("run_a failed").final_text_parts;

    let run_b = generate_book_instance_with_frontier(
        &numerical_chapter, &chapter, &dict, 0, 0, 0, 0.5, false, None,
    ).expect("run_b failed").final_text_parts;

    assert_eq!(run_a, run_b,
        "Frontier-off runs must be identical (no stochastic element)");
    println!("[frontier test 3] Frontier-off determinism: PASS");
}

// ---------------------------------------------------------------------------
// Test 4 â€” Diagnostics: emitted_frontier_tokens â‰¤ target_frontier_tokens + tolerance
// ---------------------------------------------------------------------------

#[test]
fn frontier_diagnostics_emitted_within_tolerance() {
    setup();

    let chapter = make_single_lemma_chapter(50, 300);
    let mut dict = GlobalLemmaDictionary::new();
    let (numerical_chapter, _) = preprocessor::json_chapter_to_numerical(&chapter, &mut dict);

    let target_pct = 30.0_f32;
    let total_tokens = 50_usize;
    let slice = FrontierSliceConfig {
        target_pct,
        expected_unknown_pct: 100.0,
        total_tokens,
        seed: 12345,
    };

    let result = generate_book_instance_with_frontier(
        &numerical_chapter, &chapter, &dict, 0, 0, 0, 0.5, false, Some(&slice),
    ).expect("generation failed");

    let diag = result.frontier_diagnostics.expect("FrontierDiagnostics should be present");
    let realized_pct = if diag.total_tokens > 0 {
        diag.emitted_frontier_tokens as f32 / diag.total_tokens as f32 * 100.0
    } else {
        0.0
    };
    println!(
        "[frontier test 4] Diagnostics: target={} emitted={} tokens (target_pct={:.1}% realized={:.1}%) deck={} pass={} steered={}",
        diag.target_frontier_tokens,
        diag.emitted_frontier_tokens,
        target_pct,
        realized_pct,
        diag.deck_size,
        diag.pass_count,
        diag.steering_adjustment_count,
    );

    // Emitted should be within Â±60% of target (generous tolerance for a small corpus).
    let target = diag.target_frontier_tokens as i64;
    let emitted = diag.emitted_frontier_tokens as i64;
    let tolerance = ((target as f64 * 0.6) as i64).max(5);
    assert!(
        (emitted - target).abs() <= tolerance,
        "Emitted frontier tokens ({}) deviates too far from target ({}) â€” budget steering may be broken",
        emitted,
        target
    );
    println!("[frontier test 4] Budget within tolerance: PASS");
}

// ---------------------------------------------------------------------------
// Test 5 â€” Boundary independence: re-running boundary A is reproducible
//           regardless of boundary B being run first.
// ---------------------------------------------------------------------------

#[test]
fn frontier_boundary_independence() {
    setup();

    // Two independent boundary slices with distinct seeds and lemma ranges
    let chapter_a = make_single_lemma_chapter(15, 500);
    let chapter_b = make_single_lemma_chapter(15, 600);

    let slice_a = FrontierSliceConfig {
        target_pct: 50.0,
        expected_unknown_pct: 100.0,
        total_tokens: 15,
        seed: 1,
    };
    let slice_b = FrontierSliceConfig { seed: 2, total_tokens: 15, ..slice_a.clone() };

    // Run boundary B first, then re-run A â€” result must match A-alone run
    let _out_b_first = run_with_frontier(&chapter_b, &slice_b);
    let out_a_after_b = run_with_frontier(&chapter_a, &slice_a);
    let out_a_alone  = run_with_frontier(&chapter_a, &slice_a);

    assert_eq!(out_a_after_b, out_a_alone,
        "Boundary A with the same seed must produce identical output regardless of whether boundary B ran first");
    println!("[frontier test 5] Boundary independence (A reproducible after B): PASS");

    // Sanity: A and B themselves should differ at 50% rate over 15 sentences
    let out_b = run_with_frontier(&chapter_b, &slice_b);
    // They use distinct lemma pools AND distinct seeds â€” outputs will differ
    assert_ne!(out_a_alone, out_b,
        "Boundaries with different seeds and lemma pools should produce different outputs");
    println!("[frontier test 5] Boundary isolation (A â‰  B): PASS");
}

