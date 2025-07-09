# WeaveLang Generation Test Suite Documentation

This document describes the Domain Specific Language (DSL), testing methodology, and internal validation logic for the `weavelang` core sentence generation test suite.

## 1. Goal

The purpose of this test suite is to provide a highly readable, maintainable, and rigorous "acid test" for the core sentence generation algorithm in `core_algo.rs`. It validates that for a given learner vocabulary level and a given sentence data structure, the engine produces the one, and only one, correct woven sentence.

The suite is composed of two parts:
1.  A DSL file (`generation_tests.weavetest`) that defines test scenarios in a human-readable format.
2.  A Rust test runner (`test_generation.rs`) that parses the DSL, validates the tests for correctness, and executes them against the WeaveLang engine.

## 2. Test DSL Grammar (PEG Format)

The `generation_tests.weavetest` file follows a Lisp-like parenthesized grammar.

```peg
# Top-level structure of the test file
TestSuite       <- Spacing (MaxLemmas / SegmentDef / TestCase)+ Spacing

MaxLemmas       <- '(' '#max-lemmas' Spacing Number Spacing ')'
SegmentDef      <- '(' '#define-segment' Spacing Identifier Spacing SegmentBody Spacing ')'
TestCase        <- '(' '#test-case' Spacing StringLiteral Spacing TestCaseBody Spacing ')'

# Components of a TestCase
TestCaseBody    <- (LevelDef / SentenceDef / AssertLevel / AssertText)+
LevelDef        <- '(' '#level' Spacing Number Spacing ')'
SentenceDef     <- '(' 'sentence' Spacing (AdvSegs / ModSegs / SimpleSegs / EngSpans)+ Spacing ')'
AssertLevel     <- '(' 'assert-level' Spacing ('AdvancedWeave' / 'SimpleHybrid') Spacing ')'
AssertText      <- '(' 'assert-text' Spacing StringLiteral Spacing ')'

# Components of a Sentence
AdvSegs         <- '(' 'adv-segments' Spacing Identifier* Spacing ')'
ModSegs         <- '(' 'ms-segments' Spacing Identifier* Spacing ')'
SimpleSegs      <- '(' 'ss-segments' Spacing Identifier* Spacing ')'
EngSpans        <- '(' 'en-segments' Spacing Identifier* Spacing ')'

# Components of a Segment Body
SegmentBody     <- PartList Spacing (LemmaList)?
PartList        <- '(' (Word / DiglotDef)* Spacing ')'
LemmaList       <- '(' Identifier* Spacing ')'
DiglotDef       <- '(' '#di' Spacing Word Spacing Identifier Spacing Word Spacing ')'

# Basic elements
Identifier      <- [a-zA-Z0-9_-]+
Word            <- [a-zA-Z0-9_-]+
StringLiteral   <- '"' [^"]* '"'
Number          <- [0-9]+
Spacing         <- ([ \t\n\r] / Comment)*
Comment         <- ';' [^\n]*
Use code with caution.
Md
3. Core Concepts
3.1. The Implicit Lemma Universe
The test suite assumes a large, ordered universe of learnable lemmas (e.g., lem1, lem2, ..., lem80000).
The (#max-lemmas N) tag at the top of the file informs the runner of this universe size but is primarily for documentation.
The rank of a lemma (e.g., the 5 in lem5) is its most important property.
3.2. Level-Based Vocabulary
Each test case defines the learner's knowledge with a single (#level N) tag.
This declaration means that for the duration of that test, all lemmas from lem1 to lemN are considered "known" by the hypothetical learner profile. Any lemma with a number greater than N is "unknown."
This system is simple, powerful, and directly models the vocabulary ramp-up logic of the main WeaveLang application.
3.3. Poison Markers (i and x)
The primary goal of the DSL is to ensure that for any test case, there is one and only one "golden path" of segments that can produce a clean output. All other paths must be "poisoned."
Purpose: The markers are a contract between the test author and the test runner. They have no meaning to the WeaveLang engine itself. They are purely for visual inspection and for the test runner's final assertion.
i (Inactive): Used in words within a segment that is valid for the current learner level but should not be chosen because it's part of an incorrect path. Example: msword01i.
x (Unknown): Used in words within a segment that is invalid for the current learner level (i.e., it requires a lemma with a rank higher than the learner's level). Example: adword05x.
The Acid Test: The final assertion in the test runner is that the Actual generated text must not contain the characters i or x. If it does, the WeaveLang engine chose a poisoned path, and the test fails.
4. The Ambiguity Checker Algorithm
Before running any test against the WeaveLang engine, the test runner first validates the test case definition itself to ensure it is unambiguous. It does this by checking that there is exactly one "sanitized" (clean) path through the sentence structure.
Definitions:
A word is sanitized if it contains no i or x characters.
A segment is sanitized if all its constituent words and diglot parts are sanitized.
A path is a complete set of segment choices for a given level (e.g., one choice for each "column").
Algorithm Pseudo-code:
Generated code
function validate_unambiguity(test_case):
  l0_is_sanitized = check_l0_path(test_case)
  l1_is_sanitized = check_l1_path(test_case)

  if l0_is_sanitized == l1_is_sanitized:
    panic("Ambiguity Error: Exactly one of L0 or L1 must be fully sanitized.")

function check_l0_path(test_case):
  // Assumes adv_segments and mod_segments have the same length.
  for i in 0 to test_case.sentence.adv_segments.length:
    adv_seg = test_case.sentence.adv_segments[i]
    mod_seg = test_case.sentence.mod_segments[i]

    // In any given column, exactly one choice must be sanitized.
    if is_segment_sanitized(adv_seg) == is_segment_sanitized(mod_seg):
      return false // The column is ambiguous or has no valid path.

  return true // All columns in L0 have one clear choice.

function check_l1_path(test_case):
  // Assumes ss_segments and en_segments have the same length.
  for i in 0 to test_case.sentence.simple_segments.length:
    ss_seg = test_case.sentence.simple_segments[i]
    en_span = test_case.sentence.english_spans[i]

    is_ss_sanitized = is_segment_sanitized(ss_seg)
    is_en_path_sanitized = true

    // Check the English/Diglot path for this column.
    for part in en_span.parts:
      if part is a Word(w):
        if is_word_unsanitized(w):
          is_en_path_sanitized = false
          break
      if part is a Diglot(word, english_word):
        // For a diglot pair, exactly one must be clean.
        if is_word_sanitized(word) == is_word_sanitized(english_word):
          is_en_path_sanitized = false
          break
    
    // In any given column, exactly one choice must be sanitized.
    if is_ss_sanitized == is_en_path_sanitized:
      return false

  return true
Use code with caution.
This column-based check is efficient and rigorously enforces the "golden path" principle for every test case before it is run.