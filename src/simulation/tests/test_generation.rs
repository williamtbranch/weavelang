// src/simulation/tests/test_generation.rs
use super::weavetest_parser::{
    self, DslAssertion, DslColumnBody, DslSegmentSpec, DslSubTest, DslTestCase,
};
use crate::simulation::core_algo::{determine_and_annotate_sentence_expression, ChosenLevelOutput, OutputLevel};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;
use crate::simulation::numerical_types::{
    NumericalAdvSegmentBundle, NumericalDiglotEntry, NumericalDiglotSegmentMap,
    NumericalLearnerProfile, NumericalPhraseAlignmentToEng, NumericalProcessedSentence,
    NumericalSegmentData, NumericalSegmentLemmas,
};
use crate::simulation::preprocessor; // Use the preprocessor module
use crate::simulation::text_generator;
use crate::types::json_types::{JsonAdvSpanishSegment, JsonSentenceBlock}; // Use specific json types
use itertools::Itertools;
use once_cell::sync::Lazy;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

// --- TEST SETUP & RUNNER ---
static TEST_SETUP: Lazy<Mutex<()>> = Lazy::new(|| {
    let guard = Mutex::new(());
    let test_assets_dir = PathBuf::from("target/test_assets");
    fs::create_dir_all(&test_assets_dir).expect("Failed to create test assets dir");
    let test_freq_list_path = test_assets_dir.join("test_frequency_list.txt");
    {
        let mut file =
            File::create(&test_freq_list_path).expect("Failed to create test frequency list");
        writeln!(file, "lemma\trank\toccurrences").expect("Failed to write header");
        for i in 1..=20000 {
            writeln!(file, "lem{}\t{}\t100", i, i)
                .expect("Failed to write to test frequency list");
        }
        // Add real lemmas from the debug test to ensure they are NOT treated as "rare".
        let real_lemmas = vec![
            "quien", "ser", "tu", "decir", "el", "de", "gusano", "sedar", "oruga", "quién", "tú",
        ];
        let mut rank = 20001;
        for lemma in real_lemmas {
            writeln!(file, "{}\t{}\t100", lemma, rank).expect("Failed to write real lemma");
            rank += 1;
        }
    }
    frequency_manager::load_master_frequency_list(&test_freq_list_path)
        .expect("Failed to load test frequency list");
    guard
});

#[test]
fn run_dsl_generation_test_suite() {
    let _guard = TEST_SETUP.lock().unwrap();
    let dsl_content = include_str!("generation_tests.weavetest");
    let test_cases = match weavetest_parser::parse_weavetest_file(dsl_content) {
        Ok(data) => data,
        Err(e) => panic!("FATAL: Failed to parse `generation_tests.weavetest`.\nError: {}", e),
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
            let mut n_sentence_clone = numerical_sentence.clone();
            let output =
                determine_and_annotate_sentence_expression(&mut n_sentence_clone, &profile, &dictionary, 0.4);

            let raw_text =
                text_generator::generate_raw_text_from_levels(&[&json_sentence], &[output.clone()], false)
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
        println!("{} / {} sub-tests passed.", total_passed, total_passed + total_failed);
        println!("\nFailed sub-tests:");
        for detail in failed_details {
            println!("  - {}", detail);
        }
        panic!("One or more sub-tests failed.");
    }
    println!("\n--- TEST SUITE SUMMARY ---");
    println!("All {} sub-tests passed.", total_passed);
}

// --- COMPILER & ASSERTION HELPERS ---


