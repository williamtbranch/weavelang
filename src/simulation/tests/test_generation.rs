// src/simulation/tests/test_generation.rs

use super::weavetest_parser::{
    self, DslAssertion, DslSubTest, DslTestCase, EXHAUSTED_LEVEL,
};
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
    frequency_manager::load_master_frequency_list(&test_freq_list_path)
        .expect("Failed to load test frequency list");
    guard
});

#[test]
fn run_dsl_generation_test_suite() {
    let _guard = TEST_SETUP.lock().unwrap();
    let dsl_content: &'static str = include_str!("generation_tests.weavetest");
    
    // --- HELPER FUNCTIONS MOVED INSIDE THE TEST ---

    fn compile_dsl_sentence_to_numerical(
        test_case: &DslTestCase,
        dictionary: &mut GlobalLemmaDictionary,
    ) -> (NumericalProcessedSentence, JsonSentenceBlock) {
        let mut json_sentence = JsonSentenceBlock::default();
        json_sentence.s_id = test_case.name.clone();

        let mut adv_target_tier = JsonTierV2 { tier_id: "advanced_target".to_string(), ..Default::default() };
        let mut mod_target_tier = JsonTierV2 { tier_id: "moderate_target".to_string(), ..Default::default() };
        let mut basic_spanish_tier = JsonTierV2 { tier_id: "basic_target".to_string(), ..Default::default() };
        let mut basic_english_tier = JsonTierV2 { tier_id: "basic_base".to_string(), ..Default::default() };
        let mut literary_base_tier = JsonTierV2 { tier_id: "base".to_string(), ..Default::default() };

        let dsl_seg_to_json_seg = |seg: &super::weavetest_parser::DslSegment, seg_id: String| -> JsonSegmentV2 {
            JsonSegmentV2 {
                seg_id,
                text: seg.tokens.iter().map(|t| t.value.as_str()).collect(),
                tokenized_text: seg.tokens.clone(),
                lemmas: seg.lemmas.clone(),
            }
        };

        for (i, seg) in test_case.sentence_def.l0_adv_segments.iter().enumerate() {
            adv_target_tier.segments.push(dsl_seg_to_json_seg(seg, format!("A{}", i + 1)));
        }
        for (i, seg) in test_case.sentence_def.l0_mod_segments.iter().enumerate() {
            mod_target_tier.segments.push(dsl_seg_to_json_seg(seg, format!("A{}", i + 1)));
        }
        
        let bs_def = &test_case.sentence_def.l1_basic_spanish;
        basic_spanish_tier.segments.push(dsl_seg_to_json_seg(bs_def, "S1".to_string()));
        basic_spanish_tier.lemmas = bs_def.lemmas.clone();
        
        let be_def = &test_case.sentence_def.l1_basic_english;
        basic_english_tier.segments.push(dsl_seg_to_json_seg(be_def, "S1".to_string()));

        let tiers = vec![&mut adv_target_tier, &mut mod_target_tier, &mut basic_spanish_tier, &mut basic_english_tier];
        for tier in tiers {
            tier.full_text = tier.segments.iter().map(|s| s.text.clone()).collect::<String>();
        }

        json_sentence.mappings.basic_diglot.insert(
            "S1".to_string(),
            test_case.sentence_def.l1_diglot_tuples.iter().enumerate().map(|(i, t)| {
                let proper_noun_lemmas = if t.is_proper_noun { t.replacement_lemmas.clone() } else { Vec::new() };
                (i, t.replacement_lemmas.clone(), t.replacement_word.clone(), t.is_viable, t.word_to_replace.split_whitespace().count(), proper_noun_lemmas)
            }).collect(),
        );
        json_sentence.mappings.basic_inverse_diglot.insert(
            "S1".to_string(),
            test_case.sentence_def.l1_inv_diglot_tuples.iter().enumerate()
                .map(|(idx, t)| {
                    // Calculate the spanish word count from the DSL tuple's target_word
                    let spa_wc = t.target_word.split_whitespace().count();
                    (idx, t.target_lemmas.clone(), t.base_substitute.clone(), t.base_substitute.split_whitespace().count(), spa_wc) // <--- CORRECTED (now produces a 5-tuple)
                })
                .collect()
        );

        json_sentence.tiers = vec![literary_base_tier, adv_target_tier, mod_target_tier, basic_spanish_tier, basic_english_tier];
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

    fn run_assertions(sub_test: &DslSubTest, output: &ChosenLevelOutput, actual_text: &str) -> (bool, Vec<String>) {
        let mut is_passed = true;
        let mut reasons = Vec::new();
        for assertion in &sub_test.assertions {
            match assertion {
                DslAssertion::Level(expected_level_str) => {
                    let expected_level_str_from_enum = match output.level {
                        OutputLevel::AdvancedWeave => "AdvancedWeave",
                        OutputLevel::BasicTarget => "BasicTarget",
                        OutputLevel::InverseDiglot => "InverseDiglot",
                        OutputLevel::BasicBaseDiglot => "BasicBaseDiglot",
                    };
                    if expected_level_str != expected_level_str_from_enum {
                        is_passed = false;
                        reasons.push(format!("Level Mismatch: Expected {}, got {}", expected_level_str, expected_level_str_from_enum));
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

    // --- MAIN TEST LOGIC ---

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
            
            let levels = &sub_test.learner_level;
            let highest_level = *[levels.bas, levels.mod_level, levels.adv].iter().max().unwrap_or(&0);
            if highest_level < EXHAUSTED_LEVEL {
                for i in 1..=highest_level {
                    profile.activate_lemma(dictionary.get_id_or_insert(&format!("lem{}", i)));
                }
            }

            let v_levels = VLevelRecipe {
                sim: 0,
                bas: levels.bas, // <-- FIX
                mod_v: levels.mod_level,
                adv: levels.adv,
            };

            let mut n_sentence_clone = numerical_sentence.clone();
            let output = determine_and_annotate_sentence_expression(
                &mut n_sentence_clone,
                &profile,
                &dictionary,
                &v_levels,
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
        println!( "{} / {} sub-tests passed.", total_passed, total_passed + total_failed);
        println!("\nFailed sub-tests:");
        for detail in failed_details {
            println!("  - {}", detail);
        }
        panic!("One or more sub-tests failed.");
    }
    println!("\n--- TEST SUITE SUMMARY ---");
    println!("All {} sub-tests passed.", total_passed);
}