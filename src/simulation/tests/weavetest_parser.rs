
use pest::Parser;

// --- DSL DATA STRUCTURES ---
#[derive(Debug, Clone)]
pub struct DslTestCase {
    pub name: String,
    pub sentence_def: DslSentenceDef,
    pub sub_tests: Vec<DslSubTest>, // A test case now has multiple sub-tests
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
pub enum DslColumnBody {
    L0(Vec<DslSegmentSpec>),
    L1(Vec<DslSegmentSpec>),
}

#[derive(Debug, Clone)]
pub enum DslSegmentSpec {
    Spanish { phrase: String, lemmas: Vec<String> },
    Diglot { tuples: Vec<DslDiglotTuple> },
    English { phrase: String },
    InvDiglot { tuples: Vec<DslInvDiglotTuple> },
}

#[derive(Debug, Clone)]
pub struct DslDiglotTuple {
    pub word_to_replace: String,
    pub replacement_lemma: String,
    pub replacement_word: String,
    pub is_viable: bool,
}

// *** REVERTED: This struct is now back to its 3-token form for consistency. ***
#[derive(Debug, Clone)]
pub struct DslInvDiglotTuple {
    pub spanish_word: String,
    pub spanish_lemma: String,
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

// --- PARSING LOGIC ---

pub fn parse_weavetest_file(file_content: &str) -> Result<Vec<DslTestCase>, pest::error::Error<Rule>> {
    let file_pair = WeaveTestParser::parse(Rule::test_suite, file_content)?.next().unwrap();
    Ok(file_pair.into_inner()
        .filter(|p| p.as_rule() == Rule::test_case)
        .map(parse_test_case)
        .collect())
}

fn parse_test_case(pair: pest::iterators::Pair<Rule>) -> DslTestCase {
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().trim_matches('"').to_string();
    let body_pair = inner.next().unwrap();

    let mut sentence_def_opt = None;
    let mut sub_tests = Vec::new();

    for part in body_pair.into_inner() {
        match part.as_rule() {
            Rule::sentence_def => {
                sentence_def_opt = Some(parse_sentence_def(part));
            }
            Rule::sub_test => {
                sub_tests.push(parse_sub_test(part));
            }
            _ => unreachable!(),
        }
    }

    DslTestCase { name, sentence_def: sentence_def_opt.unwrap(), sub_tests }
}

fn parse_sub_test(pair: pest::iterators::Pair<Rule>) -> DslSubTest {
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().trim_matches('"').to_string();
    let body_pair = inner.next().unwrap();

    let mut learner_level = 0;
    let mut assertions = Vec::new();

    for part in body_pair.into_inner() {
        match part.as_rule() {
            Rule::learner_level => {
                learner_level = part.into_inner().next().unwrap().as_str().parse().unwrap();
            }
            Rule::assertion => {
                assertions.push(parse_assertion(part));
            }
            _ => unreachable!(),
        }
    }
    DslSubTest { name, learner_level, assertions }
}

fn parse_sentence_def(pair: pest::iterators::Pair<Rule>) -> DslSentenceDef {
    let mut inner = pair.into_inner();
    let l0_def = parse_column_body(inner.next().unwrap());
    let l1_def = parse_column_body(inner.next().unwrap());
    DslSentenceDef { l0_def, l1_def }
}

fn parse_column_body(pair: pest::iterators::Pair<Rule>) -> DslColumnBody {
    let rule = pair.as_rule();
    let segments: Vec<DslSegmentSpec> = pair.into_inner().map(parse_segment_spec).collect();
    match rule {
        Rule::l0_column_body => DslColumnBody::L0(segments),
        Rule::l1_column_body => DslColumnBody::L1(segments),
        _ => unreachable!(),
    }
}

fn parse_segment_spec(pair: pest::iterators::Pair<Rule>) -> DslSegmentSpec {
    let inner_pair = pair.into_inner().next().unwrap();
    let rule = inner_pair.as_rule();
    let mut inner = inner_pair.into_inner();

    match rule {
        Rule::spanishSegment => {
            let phrase_with_quotes = inner.next().unwrap().as_str();
            let phrase_str = phrase_with_quotes.trim_matches('"').to_string();

            let lemmas = inner.next().map(|p| {
                p.into_inner().map(|l| l.as_str().to_string()).collect()
            }).unwrap_or_default();

            DslSegmentSpec::Spanish {
                phrase: phrase_str, // Use the full string
                lemmas,
            }
        }
        Rule::englishSegment => {
            let phrase_with_quotes = inner.next().unwrap().as_str();
            let phrase_str = phrase_with_quotes.trim_matches('"').to_string();

            DslSegmentSpec::English {
                phrase: phrase_str // Use the full string
            }
        }
        Rule::diglotSegment => {
            DslSegmentSpec::Diglot { tuples: inner.map(parse_diglot_tuple).collect() }
        }
        Rule::invDiglotSegment => {
            DslSegmentSpec::InvDiglot { tuples: inner.map(parse_inv_diglot_tuple).collect() }
        }
        _ => unreachable!("BUG in parse_segment_spec: expected a segment type, got {:?}", rule),
    }
}

fn parse_diglot_tuple(pair: pest::iterators::Pair<Rule>) -> DslDiglotTuple {
    let mut inner = pair.into_inner();
    DslDiglotTuple {
        word_to_replace: inner.next().unwrap().as_str().to_string(),
        replacement_lemma: inner.next().unwrap().as_str().to_string(),
        replacement_word: inner.next().unwrap().as_str().to_string(),
        is_viable: inner.next().map_or(true, |p| p.as_str() == "t"),
    }
}

// *** REVERTED: This function now correctly parses the 3-token tuple. ***
fn parse_inv_diglot_tuple(pair: pest::iterators::Pair<Rule>) -> DslInvDiglotTuple {
    let mut inner = pair.into_inner();
    DslInvDiglotTuple {
        spanish_word: inner.next().unwrap().as_str().to_string(),
        spanish_lemma: inner.next().unwrap().as_str().to_string(),
        english_substitute: inner.next().unwrap().as_str().to_string(),
    }
}

fn parse_assertion(pair: pest::iterators::Pair<Rule>) -> DslAssertion {
    let assertion_pair = pair.into_inner().next().unwrap();
    let rule = assertion_pair.as_rule();
    let value_pair = assertion_pair.into_inner().next().unwrap();
    let value = value_pair.as_str();

    match rule {
        Rule::assert_level => DslAssertion::Level(value.to_string()),
        Rule::assert_text => DslAssertion::Text(value.trim_matches('"').to_string()),
        _ => unreachable!(),
    }
}