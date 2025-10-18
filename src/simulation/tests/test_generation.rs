// In src/simulation/tests/test_generation.rs

use super::weavetest_parser::{
    self, DslAssertion, DslSegmentSpecEnum, DslSubTest, DslTestCase, EXHAUSTED_LEVEL,
};
use crate::types::json_types::JsonTokenV2;
use crate::simulation::core_algo::{
    determine_and_annotate_sentence_expression, ChosenLevelOutput, OutputLevel,
};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;
use crate::simulation::numerical_types::{
    NumericalLearnerProfile, NumericalProcessedSentence, VLevelRecipe,
};
use crate::simulation::preprocessor;
use crate::simulation::text_generator;
use crate::types::json_types::{
    JsonBookMetaV2, JsonChapter, JsonContentBlock, JsonSegmentV2, JsonSentenceBlock, JsonTierV2,
    JsonTokenType,
};
use once_cell::sync::Lazy;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static TEST_SETUP: Lazy<Mutex<()>> = Lazy::new(|| {
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
    // As requested, the "real lemmas" are no longer hardcoded here.
    // Tests must use lemXXX definitions.
    frequency_manager::load_master_frequency_list(&test_freq_list_path)
        .expect("Failed to load test frequency list");
    guard
});

#[test]
fn run_dsl_generation_test_suite() {
    let _guard = TEST_SETUP.lock().unwrap();
    let dsl_content: &'static str = include_str!("generation_tests.weavetest");
    let test_cases = match weavetest_parser::parse_weavetest_file(dsl_content) {
        Ok(data) => data,
        Err(e) => panic!(
            "FATAL: Failed to parse `generation_tests.weavetest`.\nError: {}",
            e
        ),
    };

    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut failed_details = Vec::new();

    for test_case in &test_cases {
        println!("\n============================================================");
        println!("TEST CASE: [{}]", test_case.name);
        println!("------------------------------------------------------------");

        let mut dictionary = GlobalLemmaDictionary::new();
        let (numerical_sentence, json_sentence) =
            compile_dsl_sentence_to_numerical(test_case, &mut dictionary);

        for sub_test in &test_case.sub_tests {
            println!("\n  Sub-Test: [{}]", sub_test.name);
            let mut profile = NumericalLearnerProfile::new();
            
            // This profile is now ONLY for inverse diglot checks.
            // We can simplify its population for now.
            let levels = &sub_test.learner_level;
            let highest_level = *[levels.sim, levels.bas, levels.mod_level, levels.adv].iter().max().unwrap_or(&0);
            if highest_level < EXHAUSTED_LEVEL {
                for i in 1..=highest_level {
                    profile.activate_lemma(dictionary.get_id_or_insert(&format!("lem{}", i)));
                }
            }

            let v_levels = VLevelRecipe {
                sim: levels.sim,
                bas: levels.bas,
                mod_v: levels.mod_level,
                adv: levels.adv,
            };

            let mut n_sentence_clone = numerical_sentence.clone();
            // Call the function with the new, correct signature
            let output = determine_and_annotate_sentence_expression(
                &mut n_sentence_clone,
                &profile,
                &dictionary,
                &v_levels, // Pass the recipe struct
                0.5,
            );

            let raw_text = text_generator::generate_raw_text_from_levels(
                &[&json_sentence],
                &[output.clone()],
                false,
            )
            .unwrap();
            let actual_text = text_generator::clean_text_for_tts(&raw_text);
            let (is_passed, failure_reasons) = run_assertions(sub_test, &output, &actual_text);
            println!("    Expected: '{}'", get_expected_text(sub_test));
            println!("    Actual:   '{}'", actual_text);
            if is_passed {
                println!("    Result:   PASS");
                total_passed += 1;
            } else {
                println!("    Result:   FAIL\n    Reasons:");
                for reason in &failure_reasons {
                    println!("      - {}", reason);
                }
                total_failed += 1;
                failed_details.push(format!(
                    "Test Case: '{}', Sub-Test: '{}'",
                    test_case.name, sub_test.name
                ));
            }
        }
    }
    println!("\n============================================================");
    if total_failed > 0 {
        println!("\n--- TEST SUITE SUMMARY ---");
        println!(
            "{} / {} sub-tests passed.",
            total_passed,
            total_passed + total_failed
        );
        println!("\nFailed sub-tests:");
        for detail in failed_details {
            println!("  - {}", detail);
        }
        panic!("One or more sub-tests failed.");
    }
    println!("\n--- TEST SUITE SUMMARY ---");
    println!("All {} sub-tests passed.", total_passed);
}

