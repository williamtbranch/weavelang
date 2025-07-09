use crate::simulation::core_algo::OutputLevel;
use crate::simulation::tests::test_generation::{DslSegment, DslSegmentPart, DslTestCase};
use pest::iterators::{Pair, Pairs};
use pest::Parser;
use std::collections::HashMap;

#[derive(pest_derive::Parser)]
#[grammar = "simulation/tests/generation_tests.pest"]
struct WeaveTestParser;

fn parse_segment_body(pair: Pair<Rule>) -> DslSegment {
    let mut inner = pair.into_inner();
    let mut segment = DslSegment::default();
    let part_list_pair = inner.next().unwrap();
    for segment_part_pair in part_list_pair.into_inner() {
        let mut part_inner = segment_part_pair.into_inner();
        let part_content_pair = part_inner.next().unwrap();
        match part_content_pair.as_rule() {
            Rule::diglot_def => {
                let mut diglot_inner = part_content_pair.into_inner();
                let word_to_replace = diglot_inner.next().unwrap().as_str().to_string();
                let replacement_lemma = diglot_inner.next().unwrap().as_str().to_string();
                let replacement_word = diglot_inner.next().unwrap().as_str().to_string();
                segment.parts.push(DslSegmentPart::Diglot {
                    word_to_replace,
                    replacement_lemma,
                    replacement_word,
                });
            }
            Rule::identifier => {
                segment.parts.push(DslSegmentPart::Word(part_content_pair.as_str().to_string()));
            }
            _ => unreachable!(),
        }
    }
    if let Some(lemma_list_pair) = inner.next() {
        segment.lemmas = lemma_list_pair.into_inner().map(|p| p.as_str().to_string()).collect();
    }
    segment
}
// ... (rest of parser file is unchanged) ...
fn parse_test_case_body(pairs: Pairs<Rule>, test_case: &mut DslTestCase) {
    for pair in pairs {
        let rule = pair.as_rule();
        let mut inner = pair.into_inner();

        match rule {
            Rule::level_def => {
                test_case.learner_level = inner.next().unwrap().as_str().parse().expect("Invalid number for #level");
            }
            Rule::sentence_def => {
                for seg_type_pair in inner {
                    let seg_rule = seg_type_pair.as_rule();
                    let seg_inner = seg_type_pair.into_inner();
                    let seg_names = seg_inner.map(|p| p.as_str().to_string()).collect();
                    
                    match seg_rule {
                        Rule::adv_segs => test_case.sentence.adv_segments = seg_names,
                        Rule::mod_segs => test_case.sentence.mod_segments = seg_names,
                        Rule::simple_segs => test_case.sentence.simple_segments = seg_names,
                        Rule::eng_spans => test_case.sentence.english_spans = seg_names,
                        _ => unreachable!("Unexpected rule inside sentence_def"),
                    }
                }
            }
            Rule::assert_level => {
                let level_str = inner.next().unwrap().as_str();
                test_case.assert_level = if level_str == "AdvancedWeave" {
                    OutputLevel::AdvancedWeave
                } else {
                    OutputLevel::SimpleHybrid
                };
            }
            Rule::assert_text => {
                test_case.assert_text = inner.next().unwrap().as_str().trim_matches('"').to_string();
            }
            _ => unreachable!("Unexpected rule inside test_case_body"),
        }
    }
}
pub fn parse_weavetest_file(
    file_content: &str,
) -> Result<(u32, HashMap<String, DslSegment>, Vec<DslTestCase>), pest::error::Error<Rule>> {
    let mut max_lemmas = 0;
    let mut segments = HashMap::new();
    let mut test_cases = Vec::new();

    let file_pair = WeaveTestParser::parse(Rule::test_suite, file_content)?.next().unwrap();

    for pair in file_pair.into_inner() {
        match pair.as_rule() {
            Rule::file_item => {
                let mut inner_item = pair.into_inner();
                let item_content = inner_item.next().unwrap();
                let rule = item_content.as_rule();
                let mut inner = item_content.into_inner();

                match rule {
                    Rule::max_lemmas => {
                        max_lemmas = inner.next().unwrap().as_str().parse().expect("Invalid number for #max-lemmas");
                    }
                    Rule::segment_def => {
                        let name = inner.next().unwrap().as_str().to_string();
                        let segment_body_pair = inner.next().unwrap();
                        segments.insert(name, parse_segment_body(segment_body_pair));
                    }
                    Rule::test_case => {
                        let name = inner.next().unwrap().as_str().trim_matches('"').to_string();
                        let mut test_case = DslTestCase { name, ..Default::default() };
                        let body_pairs = inner.next().unwrap().into_inner();
                        parse_test_case_body(body_pairs, &mut test_case);
                        test_cases.push(test_case);
                    }
                    _ => unreachable!("A file_item contained an unexpected rule: {:?}", rule),
                }
            }
            Rule::WHITESPACE | Rule::COMMENT | Rule::EOI => (),
            _ => unreachable!("Parser grammar should not allow this at top level: {:?}", pair.as_rule()),
        }
    }
    Ok((max_lemmas, segments, test_cases))
}