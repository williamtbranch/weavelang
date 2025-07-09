//*** START FILE: src/simulation/tests/test_generation.rs ***//
use crate::simulation::core_algo::{determine_and_annotate_sentence_expression, OutputLevel};
use crate::simulation::dictionary::GlobalLemmaDictionary;
use crate::simulation::frequency_manager;
use crate::simulation::numerical_types::{
    NumericalAdvSegmentBundle, NumericalLearnerProfile, NumericalProcessedSentence, NumericalSegmentData,
    NumericalSegmentLemmas, NumericalPhraseAlignmentToEng, NumericalDiglotSegmentMap, NumericalDiglotEntry
};
use crate::simulation::text_generator::{self}; // Removed clean_text_for_tts
use crate::types::json_types::JsonSentenceBlock;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use once_cell::sync::Lazy;

use super::weavetest_parser;

static TEST_SETUP: Lazy<()> = Lazy::new(|| {
    let dummy_freq_list_path = "assets/es_master_frequency_list.txt";
    fs::create_dir_all("assets").expect("Failed to create assets dir for test");
    let mut file = File::create(dummy_freq_list_path).expect("Failed to create dummy freq list");
    for i in 1..=50 {
        writeln!(file, "lem{}\t{}\t100", i, i).expect("Failed to write to dummy freq list");
    }
    frequency_manager::load_master_frequency_list(dummy_freq_list_path.as_ref()).expect("Failed to load dummy freq list");
});

// =================================================================================
// 1. DSL DATA STRUCTURES
// =================================================================================
#[derive(Debug, Clone)]
pub(crate) enum DslSegmentPart {
    Word(String),
    // CORRECTED: Renamed fields for clarity and correctness
    Diglot {
        word_to_replace: String,
        replacement_lemma: String,
        replacement_word: String,
    },
}
#[derive(Debug, Clone, Default)]
pub(crate) struct DslSegment {
    pub(crate) parts: Vec<DslSegmentPart>,
    pub(crate) lemmas: Vec<String>,
}
#[derive(Debug, Default)]
pub(crate) struct DslTestCase {
    pub(crate) name: String,
    pub(crate) learner_level: u32,
    pub(crate) sentence: DslSentence,
    pub(crate) assert_level: OutputLevel,
    pub(crate) assert_text: String,
}
#[derive(Debug, Default)]
pub(crate) struct DslSentence {
    pub(crate) adv_segments: Vec<String>,
    pub(crate) mod_segments: Vec<String>,
    pub(crate) simple_segments: Vec<String>,
    pub(crate) english_spans: Vec<String>,
}


