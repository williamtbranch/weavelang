use pest::{iterators::Pair, Parser};
use crate::types::json_types::{JsonTokenV2, JsonTokenType};

#[derive(Debug, Clone)]
pub struct DslTestCase {
    pub name: String,
    pub sentence_def: DslSentenceDef,
    pub sub_tests: Vec<DslSubTest>,
}

#[derive(Debug, Clone)]
pub struct DslSubTest {
    pub name: String,
    pub learner_level: u32,
    pub assertions: Vec<DslAssertion>,
}

#[derive(Debug, Clone)]
pub struct DslSentenceDef {
    pub l0_def: DslColumnBody,
    pub l1_def: DslColumnBody,
}

#[derive(Debug, Clone)]
pub struct DslColumnBody {
    pub segments: Vec<DslSegmentSpec>,
}

#[derive(Debug, Clone)]
pub struct DslSegmentSpec {
    pub spec: DslSegmentSpecEnum,
}

#[derive(Debug, Clone)]
pub enum DslSegmentSpecEnum {
    Spanish { tokens: Vec<JsonTokenV2>, lemmas: Vec<String> },
    Diglot { tuples: Vec<DslDiglotTuple> },
    English { tokens: Vec<JsonTokenV2> },
    InvDiglot { tuples: Vec<DslInvDiglotTuple> },
}

#[derive(Debug, Clone)]
pub struct DslDiglotTuple {
    pub _word_to_replace: String,
    pub replacement_lemmas: Vec<String>,
    pub replacement_word: String,
    pub is_viable: bool,
}

#[derive(Debug, Clone)]
pub struct DslInvDiglotTuple {
    pub _spanish_word: String,
    pub spanish_lemmas: Vec<String>,
    pub english_substitute: String,
}

#[derive(Debug, Clone)]
pub enum DslAssertion {
    Level(String),
    Text(String),
}


#[derive(pest_derive::Parser)]
#[grammar = "simulation/tests/generation_tests.pest"]
pub struct WeaveTestParser;

pub fn parse_weavetest_file(file_content: &str) -> Result<Vec<DslTestCase>, pest::error::Error<Rule>> {
    let file_pair = WeaveTestParser::parse(Rule::test_suite, file_content)?.next().unwrap();
    Ok(file_pair.into_inner().filter(|p| p.as_rule() == Rule::test_case).map(parse_test_case).collect())
}

fn parse_string_literal_content(pair: Pair<Rule>) -> String {
    let inner = pair.into_inner().next().unwrap();
    let mut result = String::new();
    let mut chars = inner.as_str().chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next_c) = chars.next() {
                match next_c {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    _ => {
                        result.push(c);
                        result.push(next_c);
                    }
                }
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}


fn parse_test_case(pair: Pair<Rule>) -> DslTestCase {
    let mut inner = pair.into_inner();
    let name = parse_string_literal_content(inner.next().unwrap());
    let body_pair = inner.next().unwrap();
    let mut sentence_def_opt = None;
    let mut sub_tests = Vec::new();
    for part in body_pair.into_inner() {
        match part.as_rule() {
            Rule::sentence_def => sentence_def_opt = Some(parse_sentence_def(part)),
            Rule::sub_test => sub_tests.push(parse_sub_test(part)),
            _ => (), 
        }
    }
    DslTestCase { name, sentence_def: sentence_def_opt.unwrap(), sub_tests }
}

fn parse_sub_test(pair: Pair<Rule>) -> DslSubTest {
    let mut inner = pair.into_inner();
    let name = parse_string_literal_content(inner.next().unwrap());
    let body_pair = inner.next().unwrap();
    let mut learner_level = 0;
    let mut assertions = Vec::new();
    for part in body_pair.into_inner() {
        match part.as_rule() {
            Rule::learner_level => learner_level = part.into_inner().next().unwrap().as_str().parse().unwrap(),
            Rule::assertion => assertions.push(parse_assertion(part)),
            _ => (),
        }
    }
    DslSubTest { name, learner_level, assertions }
}

fn parse_sentence_def(pair: Pair<Rule>) -> DslSentenceDef {
    let mut inner = pair.into_inner();
    let l0_def = parse_column_body(inner.next().unwrap());
    let l1_def = parse_column_body(inner.next().unwrap());
    DslSentenceDef { l0_def, l1_def }
}

fn parse_column_body(pair: Pair<Rule>) -> DslColumnBody {
    let segments: Vec<DslSegmentSpec> = pair.into_inner().map(parse_segment_spec).collect();
    DslColumnBody { segments }
}

fn tokenize_simple_string(content: &str) -> Vec<JsonTokenV2> {
    let mut tokens = Vec::new();
    let mut last_end = 0;
    let mut word_count = 0;
    
    let word_re = regex::Regex::new(r"[\w-]+").unwrap();

    for mat in word_re.find_iter(content) {
        if mat.start() > last_end {
            tokens.push(JsonTokenV2 {
                token_type: JsonTokenType::Background,
                value: content[last_end..mat.start()].to_string(),
                ..Default::default()
            });
        }
        tokens.push(JsonTokenV2 {
            token_type: JsonTokenType::Word,
            value: mat.as_str().to_string(),
            diglot_index: Some(word_count),
            ..Default::default()
        });
        word_count += 1;
        last_end = mat.end();
    }

    if last_end < content.len() {
        tokens.push(JsonTokenV2 {
            token_type: JsonTokenType::Background,
            value: content[last_end..].to_string(),
            ..Default::default()
        });
    }

    if tokens.is_empty() && !content.is_empty() {
        tokens.push(JsonTokenV2 { token_type: JsonTokenType::Background, value: content.to_string(), ..Default::default() });
    }
    
    tokens
}