fn compile_dsl_sentence_to_numerical(
    test_case: &DslTestCase,
    dictionary: &mut GlobalLemmaDictionary,
) -> (NumericalProcessedSentence, JsonSentenceBlock) {
    let mut json_sentence = JsonSentenceBlock::default();
    json_sentence.s_id = test_case.name.clone();

    let mut base_tier = JsonTierV2 { tier_id: "base".to_string(), ..Default::default() };
    let mut adv_target_tier = JsonTierV2 { tier_id: "advanced_target".to_string(), ..Default::default() };
    let mut mod_target_tier = JsonTierV2 { tier_id: "moderate_target".to_string(), ..Default::default() };
    let mut bas_target_tier = JsonTierV2 { tier_id: "basic_target".to_string(), ..Default::default() };
    let mut sim_target_tier = JsonTierV2 { tier_id: "simple_target".to_string(), ..Default::default() };

    // --- L0 Processing (Refactored for Multi-Segment) ---
    let l0_spanish_segments: Vec<_> = test_case.sentence_def.l0_def.segments.iter()
        .filter(|s| matches!(&s.spec, DslSegmentSpecEnum::Spanish { .. }))
        .collect();
    
    // Group segments into chunks of 4 (Adv, Mod, Bas, Sim)
    for (i, chunk) in l0_spanish_segments.chunks(4).enumerate() {
        let seg_id = format!("A{}", i + 1);

        if chunk.len() != 4 {
            panic!("DSL L0 Error: Spanish segments must be in groups of 4. Found a group with {} segments.", chunk.len());
        }

        // Helper closure to add a segment to a tier
        let add_segment_to_tier = |tier: &mut JsonTierV2, segment_spec: &DslSegmentSpecEnum| {
            if let DslSegmentSpecEnum::Spanish { tokens, lemmas } = segment_spec {
                tier.segments.push(JsonSegmentV2 {
                    seg_id: seg_id.clone(),
                    text: tokens.iter().map(|t| t.value.as_str()).collect(),
                    tokenized_text: tokens.clone(),
                    lemmas: lemmas.clone(),
                });
            }
        };

        add_segment_to_tier(&mut adv_target_tier, &chunk[0].spec);
        add_segment_to_tier(&mut mod_target_tier, &chunk[1].spec);
        add_segment_to_tier(&mut bas_target_tier, &chunk[2].spec);
        add_segment_to_tier(&mut sim_target_tier, &chunk[3].spec);
    }
    
    // --- L1 Processing (Unchanged) ---
    let l1_diglot_tuples: Vec<_> = test_case.sentence_def.l1_def.segments.iter()
        .filter_map(|s| if let DslSegmentSpecEnum::Diglot { tuples } = &s.spec { Some(tuples.clone()) } else { None })
        .flatten().collect();

    let propn_lookup: std::collections::HashSet<String> = l1_diglot_tuples.iter()
        .filter(|tuple| tuple.is_proper_noun)
        .map(|tuple| tuple.word_to_replace.clone())
        .collect();

    let l1_eng_tokens: Vec<JsonTokenV2> = test_case.sentence_def.l1_def.segments.iter()
        .filter_map(|s| if let DslSegmentSpecEnum::English { tokens } = &s.spec { Some(tokens.clone()) } else { None })
        .flatten()
        .map(|mut token| {
            if token.token_type == JsonTokenType::Word && propn_lookup.contains(&token.value) {
                token.is_pn = Some(true);
            }
            token
        })
        .collect();

    base_tier.segments.push(JsonSegmentV2 {
        seg_id: "S1".to_string(),
        text: l1_eng_tokens.iter().map(|t| t.value.as_str()).collect(),
        tokenized_text: l1_eng_tokens,
        ..Default::default()
    });

    json_sentence.mappings.simple_target_to_base_diglot.insert(
        "S1".to_string(),
        l1_diglot_tuples.iter().enumerate().map(|(i, t)| {
            // --- START OF FIX ---
            // The 6th element must now be a Vec<String>, not a bool.
            // We create an empty vec if it's not a proper noun.
            let proper_noun_lemmas = if t.is_proper_noun {
                // If it's a proper noun, use its own lemmas.
                t.replacement_lemmas.clone()
            } else {
                // Otherwise, it's an empty list.
                Vec::new()
            };

            (
                i, 
                t.replacement_lemmas.clone(), 
                t.replacement_word.clone(), 
                t.is_viable, 
                t.word_to_replace.split_whitespace().count(),
                proper_noun_lemmas // <-- Pass the new Vec<String>
            )
            // --- END OF FIX ---
        }).collect(),
    );

    // --- Inverse Diglot Processing (Refactored for Multi-Segment) ---
    let l0_inv_diglot_defs: Vec<_> = test_case.sentence_def.l0_def.segments.iter()
        .filter(|s| matches!(&s.spec, DslSegmentSpecEnum::InvDiglot { .. }))
        .collect();
    
    for (i, inv_diglot_def) in l0_inv_diglot_defs.iter().enumerate() {
        if let DslSegmentSpecEnum::InvDiglot { tuples } = &inv_diglot_def.spec {
            let seg_id = format!("A{}", i + 1);
            let simple_seg = sim_target_tier.segments.get(i).expect("Missing Simple segment for Inverse Diglot mapping");
            
            let word_tokens: Vec<_> = simple_seg.tokenized_text.iter()
                .filter(|t| t.token_type == JsonTokenType::Word)
                .collect();

            let entries = tuples.iter().zip(word_tokens.iter()).enumerate()
                .map(|(idx, (t, _))| {
                    let eng_word_count = t.base_substitute.split_whitespace().count();
                    (idx, t.target_lemmas.clone(), t.base_substitute.clone(), eng_word_count)
                }).collect();
            
            json_sentence.mappings.adv_target_to_base_inv_diglot.insert(seg_id, entries);
        }
    }

    // --- Final Assembly (Unchanged) ---
    let reconstruct_and_set_full_text = |tier: &mut JsonTierV2| {
        tier.full_text = tier.segments.iter().map(|s| s.text.clone()).collect::<String>();
    };
    reconstruct_and_set_full_text(&mut base_tier);
    reconstruct_and_set_full_text(&mut adv_target_tier);
    reconstruct_and_set_full_text(&mut mod_target_tier);
    reconstruct_and_set_full_text(&mut bas_target_tier);
    reconstruct_and_set_full_text(&mut sim_target_tier);

    json_sentence.tiers = vec![base_tier, adv_target_tier, mod_target_tier, bas_target_tier, sim_target_tier];
    let mock_chapter = JsonChapter {
        book_meta: JsonBookMetaV2 { book_name: test_case.name.clone(), ..Default::default() },
        content_blocks: vec![JsonContentBlock::Sentence(json_sentence.clone())],
        ..Default::default()
    };

    let (numerical_chapter, _) = preprocessor::json_chapter_to_numerical(&mock_chapter, dictionary);
    (numerical_chapter.sentences_numerical.into_iter().next().unwrap(), json_sentence)
}

fn get_expected_text<'s>(sub_test: &'s DslSubTest) -> &'s str {
    sub_test.assertions.iter()
        .find_map(|a| if let DslAssertion::Text(txt) = a { Some(txt.as_str()) } else { None })
        .unwrap_or("")
}

fn run_assertions(
    sub_test: &DslSubTest,
    output: &ChosenLevelOutput,
    actual_text: &str,
) -> (bool, Vec<String>) {
    let mut is_passed = true;
    let mut reasons = Vec::new();
    for assertion in &sub_test.assertions {
        match assertion {
            DslAssertion::Level(expected_level_str) => {
                let expected_level = if expected_level_str == "AdvancedWeave" { OutputLevel::AdvancedWeave } else { OutputLevel::SimpleHybrid };
                if output.level != expected_level {
                    is_passed = false;
                    reasons.push(format!("Level Mismatch: Expected {:?}, got {:?}", expected_level, output.level));
                }
            }
            DslAssertion::Text(expected_text) => {
                if actual_text != *expected_text {
                    is_passed = false;
                    reasons.push(format!("Text Mismatch: Expected '{}', got '{}'", expected_text, actual_text));
                }
            }
        }
    }
    (is_passed, reasons)
}