#[test]
fn run_dsl_generation_test_suite() {
    Lazy::force(&TEST_SETUP);
    let dsl_content = include_str!("generation_tests.weavetest");
    
    let (max_lemmas, segments, test_cases) =
        match weavetest_parser::parse_weavetest_file(dsl_content) {
            Ok(data) => data,
            Err(e) => panic!("FATAL: Failed to parse `generation_tests.weavetest`.\nError: {}", e),
        };

    println!("\nParsed {} segments and {} test cases from DSL file. Max lemmas: {}", segments.len(), test_cases.len(), max_lemmas);

    let mut passed_count = 0;
    let mut failed_tests = Vec::new();

    for test_case in &test_cases {
        println!("\n============================================================");
        println!("TEST CASE: [{}]", test_case.name);
        println!("------------------------------------------------------------");

        validate_test_case_unambiguity(test_case, &segments);

        let mut dictionary = GlobalLemmaDictionary::new();
        let mut profile = NumericalLearnerProfile::new();
        for i in 1..=test_case.learner_level {
            let lemma_str = format!("lem{}", i);
            profile.activate_lemma(dictionary.get_id_or_insert(&lemma_str));
        }

        let numerical_sentence = compile_dsl_to_numerical(&test_case.sentence, &segments, &mut dictionary);
        
        let dummy_json_sentence = JsonSentenceBlock::default();

        let mut n_sentence_clone = numerical_sentence.clone();
        let output = determine_and_annotate_sentence_expression(&mut n_sentence_clone, &profile, &dictionary, 0.4);

        let actual_text = text_generator::generate_raw_text_from_levels(&[&dummy_json_sentence], &[output.clone()], false).expect("Raw text generation failed");
        
        let mut is_case_passed = true;
        let mut failure_reasons = Vec::new();

        if output.level != test_case.assert_level {
            is_case_passed = false;
            failure_reasons.push(format!("Level Mismatch: Expected {:?}, got {:?}", test_case.assert_level, output.level));
        }
        
        if actual_text != test_case.assert_text {
            is_case_passed = false;
            failure_reasons.push("Final text content does not match.".to_string());
        }
        
        println!("  Expected: {}", test_case.assert_text);
        println!("  Actual:   {}", actual_text);
        
        if is_case_passed {
            println!("  Result:   PASS");
            passed_count += 1;
        } else {
            println!("  Result:   FAIL\n  Reasons:");
            for reason in &failure_reasons { println!("    - {}", reason); }
            failed_tests.push(test_case.name.clone());
        }
        println!("============================================================");
    }
    
    if !failed_tests.is_empty() {
        println!("\n--- TEST SUITE SUMMARY ---");
        println!("{} / {} tests passed.", passed_count, test_cases.len());
        println!("\nFailed tests:");
        for name in failed_tests { println!("  - {}", name); }
        panic!("One or more test cases failed.");
    }
    
    println!("\n--- TEST SUITE SUMMARY ---");
    println!("All {} tests passed.", test_cases.len());
}

// --- AMBIGUITY CHECKER (Unchanged) ---
fn is_word_unsanitized(word: &str) -> bool {
    word.ends_with("-x") || word.ends_with("-i")
}
fn is_segment_def_sanitized(seg_name: &str, segments: &HashMap<String, DslSegment>) -> bool {
    let seg = segments.get(seg_name).unwrap_or_else(|| panic!("Segment '{}' not found", seg_name));
    !seg.parts.iter().any(|part| match part {
        DslSegmentPart::Word(w) => is_word_unsanitized(w),
        DslSegmentPart::Diglot { word_to_replace, replacement_word, .. } => is_word_unsanitized(word_to_replace) || is_word_unsanitized(replacement_word),
    })
}
fn validate_test_case_unambiguity(test_case: &DslTestCase, segments: &HashMap<String, DslSegment>) {
    let l0_is_clean = check_l0_path_sanity(test_case, segments);
    let l1_is_clean = check_l1_path_sanity(test_case, segments);
    if l0_is_clean == l1_is_clean {
        panic!("\nTEST DEFINITION ERROR in [{}]: Ambiguity or lack of golden path.\n  L0 Clean: {}\n  L1 Clean: {}\nExactly one path must be fully clean.", test_case.name, l0_is_clean, l1_is_clean);
    }
}
fn check_l0_path_sanity(test_case: &DslTestCase, segments: &HashMap<String, DslSegment>) -> bool {
    if test_case.sentence.adv_segments.is_empty() { return false; }
    for i in 0..test_case.sentence.adv_segments.len() {
        let adv_is_clean = is_segment_def_sanitized(&test_case.sentence.adv_segments[i], segments);
        let mod_is_clean = is_segment_def_sanitized(&test_case.sentence.mod_segments[i], segments);
        if !(adv_is_clean ^ mod_is_clean) { return false; }
    }
    true
}
fn check_l1_path_sanity(test_case: &DslTestCase, segments: &HashMap<String, DslSegment>) -> bool {
    if test_case.sentence.simple_segments.is_empty() { return false; }
    for i in 0..test_case.sentence.simple_segments.len() {
        let ss_is_clean = is_segment_def_sanitized(&test_case.sentence.simple_segments[i], segments);
        let en_span_name = &test_case.sentence.english_spans[i];
        let en_seg = segments.get(en_span_name).unwrap();
        let mut en_path_is_clean = true;
        for part in &en_seg.parts {
            match part {
                DslSegmentPart::Word(w) => { if is_word_unsanitized(w) { en_path_is_clean = false; break; } },
                DslSegmentPart::Diglot { word_to_replace, replacement_word, .. } => {
                    if is_word_unsanitized(word_to_replace) == is_word_unsanitized(replacement_word) { en_path_is_clean = false; break; }
                }
            }
        }
        if !(ss_is_clean ^ en_path_is_clean) { return false; }
    }
    true
}