fn compile_dsl_sentence_to_numerical(
    test_case: &DslTestCase,
    dictionary: &mut GlobalLemmaDictionary,
) -> (NumericalProcessedSentence, JsonSentenceBlock) {
    let mut json_sentence = JsonSentenceBlock::default();

    // Convert DSL into intermediate JSON-like structures
    let l0_segments = if let DslColumnBody::L0(segments) = &test_case.sentence_def.l0_def { segments } else { unreachable!() };
    for (i, chunk) in l0_segments.chunks(3).enumerate() {
        let Some((adv_spec, mod_spec, inv_spec)) = chunk.iter().collect_tuple() else { panic!("L0 def must have segments in multiples of 3.") };
        let DslSegmentSpec::Spanish { phrase: adv_phrase, lemmas: adv_lemmas } = adv_spec else { panic!("L0 Adv must be Spanish") };
        let DslSegmentSpec::Spanish { phrase: mod_phrase, lemmas: mod_lemmas } = mod_spec else { panic!("L0 Mod must be Spanish") };
        let DslSegmentSpec::InvDiglot { tuples: inv_tuples } = inv_spec else { panic!("L0 Inv must be InvDiglot") };

        json_sentence.adv_spanish_segments.push(JsonAdvSpanishSegment {
            segment_id: format!("A{}", i + 1),
            // *** FIX: Use the String directly, no join needed ***
            advanced_text: adv_phrase.clone(),
            advanced_lemmas: adv_lemmas.clone(),
            // *** FIX: Use the String directly, no join needed ***
            simpler_text: mod_phrase.clone(),
            simpler_lemmas: mod_lemmas.clone(),
            inverse_diglot_map: inv_tuples.iter().map(|t| crate::types::json_types::JsonInverseDiglotMapEntry {
                spanish_word: t.spanish_word.clone(),
                spanish_lemma: t.spanish_lemma.clone(),
                english_substitute: t.english_substitute.clone(),
            }).collect(),
        });
    }

    let l1_segments = if let DslColumnBody::L1(segments) = &test_case.sentence_def.l1_def { segments } else { unreachable!() };
    for (i, chunk) in l1_segments.chunks(3).enumerate() {
        let col_num = i + 1;
        let s_id = format!("S{}", col_num);
        let Some((sim_spec, dig_spec, eng_spec)) = chunk.iter().collect_tuple() else { panic!("L1 def must have segments in multiples of 3.") };
        let DslSegmentSpec::Spanish { phrase: sim_phrase, lemmas: sim_lemmas } = sim_spec else { panic!("L1 Sim must be Spanish") };
        let DslSegmentSpec::Diglot { tuples: dig_tuples } = dig_spec else { panic!("L1 Dig must be Diglot") };
        let DslSegmentSpec::English { phrase: eng_phrase } = eng_spec else { panic!("L1 Eng must be English") };
        json_sentence.simple_spanish_l3_segments.push(crate::types::json_types::JsonSimpleSpanishL3Segment {
            segment_id: s_id.clone(),
            // *** FIX: Use the String directly, no join needed ***
            simple_text: sim_phrase.clone(),
        });
        json_sentence.phrase_alignments_l3_to_english.push(crate::types::json_types::JsonPhraseAlignmentL3ToEng {
            segment_id: s_id.clone(),
            // *** FIX: Use the String directly, no join needed ***
            simple_spanish_text: sim_phrase.clone(),
            // *** FIX: Use the String directly, no join needed ***
            english_span_text: eng_phrase.clone(),
        });
        json_sentence.simple_spanish_l3_lemmas_per_segment.insert(s_id.clone(), sim_lemmas.clone());
        for t in dig_tuples {
            json_sentence.diglot_map_entries.push(crate::types::json_types::JsonDiglotMapEntry {
                segment_id: s_id.clone(),
                english_word: t.word_to_replace.clone(),
                spanish_lemma: t.replacement_lemma.clone(),
                exact_spanish_form: t.replacement_word.clone(),
                is_viable_for_substitution: t.is_viable,
                note: if t.is_viable { "viable".to_string() } else { "not_viable".to_string() },
            });
        }
    }
    
    // Now, run the actual preprocessor on the constructed JsonSentenceBlock
    let numerical_sentence = preprocessor::json_sentence_to_numerical(&json_sentence, dictionary, &test_case.name);
    (numerical_sentence, json_sentence)
}

fn get_expected_text<'s>(sub_test: &'s DslSubTest) -> &'s str {
    sub_test.assertions.iter().find_map(|a|
        if let DslAssertion::Text(txt) = a { Some(txt.as_str()) } else { None }
    ).unwrap_or("")
}

fn run_assertions(
    sub_test: &DslSubTest,
    output: &ChosenLevelOutput,
    actual_text: &str
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
            },
            DslAssertion::Text(expected_text) => {
                if actual_text != expected_text {
                    is_passed = false;
                    reasons.push(format!("Text Mismatch: Expected '{}', got '{}'", expected_text, actual_text));
                }
            }
        }
    }
    (is_passed, reasons)
}

// The debug_caterpillar_bug test is no longer needed as the main suite now covers it.
// You can remove it to clean up the file. If you want to keep it,
// you would need to construct a full JsonSentenceBlock manually instead of parsing a string.