fn tokenize_bracketed_phrase(content: &str) -> Vec<JsonTokenV2> {
    let mut tokens = Vec::new();
    let mut last_end = 0;
    let mut diglot_idx_counter = 0;
    
    let word_re = regex::Regex::new(r"\[\[(.*?)\]\]").unwrap();

    for cap in word_re.captures_iter(content) {
        let full_match = cap.get(0).unwrap();
        let word_value = cap.get(1).unwrap().as_str();

        if full_match.start() > last_end {
            tokens.push(JsonTokenV2 {
                token_type: JsonTokenType::Background,
                value: content[last_end..full_match.start()].to_string(),
                ..Default::default()
            });
        }
        
        tokens.push(JsonTokenV2 {
            token_type: JsonTokenType::Word,
            value: word_value.to_string(),
            diglot_index: Some(diglot_idx_counter),
            ..Default::default()
        });
        diglot_idx_counter += 1;
        
        last_end = full_match.end();
    }

    if last_end < content.len() {
        tokens.push(JsonTokenV2 {
            token_type: JsonTokenType::Background,
            value: content[last_end..].to_string(),
            ..Default::default()
        });
    }

    if tokens.is_empty() && !content.is_empty() {
        return vec![JsonTokenV2 { token_type: JsonTokenType::Background, value: content.to_string(), ..Default::default() }];
    }
    
    tokens
}

fn parse_segment_spec(pair: Pair<Rule>) -> DslSegmentSpec {
    let inner_pair = pair.into_inner().next().unwrap();
    let rule = inner_pair.as_rule();
    let mut inner = inner_pair.into_inner();

    let spec = match rule {
        Rule::spanishSegment | Rule::englishSegment => {
            let tokens = if let Some(phrase_pair) = inner.find(|p| p.as_rule() == Rule::phrase) {
                let content_pair = phrase_pair.into_inner().next().unwrap();
                let content_rule = content_pair.as_rule();
                let content = parse_string_literal_content(content_pair);
                
                match content_rule {
                    Rule::tokenizedPhrase => tokenize_bracketed_phrase(&content),
                    Rule::stringLiteral => tokenize_simple_string(&content),
                    _ => unreachable!(),
                }
            } else {
                vec![]
            };

            let lemmas = inner
                .find(|p| p.as_rule() == Rule::lemmaList)
                .map(|p| p.into_inner().map(|l| l.as_str().to_string()).collect())
                .unwrap_or_default();
            
            if rule == Rule::spanishSegment {
                DslSegmentSpecEnum::Spanish { tokens, lemmas }
            } else {
                DslSegmentSpecEnum::English { tokens }
            }
        }
        Rule::diglotSegment => DslSegmentSpecEnum::Diglot { tuples: inner.map(parse_diglot_tuple).collect() },
        Rule::invDiglotSegment => DslSegmentSpecEnum::InvDiglot { tuples: inner.map(parse_inv_diglot_tuple).collect() },
        _ => unreachable!(),
    };
    DslSegmentSpec { spec }
}

fn parse_diglot_tuple(pair: Pair<Rule>) -> DslDiglotTuple {
    let mut inner = pair.into_inner();
    let word_to_replace = inner.next().unwrap().as_str().to_string()
        .replace("__", " "); // Also un-escape the source phrase
    
    let combined_lemmas_str = inner.next().unwrap().as_str();
    let replacement_lemmas = combined_lemmas_str
        .split("__")
        .map(|s| s.to_string())
        .collect();

    let replacement_word = inner.next().unwrap().as_str().to_string()
        .replace("__", " "); // Un-escape the target phrase

    let is_viable = inner.next().map_or(true, |p| p.as_str() == "t");

    DslDiglotTuple {
        _word_to_replace: word_to_replace,
        replacement_lemmas,
        replacement_word,
        is_viable,
    }
}

fn parse_inv_diglot_tuple(pair: Pair<Rule>) -> DslInvDiglotTuple {
    let mut inner = pair.into_inner();
    let spanish_word = inner.next().unwrap().as_str().to_string().replace("__", " ");
    
    // --- THIS IS THE FIX ---
    // Split the combined lemma string into a vector of individual lemma strings.
    let combined_lemmas_str = inner.next().unwrap().as_str();
    let spanish_lemmas = combined_lemmas_str
        .split("__")
        .map(|s| s.to_string())
        .collect();
    // --- END OF FIX ---

    let english_substitute = inner.next().unwrap().as_str().to_string().replace("__", " ");

    DslInvDiglotTuple {
        _spanish_word: spanish_word,
        spanish_lemmas,
        english_substitute,
    }
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