// --- DSL COMPILER (CORRECTED) ---
fn compile_dsl_to_numerical(
    dsl_sentence: &DslSentence,
    segments: &HashMap<String, DslSegment>,
    dictionary: &mut GlobalLemmaDictionary,
) -> NumericalProcessedSentence {
    let mut num_sentence = NumericalProcessedSentence::default();

    let get_seg = |seg_name: &str| -> &DslSegment {
        segments.get(seg_name).unwrap_or_else(|| panic!("TEST DEFINITION ERROR: Tried to use segment '{}' which was never defined.", seg_name))
    };

    let text_from_parts = |parts: &[DslSegmentPart]| -> String {
        parts.iter().map(|p| match p {
            DslSegmentPart::Word(w) => w.clone(),
            DslSegmentPart::Diglot { word_to_replace, .. } => word_to_replace.clone(),
        }).collect::<Vec<_>>().join(" ")
    };

    for i in 0..dsl_sentence.adv_segments.len() {
        let adv_seg = get_seg(&dsl_sentence.adv_segments[i]);
        let mod_seg = get_seg(&dsl_sentence.mod_segments[i]);
        
        num_sentence.adv_segment_bundles_numerical.push(NumericalAdvSegmentBundle {
            a_id_str: format!("A{}", i + 1),
            adv_text_original: text_from_parts(&adv_seg.parts),
            adv_lemma_ids: adv_seg.lemmas.iter().map(|l| dictionary.get_id_or_insert(l)).collect(),
            simpler_text_original: text_from_parts(&mod_seg.parts),
            simpler_lemma_ids: mod_seg.lemmas.iter().map(|l| dictionary.get_id_or_insert(l)).collect(),
            inverse_diglot_map_numerical: HashMap::new(),
        });
    }

    for i in 0..dsl_sentence.simple_segments.len() {
        let ss_seg = get_seg(&dsl_sentence.simple_segments[i]);
        let en_span = get_seg(&dsl_sentence.english_spans[i]);
        let ss_text = text_from_parts(&ss_seg.parts);
        let en_text = text_from_parts(&en_span.parts);

        num_sentence.sims_l3_segments_numerical.push(NumericalSegmentData {
            id_str: format!("S{}", i + 1),
            text_original: ss_text.clone(),
        });

        num_sentence.l3_simsl_per_segment_numerical.push(NumericalSegmentLemmas {
            segment_id_str: format!("S{}", i + 1),
            lemma_ids: ss_seg.lemmas.iter().map(|l| dictionary.get_id_or_insert(l)).collect(),
        });
        
        num_sentence.phrase_alignments_l3_to_eng_numerical.push(NumericalPhraseAlignmentToEng {
            s_segment_id_str: format!("S{}", i + 1),
            sims_l3_segment_text_original: ss_text,
            eng_span_text_original: en_text.clone(),
            eng_span_word_count: en_text.split_whitespace().count(),
        });

        let mut diglot_entries = Vec::new();
        for part in &en_span.parts {
            if let DslSegmentPart::Diglot{ word_to_replace, replacement_lemma, replacement_word } = part {
                diglot_entries.push(NumericalDiglotEntry {
                    eng_word_original: word_to_replace.clone(),
                    spa_lemma_id: dictionary.get_id_or_insert(replacement_lemma),
                    exact_spa_form_original: replacement_word.clone(),
                    viable: !is_word_unsanitized(replacement_word),
                });
            }
        }
        if !diglot_entries.is_empty() {
             num_sentence.diglot_map_numerical.push(NumericalDiglotSegmentMap {
                s_segment_id_str: format!("S{}", i + 1),
                entries: diglot_entries,
            });
        }
    }
    num_sentence
}
//*** END FILE: src/simulation/tests/test_generation.rs ***//