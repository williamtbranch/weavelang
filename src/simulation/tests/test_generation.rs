// src/simulation/tests/test_generation.rs

use super::weavetest_parser::{
    self, DslAssertion, DslColumnBody, DslSegmentSpec, DslSubTest, DslTestCase,
};
use crate::simulation::core_algo::{determine_and_annotate_sentence_expression, OutputLevel, ChosenLevelOutput};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;
use crate::simulation::numerical_types::{
    NumericalAdvSegmentBundle, NumericalLearnerProfile, NumericalProcessedSentence,
    NumericalSegmentData, NumericalSegmentLemmas, NumericalPhraseAlignmentToEng,
    NumericalDiglotSegmentMap, NumericalDiglotEntry,
};
use crate::simulation::text_generator;
use crate::types::json_types::JsonSentenceBlock;
use std::fs::{self, File};
use std::io::Write;
use once_cell::sync::Lazy;
use itertools::Itertools;

// --- TEST SETUP & RUNNER ---

static TEST_SETUP: Lazy<()> = Lazy::new(|| {
    let dummy_freq_list_path = "assets/es_master_frequency_list.txt";
    fs::create_dir_all("assets").expect("Failed to create assets dir for test");
    let mut file = File::create(dummy_freq_list_path).expect("Failed to create dummy freq list");
    for i in 1..=10000 {
        writeln!(file, "lem{}\t{}\t100", i, i).expect("Failed to write to dummy freq list");
    }
    frequency_manager::load_master_frequency_list(dummy_freq_list_path.as_ref()).expect("Failed to load dummy freq list");
});

#[test]
fn run_dsl_generation_test_suite() {
    Lazy::force(&TEST_SETUP);
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
        let numerical_sentence = compile_dsl_sentence_to_numerical(test_case, &mut dictionary);

        for sub_test in &test_case.sub_tests {
            println!("\n  Sub-Test: [{}]", sub_test.name);

            let mut profile = NumericalLearnerProfile::new();
            for i in 1..=sub_test.learner_level {
                profile.activate_lemma(dictionary.get_id_or_insert(&format!("lem{}", i)));
            }

            let mut n_sentence_clone = numerical_sentence.clone();
            let output = determine_and_annotate_sentence_expression(&mut n_sentence_clone, &profile, &dictionary, 0.4);

            let raw_text_with_escapes = text_generator::generate_raw_text_from_levels(&[&JsonSentenceBlock::default()], &[output.clone()], false).unwrap();
            let actual_text_with_escapes = text_generator::clean_text_for_tts(&raw_text_with_escapes);

            let (is_passed, failure_reasons) = run_assertions(sub_test, &output, &actual_text_with_escapes);
            
            let expected_text_for_display = get_expected_text(sub_test).replace("\\\"", "\"");
            let actual_text_for_display = actual_text_with_escapes.replace("\\\"", "\"");

            println!("    Expected: {}", expected_text_for_display);
            println!("    Actual:   {}", actual_text_for_display);

            if is_passed {
                println!("    Result:   PASS");
                total_passed += 1;
            } else {
                println!("    Result:   FAIL\n    Reasons:");
                for reason in &failure_reasons { println!("      - {}", reason); }
                total_failed += 1;
                failed_details.push(format!("Test Case: '{}', Sub-Test: '{}'", test_case.name, sub_test.name));
            }
        }
    }

    println!("\n============================================================");
    if total_failed > 0 {
        println!("\n--- TEST SUITE SUMMARY ---");
        println!("{} / {} sub-tests passed.", total_passed, total_passed + total_failed);
        println!("\nFailed sub-tests:");
        for detail in failed_details { println!("  - {}", detail); }
        panic!("One or more sub-tests failed.");
    }

    println!("\n--- TEST SUITE SUMMARY ---");
    println!("All {} sub-tests passed.", total_passed);
}


// --- COMPILER ---

