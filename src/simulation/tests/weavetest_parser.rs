// src/simulation/tests/weavetest_parser.rs
use pest::{iterators::Pair, Parser};
use crate::types::json_types::{JsonTokenV2, JsonTokenType};

// --- CORE DATA STRUCTURES ---

#[derive(Debug, Clone)]
pub struct DslTestCase {
    pub name: String,
    pub sentence_def: DslSentenceDef,
    pub sub_tests: Vec<DslSubTest>,
}

#[derive(Debug, Clone)]
pub struct DslSubTest {
    pub name: String,
    pub learner_level: DslLearnerLevel,
    pub assertions: Vec<DslAssertion>,
}

#[derive(Debug, Clone, Default)]
pub struct DslLearnerLevel {
    pub sim: u32,
    pub bas: u32,
    pub mod_level: u32,
    pub adv: u32,
}

pub const EXHAUSTED_LEVEL: u32 = u32::MAX;

// --- DslSentenceDef now contains the inverse diglot tuples ---
#[derive(Debug, Clone, Default)]
pub struct DslSentenceDef {
    pub l0_adv_segments: Vec<DslSegment>,
    pub l0_mod_segments: Vec<DslSegment>,
    pub l1_basic_spanish: DslSegment,
    pub l1_inv_diglot_tuples: Vec<DslInvDiglotTuple>, // <-- NEW FIELD
    pub l1_basic_english: DslSegment,
    pub l1_diglot_tuples: Vec<DslDiglotTuple>,
}

