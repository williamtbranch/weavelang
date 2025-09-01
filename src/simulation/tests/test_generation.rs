// In src/simulation/tests/test_generation.rs

use super::weavetest_parser::{
    self, DslAssertion, DslSegmentSpecEnum, DslSubTest, DslTestCase,
};
// --- FIX #1: Add the missing import for JsonTokenV2 ---
use crate::types::json_types::JsonTokenV2;
// --- END FIX #1 ---
use crate::simulation::core_algo::{
    determine_and_annotate_sentence_expression, ChosenLevelOutput, OutputLevel,
};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;
use crate::simulation::numerical_types::{
    NumericalLearnerProfile, NumericalProcessedSentence,
};
use crate::simulation::preprocessor;
use crate::simulation::text_generator;
use crate::types::json_types::{
    JsonBookMetaV2, JsonChapter, JsonContentBlock, JsonSegmentV2, JsonSentenceBlock, JsonTierV2,
    JsonTokenType,
};
use itertools::Itertools;
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
    let real_lemmas = vec![
        "quien", "ser", "tu", "decir", "el", "de", "gusano", "sedar", "oruga", "quién", "tú",
        "claro", "exclamar", "rey", "si", "que", "estar", "muerto", "no", "haber", "duda",
        "frase", "avanzar", "prueba", "uno", "simple", "otro", "segmento", "segundo",
        "hombre", "ir", "bueno", "tarde", "el", "ella", "dar", "paseo",
    ];
    let mut rank = 20001;
    for lemma in real_lemmas {
        writeln!(file, "{}\t{}\t100", lemma, rank).expect("Failed to write real lemma");
        rank += 1;
    }
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
            for i in 1..=sub_test.learner_level {
                profile.activate_lemma(dictionary.get_id_or_insert(&format!("lem{}", i)));
            }
            let real_lemmas_to_activate = vec![
                "dar", "uno", "paseo", "seg", "dos", "quién", "tú", "dijo", "el", "sí", "qué",
                "es", "ese",
            ];
            for lemma in real_lemmas_to_activate {
                if sub_test.learner_level >= dictionary.get_id(lemma).unwrap_or(u32::MAX) {
                    profile.activate_lemma(dictionary.get_id_or_insert(lemma));
                }
            }
            let mut n_sentence_clone = numerical_sentence.clone();
            let output = determine_and_annotate_sentence_expression(
                &mut n_sentence_clone,
                &profile,
                &dictionary,
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

    let mut base_tier = JsonTierV2 {
        tier_id: "base".to_string(),
        ..Default::default()
    };
    let mut adv_target_tier = JsonTierV2 {
        tier_id: "advanced_target".to_string(),
        ..Default::default()
    };
    let mut simpler_adv_target_tier = JsonTierV2 {
        tier_id: "simpler_advanced_target".to_string(),
        ..Default::default()
    };

    let l0_spanish_segments: Vec<_> = test_case
        .sentence_def
        .l0_def
        .segments
        .iter()
        .filter(|s| matches!(&s.spec, DslSegmentSpecEnum::Spanish { .. }))
        .collect();

    adv_target_tier.segments = l0_spanish_segments
        .iter()
        .step_by(2)
        .enumerate()
        .map(|(i, s)| {
            if let DslSegmentSpecEnum::Spanish { tokens, lemmas } = &s.spec {
                JsonSegmentV2 {
                    seg_id: format!("A{}", i + 1),
                    text: tokens.iter().map(|t| t.value.as_str()).collect(),
                    tokenized_text: tokens.clone(),
                    lemmas: lemmas.clone(),
                }
            } else {
                unreachable!()
            }
        })
        .collect();

    simpler_adv_target_tier.segments = l0_spanish_segments
        .iter()
        .skip(1)
        .step_by(2)
        .enumerate()
        .map(|(i, s)| {
            if let DslSegmentSpecEnum::Spanish { tokens, lemmas } = &s.spec {
                JsonSegmentV2 {
                    seg_id: format!("A{}", i + 1),
                    text: tokens.iter().map(|t| t.value.as_str()).collect(),
                    tokenized_text: tokens.clone(),
                    lemmas: lemmas.clone(),
                }
            } else {
                unreachable!()
            }
        })
        .collect();

    let l1_eng_tokens: Vec<JsonTokenV2> = test_case
        .sentence_def
        .l1_def
        .segments
        .iter()
        .filter_map(|s| {
            if let DslSegmentSpecEnum::English { tokens } = &s.spec {
                Some(tokens.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect();

    base_tier.segments.push(JsonSegmentV2 {
        seg_id: "S1".to_string(),
        text: l1_eng_tokens.iter().map(|t| t.value.as_str()).collect(),
        tokenized_text: l1_eng_tokens,
        ..Default::default()
    });

    let l1_diglot_tuples: Vec<_> = test_case
        .sentence_def
        .l1_def
        .segments
        .iter()
        .filter_map(|s| {
            if let DslSegmentSpecEnum::Diglot { tuples } = &s.spec {
                Some(tuples.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect();

    json_sentence.mappings.simple_target_to_base_diglot.insert(
        "S1".to_string(),
        l1_diglot_tuples
            .iter()
            .enumerate()
            .map(|(i, t)| {
                (
                    i,
                    t.replacement_lemmas.clone(),
                    t.replacement_word.clone(),
                    t.is_viable,
                    t.word_to_replace.split_whitespace().count(),
                )
            })
            .collect(),
    );

    let reconstruct_and_set_full_text = |tier: &mut JsonTierV2| {
        tier.full_text = tier.segments.iter().map(|s| s.text.clone()).collect::<String>();
    };
    reconstruct_and_set_full_text(&mut base_tier);
    reconstruct_and_set_full_text(&mut adv_target_tier);
    reconstruct_and_set_full_text(&mut simpler_adv_target_tier);

    json_sentence.tiers = vec![base_tier, adv_target_tier, simpler_adv_target_tier];

    let l0_chunks: Vec<_> = test_case.sentence_def.l0_def.segments.chunks(3).collect();
    for (i, chunk) in l0_chunks.iter().enumerate() {
        let seg_id = format!("A{}", i + 1);
        if chunk.len() == 3 {
            if let Some((_, simpler_spec, inv_spec)) = chunk.iter().collect_tuple() {
                if let (
                    DslSegmentSpecEnum::Spanish {
                        tokens: simpler_tokens,
                        ..
                    },
                    DslSegmentSpecEnum::InvDiglot { tuples },
                ) = (&simpler_spec.spec, &inv_spec.spec)
                {
                    let word_tokens: Vec<_> = simpler_tokens
                        .iter()
                        .filter(|t| t.token_type == JsonTokenType::Word)
                        .collect();
                    let entries = tuples
                        .iter()
                        .zip(word_tokens.iter())
                        .enumerate()
                        .map(|(idx, (t, _))| {
                            // --- FIX #2: Use the new field names ---
                            let eng_word_count =
                                t.base_substitute.split_whitespace().count();
                            (
                                idx,
                                t.target_lemmas.clone(),
                                t.base_substitute.clone(),
                                eng_word_count,
                            )
                            // --- END FIX #2 ---
                        })
                        .collect();
                    json_sentence
                        .mappings
                        .adv_target_to_base_inv_diglot
                        .insert(seg_id.clone(), entries);
                }
            }
        }
    }

    let mock_chapter = JsonChapter {
        book_meta: JsonBookMetaV2 {
            book_name: test_case.name.clone(),
            ..Default::default()
        },
        content_blocks: vec![JsonContentBlock::Sentence(json_sentence.clone())],
    };

    let (numerical_chapter, _) =
        preprocessor::json_chapter_to_numerical(&mock_chapter, dictionary);
    (
        numerical_chapter.sentences_numerical.into_iter().next().unwrap(),
        json_sentence,
    )
}

fn get_expected_text<'s>(sub_test: &'s DslSubTest) -> &'s str {
    sub_test
        .assertions
        .iter()
        .find_map(|a| {
            if let DslAssertion::Text(txt) = a {
                Some(txt.as_str())
            } else {
                None
            }
        })
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
                let expected_level = if expected_level_str == "AdvancedWeave" {
                    OutputLevel::AdvancedWeave
                } else {
                    OutputLevel::SimpleHybrid
                };
                if output.level != expected_level {
                    is_passed = false;
                    reasons.push(format!(
                        "Level Mismatch: Expected {:?}, got {:?}",
                        expected_level, output.level
                    ));
                }
            }
            DslAssertion::Text(expected_text) => {
                if actual_text != *expected_text {
                    is_passed = false;
                    reasons.push(format!(
                        "Text Mismatch: Expected '{}', got '{}'",
                        expected_text, actual_text
                    ));
                }
            }
        }
    }
    (is_passed, reasons)
}