fn compile_dsl_sentence_to_numerical(
    test_case: &DslTestCase,
    dictionary: &mut GlobalLemmaDictionary,
) -> NumericalProcessedSentence {
    let mut num_sentence = NumericalProcessedSentence::default();

    let l0_segments = if let DslColumnBody::L0(segments) = &test_case.sentence_def.l0_def { segments } else { unreachable!() };
    for (i, chunk) in l0_segments.chunks(3).enumerate() {
        let col_num = i + 1;
        let Some((adv_spec, mod_spec, inv_spec)) = chunk.iter().collect_tuple() else { panic!("L0 def must have segments in multiples of 3.") };
        let DslSegmentSpec::Spanish { phrase: adv_phrase, lemmas: adv_lemmas } = adv_spec else { panic!("L0 Adv must be Spanish") };
        let DslSegmentSpec::Spanish { phrase: mod_phrase, lemmas: mod_lemmas } = mod_spec else { panic!("L0 Mod must be Spanish") };
        let DslSegmentSpec::InvDiglot { tuples: inv_tuples } = inv_spec else { panic!("L0 Inv must be InvDiglot") };
        
        // --- FIX: Create a longer-lived String to borrow from ---
        let joined_mod_phrase = mod_phrase.join(" ");
        let mod_phrase_words: Vec<&str> = joined_mod_phrase.split_whitespace().collect();

        if mod_phrase_words.len() != inv_tuples.len() {
            panic!(
                "Test Case '{}', L0 Segment {}: Count mismatch. Moderate Spanish phrase has {} words, but Inverse Diglot has {} tuples.",
                test_case.name, col_num, mod_phrase_words.len(), inv_tuples.len()
            );
        }
        for (idx, word) in mod_phrase_words.iter().enumerate() {
            let cleaned_word = word.trim_matches(|c: char| !c.is_alphanumeric());
            if cleaned_word != inv_tuples[idx].spanish_word {
                panic!(
                    "Test Case '{}', L0 Segment {}: Mismatch at word {}. Moderate Spanish has '{}' (cleaned to '{}'), but Inverse Diglot tuple expects '{}'.",
                    test_case.name, col_num, idx + 1, word, cleaned_word, inv_tuples[idx].spanish_word
                );
            }
        }

        num_sentence.adv_segment_bundles_numerical.push(NumericalAdvSegmentBundle {
            a_id_str: format!("A{}", col_num),
            adv_text_original: adv_phrase.join(" "),
            adv_lemma_ids: adv_lemmas.iter().map(|l| dictionary.get_id_or_insert(l)).collect(),
            simpler_text_original: mod_phrase.join(" "),
            simpler_lemma_ids: mod_lemmas.iter().map(|l| dictionary.get_id_or_insert(l)).collect(),
            inverse_diglot_map_numerical: inv_tuples.iter().map(|t| (
                t.spanish_word.clone(),
                dictionary.get_id_or_insert(&t.spanish_lemma),
                t.english_substitute.clone()
            )).collect(),
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
        
        // --- FIX: Create a longer-lived String to borrow from ---
        let joined_eng_phrase = eng_phrase.join(" ");
        let eng_phrase_words: Vec<&str> = joined_eng_phrase.split_whitespace().collect();

        if eng_phrase_words.len() != dig_tuples.len() {
            panic!(
                "Test Case '{}', L1 Segment {}: Count mismatch. English phrase has {} words, but Diglot block has {} tuples.",
                test_case.name, col_num, eng_phrase_words.len(), dig_tuples.len()
            );
        }
        for (idx, word) in eng_phrase_words.iter().enumerate() {
            let cleaned_word = word.trim_matches(|c: char| !c.is_alphanumeric());
            if cleaned_word != dig_tuples[idx].word_to_replace {
                panic!(
                    "Test Case '{}', L1 Segment {}: Mismatch at word {}. English phrase has '{}' (cleaned to '{}'), but Diglot tuple expects '{}'.",
                    test_case.name, col_num, idx + 1, word, cleaned_word, dig_tuples[idx].word_to_replace
                );
            }
        }

        num_sentence.sims_l3_segments_numerical.push(NumericalSegmentData { id_str: s_id.clone(), text_original: sim_phrase.join(" ") });
        num_sentence.l3_simsl_per_segment_numerical.push(NumericalSegmentLemmas { segment_id_str: s_id.clone(), lemma_ids: sim_lemmas.iter().map(|l| dictionary.get_id_or_insert(l)).collect() });
        num_sentence.phrase_alignments_l3_to_eng_numerical.push(NumericalPhraseAlignmentToEng {
            s_segment_id_str: s_id.clone(),
            sims_l3_segment_text_original: sim_phrase.join(" "),
            eng_span_text_original: eng_phrase.join(" "),
            eng_span_word_count: eng_phrase_words.len(),
        });
        num_sentence.diglot_map_numerical.push(NumericalDiglotSegmentMap {
            s_segment_id_str: s_id,
            entries: dig_tuples.iter().map(|t| NumericalDiglotEntry {
                eng_word_original: t.word_to_replace.clone(),
                spa_lemma_id: dictionary.get_id_or_insert(&t.replacement_lemma),
                exact_spa_form_original: t.replacement_word.clone(),
                viable: t.is_viable,
            }).collect()
        });
    }
    num_sentence
}

// --- TEST ASSERTION HELPERS ---
fn get_expected_text<'s>(sub_test: &'s DslSubTest) -> &'s str {
    sub_test.assertions.iter().find_map(|a|
        if let DslAssertion::Text(txt) = a { Some(txt.as_str()) } else { None }
    ).unwrap_or("")
}

fn run_assertions(
    sub_test: &DslSubTest,
    output: &ChosenLevelOutput,
    actual_text_with_escapes: &str
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
            DslAssertion::Text(expected_text_with_escapes) => {
                if actual_text_with_escapes != expected_text_with_escapes {
                    is_passed = false;
                    let display_expected = expected_text_with_escapes.replace("\\\"", "\"");
                    let display_actual = actual_text_with_escapes.replace("\\\"", "\"");
                    reasons.push(format!("Text Mismatch: Expected '{}', got '{}'", display_expected, display_actual));
                }
            }
        }
    }
    (is_passed, reasons)
}