#[derive(Debug, Clone, Default)]
pub struct DslSegment {
    pub tokens: Vec<JsonTokenV2>,
    pub lemmas: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DslDiglotTuple {
    pub word_to_replace: String,
    pub replacement_lemmas: Vec<String>,
    pub replacement_word: String,
    pub is_viable: bool,
    pub is_proper_noun: bool,
}

// --- Re-added the DslInvDiglotTuple struct ---
#[derive(Debug, Clone)]
pub struct DslInvDiglotTuple {
    pub target_word: String,
    pub target_lemmas: Vec<String>,
    pub base_substitute: String,
}

#[derive(Debug, Clone)]
pub enum DslAssertion {
    Level(String),
    Text(String),
}

#[derive(pest_derive::Parser)]
#[grammar = "simulation/tests/generation_tests.pest"]
pub struct WeaveTestParser;

// --- PUBLIC PARSING FUNCTION ---
pub fn parse_weavetest_file(file_content: &str) -> Result<Vec<DslTestCase>, pest::error::Error<Rule>> {
    let file_pair = WeaveTestParser::parse(Rule::test_suite, file_content)?.next().unwrap();
    Ok(file_pair.into_inner().filter(|p| p.as_rule() == Rule::test_case).map(parse_test_case).collect())
}

// --- PARSING IMPLEMENTATION (with changes) ---

fn parse_test_case(pair: Pair<Rule>) -> DslTestCase {
    let mut inner = pair.into_inner();
    let name = parse_string_literal_content(inner.next().unwrap());
    let body_pair = inner.next().unwrap();
    let mut sentence_def = DslSentenceDef::default();
    let mut sub_tests = Vec::new();
    for part in body_pair.into_inner() {
        match part.as_rule() {
            Rule::sentence_def => sentence_def = parse_sentence_def(part),
            Rule::sub_test => sub_tests.push(parse_sub_test(part)),
            _ => (), 
        }
    }
    DslTestCase { name, sentence_def, sub_tests }
}

fn parse_sub_test(pair: Pair<Rule>) -> DslSubTest {
    let mut inner = pair.into_inner();
    let name = parse_string_literal_content(inner.next().unwrap());
    let body_pair = inner.next().unwrap();
    let mut learner_level = DslLearnerLevel::default();
    let mut assertions = Vec::new();
    for part in body_pair.into_inner() {
        match part.as_rule() {
            Rule::learner_level => learner_level = parse_learner_level(part),
            Rule::assertion => assertions.push(parse_assertion(part)),
            _ => (),
        }
    }
    DslSubTest { name, learner_level, assertions }
}

fn parse_learner_level(pair: Pair<Rule>) -> DslLearnerLevel {
    let mut level = DslLearnerLevel::default();
    for def_pair in pair.into_inner() {
        let inner_pair = def_pair.into_inner().next().unwrap();
        let rule = inner_pair.as_rule();
        let value_pair = inner_pair.into_inner().next().unwrap();
        let value = match value_pair.as_str() {
            "exhausted" => EXHAUSTED_LEVEL,
            num_str => num_str.parse().unwrap(),
        };
        match rule {
            Rule::sim_level => level.sim = value,
            Rule::bas_level => level.bas = value,
            Rule::mod_level => level.mod_level = value,
            Rule::adv_level => level.adv = value,
            _ => unreachable!(),
        }
    }
    level
}

// --- This function is the main change ---
fn parse_sentence_def(pair: Pair<Rule>) -> DslSentenceDef {
    let mut inner = pair.into_inner();
    let mut def = DslSentenceDef::default();

    // Parse L0 Column (Adv/Mod pairs)
    let l0_body = inner.next().unwrap();
    let l0_segments: Vec<_> = l0_body.into_inner().map(parse_phrase_and_lemmas).collect();
    if l0_segments.len() % 2 != 0 {
        panic!("DSL L0 Error: Spanish segments must be in pairs (Advanced, Moderate). Found an odd number of segments.");
    }
    for chunk in l0_segments.chunks(2) {
        def.l0_adv_segments.push(chunk[0].clone());
        def.l0_mod_segments.push(chunk[1].clone());
    }

    // Parse L1 Column (BS, ID, BE, D)
    let l1_body = inner.next().unwrap();
    let mut l1_inner = l1_body.into_inner();
    def.l1_basic_spanish = parse_phrase_and_lemmas(l1_inner.next().unwrap());
    def.l1_inv_diglot_tuples = l1_inner.next().unwrap().into_inner().map(parse_inv_diglot_tuple).collect(); // <-- PARSE ID
    def.l1_basic_english = parse_phrase_and_lemmas(l1_inner.next().unwrap());
    def.l1_diglot_tuples = l1_inner.next().unwrap().into_inner().map(parse_diglot_tuple).collect();

    def
}

fn parse_assertion(pair: Pair<Rule>) -> DslAssertion {
    let assertion_pair = pair.into_inner().next().unwrap();
    let rule = assertion_pair.as_rule();
    let value_pair = assertion_pair.into_inner().next().unwrap();
    match rule {
        Rule::assert_level => DslAssertion::Level(value_pair.as_str().to_string()),
        Rule::assert_text => DslAssertion::Text(parse_string_literal_content(value_pair)),
        _ => unreachable!(),
    }
}

// --- Helper for parsing a generic segment ---
fn parse_phrase_and_lemmas(pair: Pair<Rule>) -> DslSegment {
    let mut inner = pair.into_inner();
    let phrase_pair = inner.next().unwrap();
    let content_pair = phrase_pair.into_inner().next().unwrap();
    let content_rule = content_pair.as_rule();
    let content = parse_string_literal_content(content_pair);

    let tokens = match content_rule {
        Rule::tokenizedPhrase => tokenize_bracketed_phrase(&content),
        Rule::stringLiteral => tokenize_simple_string(&content),
        _ => unreachable!(),
    };
    
    let lemmas = inner.next()
        .map(|p| p.into_inner().map(|l| l.as_str().to_string()).collect())
        .unwrap_or_default();

    DslSegment { tokens, lemmas }
}

// --- Re-added the parser for inverse diglot tuples ---
fn parse_inv_diglot_tuple(pair: Pair<Rule>) -> DslInvDiglotTuple {
    let mut inner = pair.into_inner();
    let target_word = inner.next().unwrap().as_str().to_string().replace("__", " ");
    let combined_lemmas_str = inner.next().unwrap().as_str();
    let target_lemmas = combined_lemmas_str.split("__").map(|s| s.to_string()).collect();
    let base_substitute = inner.next().unwrap().as_str().to_string().replace("__", " ");
    
    DslInvDiglotTuple {
        target_word,
        target_lemmas,
        base_substitute,
    }
}

// --- All other helpers are unchanged ---

fn parse_diglot_tuple(pair: Pair<Rule>) -> DslDiglotTuple {
    let mut inner = pair.into_inner();
    let word_to_replace = inner.next().unwrap().as_str().to_string().replace("__", " ");
    let combined_lemmas_str = inner.next().unwrap().as_str();
    let replacement_lemmas = combined_lemmas_str.split("__").map(|s| s.to_string()).collect();
    let replacement_word = inner.next().unwrap().as_str().to_string().replace("__", " ");
    let mut is_viable = true;
    let mut is_proper_noun = false;
    for flag_pair in inner {
        match flag_pair.as_rule() {
            Rule::viabilityFlag => is_viable = flag_pair.as_str() == "t",
            Rule::properNounFlag => is_proper_noun = true,
            _ => unreachable!(),
        }
    }
    DslDiglotTuple { word_to_replace, replacement_lemmas, replacement_word, is_viable, is_proper_noun }
}

fn parse_string_literal_content(pair: Pair<Rule>) -> String {
    pair.into_inner().next().unwrap().as_str().replace("\\\"", "\"")
}

fn tokenize_simple_string(content: &str) -> Vec<JsonTokenV2> {
    // ... implementation unchanged ...
    let mut tokens = Vec::new();
    let mut last_end = 0;
    let mut word_count = 0;
    let word_re = regex::Regex::new(r"[\w'-]+").unwrap();
    for mat in word_re.find_iter(content) {
        if mat.start() > last_end {
            tokens.push(JsonTokenV2 { token_type: JsonTokenType::Background, value: content[last_end..mat.start()].to_string(), ..Default::default() });
        }
        tokens.push(JsonTokenV2 { token_type: JsonTokenType::Word, value: mat.as_str().to_string(), diglot_index: Some(word_count), ..Default::default() });
        word_count += 1;
        last_end = mat.end();
    }
    if last_end < content.len() {
        tokens.push(JsonTokenV2 { token_type: JsonTokenType::Background, value: content[last_end..].to_string(), ..Default::default() });
    }
    if tokens.is_empty() && !content.is_empty() {
        tokens.push(JsonTokenV2 { token_type: JsonTokenType::Background, value: content.to_string(), ..Default::default() });
    }
    tokens
}

fn tokenize_bracketed_phrase(content: &str) -> Vec<JsonTokenV2> {
    // ... implementation unchanged ...
    let mut tokens = Vec::new();
    let mut last_end = 0;
    let mut diglot_idx_counter = 0;
    let word_re = regex::Regex::new(r"\[\[(.*?)\]\]").unwrap();
    for cap in word_re.captures_iter(content) {
        let full_match = cap.get(0).unwrap();
        let word_value = cap.get(1).unwrap().as_str();
        if full_match.start() > last_end {
            tokens.push(JsonTokenV2 { token_type: JsonTokenType::Background, value: content[last_end..full_match.start()].to_string(), ..Default::default() });
        }
        tokens.push(JsonTokenV2 { token_type: JsonTokenType::Word, value: word_value.to_string(), diglot_index: Some(diglot_idx_counter), ..Default::default() });
        diglot_idx_counter += 1;
        last_end = full_match.end();
    }
    if last_end < content.len() {
        tokens.push(JsonTokenV2 { token_type: JsonTokenType::Background, value: content[last_end..].to_string(), ..Default::default() });
    }
    if tokens.is_empty() && !content.is_empty() {
        return vec![JsonTokenV2 { token_type: JsonTokenType::Background, value: content.to_string(), ..Default::default() }];
    }
    tokens
}