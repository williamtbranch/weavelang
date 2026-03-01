// src/domain/validation.rs
//
// Port of `llm2books/validator.py`.
//
// Data integrity validators for the WeaveLang domain model.  These functions
// enforce structural invariants on Tiers, TokenStreams, and Sentences.
//
// ## Python → Rust mapping
//
// | Rust function                       | Python function                                |
// |-------------------------------------|------------------------------------------------|
// | `validate_full_text_reconstruction` | `validate_full_text_reconstruction`            |
// | `validate_bwbw_invariant`           | `validate_bwbw_invariant`                      |
// | `validate_word_ids_sequential`      | `validate_base_tier_diglot_indices`            |
// | `validate_exhaustive_mapping`       | `validate_exhaustive_diglot_mapping` + inverse |
// | `validate_segment_reconstruction`   | `validate_segment_reconstruction`              |
// | *(deferred)*                        | `validate_precomputed_word_counts`             |

use crate::domain::mapping::TierMapping;
use crate::domain::sentence::Sentence;
use crate::domain::tier::Tier;
use crate::domain::token_stream::{Token, TokenStream};
use std::collections::HashSet;
use std::fmt;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Custom error type for data integrity failures.
#[derive(Debug, Clone)]
pub struct ValidationError(pub String);

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ValidationError {}

// ---------------------------------------------------------------------------
// Validators
// ---------------------------------------------------------------------------

/// Validates the "Lossless Reconstruction" rule for a single tier.
///
/// The text formed by concatenating all token values from all segments
/// MUST equal the tier's computed `full_text()`.
///
/// **Note:** In the current Rust model `Tier::full_text()` is always computed
/// from segment token data, so this check is tautologically true for
/// properly-constructed types.  It exists as a regression guard and will be
/// critical when importing external JSON where `full_text` is a stored field.
///
/// Python equivalent: `validator.validate_full_text_reconstruction`
pub fn validate_full_text_reconstruction(tier: &Tier) -> Result<(), ValidationError> {
    let full_text = tier.full_text();

    let mut reconstructed = String::new();
    for segment in &tier.segments {
        for token in segment.stream.tokens() {
            match token {
                Token::Background(s) => reconstructed.push_str(s),
                Token::Word(w) => reconstructed.push_str(&w.text),
            }
        }
    }

    if reconstructed != full_text {
        return Err(ValidationError(format!(
            "Lossless reconstruction failed for tier '{}'.\n\
             \x20 Expected: '{}'\n\
             \x20 Got:      '{}'",
            tier.id, full_text, reconstructed
        )));
    }

    Ok(())
}

/// Validates the BWBWB Invariant for a TokenStream.
///
/// The stream must:
/// 1. Not be empty
/// 2. Start with a Background token
/// 3. End with a Background token
/// 4. Alternate strictly between Background and Word tokens
///
/// Python equivalent: `validator.validate_bwbw_invariant`
pub fn validate_bwbw_invariant(
    stream: &TokenStream,
    tier_id: &str,
    seg_id: &str,
) -> Result<(), ValidationError> {
    let tokens = stream.tokens();

    if tokens.is_empty() {
        return Err(ValidationError(format!(
            "Token list for {tier_id}.{seg_id} is empty."
        )));
    }

    // Check start
    if !matches!(tokens.first(), Some(Token::Background(_))) {
        return Err(ValidationError(format!(
            "BWBWB Invariant Violation in {tier_id}.{seg_id}: \
             Token list must start with a background ('b') token."
        )));
    }

    // Check end
    if !matches!(tokens.last(), Some(Token::Background(_))) {
        return Err(ValidationError(format!(
            "BWBWB Invariant Violation in {tier_id}.{seg_id}: \
             Token list must end with a background ('b') token."
        )));
    }

    // Check alternation
    for i in 0..tokens.len() - 1 {
        let curr_is_bg = matches!(&tokens[i], Token::Background(_));
        let next_is_bg = matches!(&tokens[i + 1], Token::Background(_));

        if curr_is_bg == next_is_bg {
            let type_name = if curr_is_bg { "b" } else { "w" };
            return Err(ValidationError(format!(
                "BWBWB Invariant Violation in {tier_id}.{seg_id}: \
                 Found consecutive tokens of the same type ('{type_name}')."
            )));
        }
    }

    Ok(())
}

/// Validates that WordIds across all segments of a tier are unique and
/// sequential starting from 0.
///
/// In the Python pipeline this validates the `di` (diglot index) values on
/// word tokens in the base tier.  The Rust equivalent checks `WordId` values
/// and works on any tier.
///
/// Python equivalent: `validator.validate_base_tier_diglot_indices`
pub fn validate_word_ids_sequential(tier: &Tier) -> Result<(), ValidationError> {
    let mut all_ids: Vec<u64> = Vec::new();

    for segment in &tier.segments {
        for token in segment.stream.tokens() {
            if let Token::Word(w) = token {
                all_ids.push(w.id.0);
            }
        }
    }

    if all_ids.is_empty() {
        return Ok(());
    }

    // Check for duplicates
    let mut seen = HashSet::new();
    for &id in &all_ids {
        if !seen.insert(id) {
            return Err(ValidationError(format!(
                "Validation failed for tier '{}': Duplicate word ID found: {id}.",
                tier.id
            )));
        }
    }

    // Check for sequentiality (0, 1, 2, …)
    let mut sorted = all_ids;
    sorted.sort();
    for (i, &id) in sorted.iter().enumerate() {
        if i as u64 != id {
            return Err(ValidationError(format!(
                "Validation failed for tier '{}': Word ID sequence was not sequential. \
                 Expected {i}, but got {id}.",
                tier.id
            )));
        }
    }

    Ok(())
}

/// Validates that the word count in the source tier equals the number of
/// entries in the given mapping.
///
/// In the Python pipeline, separate functions checked forward and inverse
/// diglot mappings per-segment.  Rust's `TierMapping` is flat (not
/// per-segment), so a single function checks the totals.
///
/// Python equivalent: `validator.validate_exhaustive_diglot_mapping` and
/// `validator.validate_exhaustive_inverse_diglot_mapping`
pub fn validate_exhaustive_mapping(
    sentence: &Sentence,
    mapping: &TierMapping,
) -> Result<(), ValidationError> {
    let source_tier = match sentence.get_tier(&mapping.from_tier_id) {
        Some(t) => t,
        None => return Ok(()), // Can't validate without source tier
    };

    let word_count: usize = source_tier
        .segments
        .iter()
        .map(|seg| seg.stream.word_count())
        .sum();

    let mapping_count = mapping.entries.len();

    if word_count != mapping_count {
        return Err(ValidationError(format!(
            "Exhaustive mapping failed for sentence '{}': \
             Source tier '{}' has {word_count} word(s) but mapping has \
             {mapping_count} entry/entries.",
            sentence.id, mapping.from_tier_id
        )));
    }

    Ok(())
}

/// Validates that the concatenated segment texts reconstruct the tier's
/// `full_text()`.
///
/// **Note:** In the current Rust model, `Tier::full_text()` is computed from
/// segment data, so this is tautologically true.  The validator guards against
/// regressions and will be critical when importing external JSON where
/// segment `text` fields are stored independently from `full_text`.
///
/// Python equivalent: `validator.validate_segment_reconstruction`
pub fn validate_segment_reconstruction(tier: &Tier) -> Result<(), ValidationError> {
    let full_text = tier.full_text();
    let reconstructed: String = tier.segments.iter().map(|s| s.full_text()).collect();

    if reconstructed != full_text {
        return Err(ValidationError(format!(
            "Segment reconstruction failed for tier '{}'.\n\
             \x20 Expected: '{}'\n\
             \x20 Got:      '{}'",
            tier.id, full_text, reconstructed
        )));
    }

    Ok(())
}

// NOTE: `validate_precomputed_word_counts` (from llm2books/validator.py) is
// not yet ported.  The Python version validates that forward-map tuples are
// 6-element and inverse-map tuples are 5-element.  In the Rust domain,
// `MappingEntry` is a typed struct so tuple length is enforced at compile
// time.  This validator will be added when word-count fields (eng_wc, spa_wc)
// are added to MappingEntry.

// ---------------------------------------------------------------------------
// Convenience: validate all invariants for a Tier / Sentence
// ---------------------------------------------------------------------------

/// Runs all tier-level validations: BWBWB invariant per segment, full-text
/// reconstruction, segment reconstruction, and sequential word IDs.
pub fn validate_tier(tier: &Tier) -> Result<(), ValidationError> {
    validate_full_text_reconstruction(tier)?;
    validate_segment_reconstruction(tier)?;
    validate_word_ids_sequential(tier)?;

    for segment in &tier.segments {
        validate_bwbw_invariant(&segment.stream, &tier.id, &segment.id)?;
    }

    Ok(())
}

/// Runs all sentence-level validations: tier validation for each tier, plus
/// exhaustive mapping checks for each mapping.
pub fn validate_sentence(sentence: &Sentence) -> Result<(), ValidationError> {
    for tier in sentence.tiers.values() {
        validate_tier(tier)?;
    }

    for mapping in &sentence.mappings {
        validate_exhaustive_mapping(sentence, mapping)?;
    }

    Ok(())
}

// ===========================================================================
// Tests — ported from llm2books/tests/test_validator.py
//                  and llm2books/tests/test_final_schema.py
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mapping::{MappingEntry, TierMapping};
    use crate::domain::primitives::{WordData, WordId};
    use crate::domain::segment::Segment;
    use crate::domain::sentence::Sentence;
    use crate::domain::tier::Tier;
    use crate::domain::token_stream::{Token, TokenStream};

    // -- Helpers ----------------------------------------------------------

    fn bg(s: &str) -> Token {
        Token::Background(s.to_string())
    }

    fn word(id: u64, text: &str) -> Token {
        Token::Word(WordData::new(WordId(id), text.to_string(), vec![]))
    }

    fn tier_one_seg(tier_id: &str, seg_id: &str, tokens: Vec<Token>) -> Tier {
        let stream = TokenStream::from_tokens(tokens);
        let seg = Segment::from_stream(seg_id.to_string(), stream, vec![]);
        let mut tier = Tier::new(tier_id.to_string());
        tier.segments.push(seg);
        tier
    }

    fn make_mapping_entry(source_word_id: u64) -> MappingEntry {
        MappingEntry::new(
            WordId(source_word_id),
            "target".to_string(),
            vec!["lemma".to_string()],
        )
    }

    // =====================================================================
    // validate_full_text_reconstruction
    // Python: test_reconstruction_happy_path
    // =====================================================================

    /// Port of `test_reconstruction_happy_path`.
    #[test]
    fn test_full_text_reconstruction_valid() {
        // \u{201c}Who are you?\u{201d} as properly tokenized stream
        let tier = tier_one_seg(
            "base",
            "S1",
            vec![
                bg("\u{201c}"),
                word(0, "Who"),
                bg(" "),
                word(1, "are"),
                bg(" "),
                word(2, "you"),
                bg("?\u{201d}"),
            ],
        );
        assert!(validate_full_text_reconstruction(&tier).is_ok());
    }

    // NOTE: Python's `test_reconstruction_fails_on_mismatch` cannot be ported
    // directly because Rust's Tier.full_text() is always computed from tokens
    // — there is no stored full_text field to put out of sync.  This will
    // gain fail-case tests when a stored full_text is added for JSON import.

    // =====================================================================
    // validate_bwbw_invariant
    // Python: test_bwbw_happy_path, test_bwbw_fails_on_*
    // =====================================================================

    /// Port of `test_bwbw_happy_path` — single background [B].
    #[test]
    fn test_bwbw_valid_single_bg() {
        let stream = TokenStream::from_tokens(vec![bg("")]);
        assert!(validate_bwbw_invariant(&stream, "t", "S1").is_ok());
    }

    /// Port of `test_bwbw_happy_path` — [B, W, B].
    #[test]
    fn test_bwbw_valid_bwb() {
        let stream = TokenStream::from_tokens(vec![bg(" "), word(0, "word"), bg(".")]);
        assert!(validate_bwbw_invariant(&stream, "t", "S1").is_ok());
    }

    /// Port of `test_bwbw_happy_path` — [B, W, B, W, B].
    #[test]
    fn test_bwbw_valid_bwbwb() {
        let stream = TokenStream::from_tokens(vec![
            bg(""),
            word(0, "a"),
            bg(" "),
            word(1, "b"),
            bg(""),
        ]);
        assert!(validate_bwbw_invariant(&stream, "t", "S1").is_ok());
    }

    /// Port of `test_bwbw_fails_on_starts_with_word`.
    #[test]
    fn test_bwbw_fails_starts_with_word() {
        let stream = TokenStream::from_tokens(vec![word(0, "word"), bg(" ")]);
        let err = validate_bwbw_invariant(&stream, "test_tier", "S1").unwrap_err();
        assert!(
            err.0.contains("must start with a background ('b') token"),
            "Unexpected error: {err}"
        );
    }

    /// Port of `test_bwbw_fails_on_ends_with_word`.
    #[test]
    fn test_bwbw_fails_ends_with_word() {
        let stream = TokenStream::from_tokens(vec![bg(" "), word(0, "word")]);
        let err = validate_bwbw_invariant(&stream, "test_tier", "S1").unwrap_err();
        assert!(
            err.0.contains("must end with a background ('b') token"),
            "Unexpected error: {err}"
        );
    }

    /// Port of `test_bwbw_fails_on_consecutive_words`.
    #[test]
    fn test_bwbw_fails_consecutive_words() {
        let stream = TokenStream::from_tokens(vec![
            bg(""),
            word(0, "a"),
            word(1, "b"),
            bg(""),
        ]);
        let err = validate_bwbw_invariant(&stream, "test_tier", "S1").unwrap_err();
        assert!(
            err.0.contains("consecutive tokens of the same type ('w')"),
            "Unexpected error: {err}"
        );
    }

    /// Port of `test_bwbw_fails_on_consecutive_backgrounds`.
    #[test]
    fn test_bwbw_fails_consecutive_backgrounds() {
        let stream = TokenStream::from_tokens(vec![
            bg(" "),
            bg("."),
            word(0, "a"),
            bg(""),
        ]);
        let err = validate_bwbw_invariant(&stream, "test_tier", "S1").unwrap_err();
        assert!(
            err.0.contains("consecutive tokens of the same type ('b')"),
            "Unexpected error: {err}"
        );
    }

    // =====================================================================
    // validate_word_ids_sequential
    // Python: test_di_happy_path, test_di_fails_on_*
    // =====================================================================

    /// Port of `test_di_happy_path`.
    #[test]
    fn test_word_ids_sequential_happy_path() {
        // Two segments: IDs 0,1 in first; 2 in second
        let seg1 = Segment::from_stream(
            "S1".to_string(),
            TokenStream::from_tokens(vec![
                bg(""),
                word(0, "a"),
                bg(" "),
                word(1, "b"),
                bg(""),
            ]),
            vec![],
        );
        let seg2 = Segment::from_stream(
            "S2".to_string(),
            TokenStream::from_tokens(vec![bg(""), word(2, "c"), bg("")]),
            vec![],
        );
        let mut tier = Tier::new("base".to_string());
        tier.segments.push(seg1);
        tier.segments.push(seg2);

        assert!(validate_word_ids_sequential(&tier).is_ok());
    }

    // NOTE: Python's `test_di_fails_on_missing_di_key` has no Rust equivalent
    // — Token::Word always contains a WordData with an id field.

    /// Port of `test_di_fails_on_non_sequential_di`.
    #[test]
    fn test_word_ids_sequential_fails_on_gap() {
        // IDs 0 and 2 (skips 1)
        let tier = tier_one_seg(
            "base",
            "S1",
            vec![
                bg(""),
                word(0, "a"),
                bg(" "),
                word(2, "b"), // gap: skips 1
                bg(""),
            ],
        );
        let err = validate_word_ids_sequential(&tier).unwrap_err();
        assert!(err.0.contains("was not sequential"), "Unexpected error: {err}");
        assert!(err.0.contains("Expected 1, but got 2"), "Unexpected error: {err}");
    }

    /// Port of `test_di_fails_on_duplicate_di`.
    #[test]
    fn test_word_ids_sequential_fails_on_duplicate() {
        // Two segments, both with ID 0
        let seg1 = Segment::from_stream(
            "S1".to_string(),
            TokenStream::from_tokens(vec![bg(""), word(0, "a"), bg("")]),
            vec![],
        );
        let seg2 = Segment::from_stream(
            "S2".to_string(),
            TokenStream::from_tokens(vec![bg(""), word(0, "b"), bg("")]),
            vec![],
        );
        let mut tier = Tier::new("base".to_string());
        tier.segments.push(seg1);
        tier.segments.push(seg2);

        let err = validate_word_ids_sequential(&tier).unwrap_err();
        assert!(
            err.0.contains("Duplicate word ID found: 0"),
            "Unexpected error: {err}"
        );
    }

    // =====================================================================
    // validate_exhaustive_mapping
    // Python: test_diglot_mapping_happy_path, test_diglot_mapping_fails_*,
    //         test_inverse_diglot_mapping_*
    // =====================================================================

    /// Port of `test_diglot_mapping_happy_path`.
    #[test]
    fn test_exhaustive_mapping_happy_path() {
        let mut sentence = Sentence::new("S1".into());
        sentence.add_tier(tier_one_seg(
            "basic_base",
            "S1",
            vec![bg(""), word(0, "word1"), bg(" "), word(1, "word2"), bg("")],
        ));

        let mut mapping = TierMapping::new("basic_base".into(), "basic_target".into());
        mapping.add_entry(make_mapping_entry(0));
        mapping.add_entry(make_mapping_entry(1));

        assert!(validate_exhaustive_mapping(&sentence, &mapping).is_ok());
    }

    /// Port of `test_diglot_mapping_fails_on_mismatched_counts`.
    #[test]
    fn test_exhaustive_mapping_fails_on_mismatch() {
        let mut sentence = Sentence::new("S1".into());
        sentence.add_tier(tier_one_seg(
            "basic_base",
            "S1",
            vec![bg(""), word(0, "word1"), bg("")], // 1 word
        ));

        let mut mapping = TierMapping::new("basic_base".into(), "basic_target".into());
        mapping.add_entry(make_mapping_entry(0));
        mapping.add_entry(make_mapping_entry(1)); // 2 entries

        let err = validate_exhaustive_mapping(&sentence, &mapping).unwrap_err();
        assert!(
            err.0.contains("1 word(s) but mapping has 2 entry/entries"),
            "Unexpected error: {err}"
        );
    }

    /// Port of `test_diglot_mapping_handles_missing_map_gracefully`.
    /// In Rust, if the source tier doesn't exist we return Ok.
    #[test]
    fn test_exhaustive_mapping_missing_source_tier() {
        let sentence = Sentence::new("S1".into());
        let mapping = TierMapping::new("nonexistent".into(), "basic_target".into());
        assert!(validate_exhaustive_mapping(&sentence, &mapping).is_ok());
    }

    /// Port of `test_inverse_diglot_mapping_happy_path`.
    /// In Rust, forward and inverse use the same `validate_exhaustive_mapping`.
    #[test]
    fn test_exhaustive_inverse_mapping_happy_path() {
        let mut sentence = Sentence::new("S1".into());
        sentence.add_tier(tier_one_seg(
            "basic_target",
            "A1",
            vec![bg(""), word(0, "word1"), bg(" "), word(1, "word2"), bg("")],
        ));

        let mut mapping = TierMapping::new("basic_target".into(), "basic_base".into());
        mapping.add_entry(make_mapping_entry(0));
        mapping.add_entry(make_mapping_entry(1));

        assert!(validate_exhaustive_mapping(&sentence, &mapping).is_ok());
    }

    /// Port of `test_inverse_diglot_mapping_fails_on_mismatch`.
    #[test]
    fn test_exhaustive_inverse_mapping_fails_on_mismatch() {
        let mut sentence = Sentence::new("S1".into());
        sentence.add_tier(tier_one_seg(
            "basic_target",
            "A1",
            vec![bg(""), word(0, "word1"), bg("")], // 1 word
        ));

        let mut mapping = TierMapping::new("basic_target".into(), "basic_base".into());
        mapping.add_entry(make_mapping_entry(0));
        mapping.add_entry(make_mapping_entry(1)); // 2 entries

        let err = validate_exhaustive_mapping(&sentence, &mapping).unwrap_err();
        assert!(
            err.0.contains("1 word(s) but mapping has 2 entry/entries"),
            "Unexpected error: {err}"
        );
    }

    // =====================================================================
    // validate_segment_reconstruction
    // Python: test_segment_reconstruction_happy_path,
    //         test_segment_reconstruction_fails_on_mushed_words
    // =====================================================================

    /// Port of `test_segment_reconstruction_happy_path`.
    #[test]
    fn test_segment_reconstruction_happy_path() {
        // Two segments: "hijo " + "fue."
        let seg1 = Segment::from_stream(
            "S1".to_string(),
            TokenStream::from_tokens(vec![bg(""), word(0, "hijo"), bg(" ")]),
            vec![],
        );
        let seg2 = Segment::from_stream(
            "S2".to_string(),
            TokenStream::from_tokens(vec![bg(""), word(1, "fue"), bg(".")]),
            vec![],
        );
        let mut tier = Tier::new("test".to_string());
        tier.segments.push(seg1);
        tier.segments.push(seg2);

        assert!(validate_segment_reconstruction(&tier).is_ok());
    }

    // NOTE: Python's `test_segment_reconstruction_fails_on_mushed_words`
    // cannot be ported directly.  In the Python pipeline, segments store a
    // "text" field independently from "full_text", so they can diverge.  In
    // Rust, both are computed from tokens, making mismatches structurally
    // impossible.

    // =====================================================================
    // validate_tier / validate_sentence (convenience combinators)
    // =====================================================================

    #[test]
    fn test_validate_tier_catches_bwbw_violation() {
        let mut tier = Tier::new("base".to_string());
        tier.segments.push(Segment::from_stream(
            "S1".to_string(),
            TokenStream::from_tokens(vec![word(0, "bad"), bg("")]),
            vec![],
        ));
        let err = validate_tier(&tier).unwrap_err();
        assert!(err.0.contains("BWBWB Invariant Violation"));
    }

    #[test]
    fn test_validate_tier_catches_duplicate_ids() {
        let seg1 = Segment::from_stream(
            "S1".to_string(),
            TokenStream::from_tokens(vec![bg(""), word(0, "a"), bg("")]),
            vec![],
        );
        let seg2 = Segment::from_stream(
            "S2".to_string(),
            TokenStream::from_tokens(vec![bg(""), word(0, "b"), bg("")]),
            vec![],
        );
        let mut tier = Tier::new("base".to_string());
        tier.segments.push(seg1);
        tier.segments.push(seg2);

        let err = validate_tier(&tier).unwrap_err();
        assert!(err.0.contains("Duplicate word ID"));
    }

    #[test]
    fn test_validate_sentence_catches_mapping_mismatch() {
        let mut sentence = Sentence::new("S1".into());
        sentence.add_tier(tier_one_seg(
            "basic_base",
            "S1",
            vec![bg(""), word(0, "only_one"), bg("")],
        ));

        let mut mapping = TierMapping::new("basic_base".into(), "basic_target".into());
        mapping.add_entry(make_mapping_entry(0));
        mapping.add_entry(make_mapping_entry(1)); // too many
        sentence.add_mapping(mapping);

        let err = validate_sentence(&sentence).unwrap_err();
        assert!(err.0.contains("Exhaustive mapping failed"));
    }

    #[test]
    fn test_validate_sentence_passes_valid() {
        let mut sentence = Sentence::new("S1".into());
        sentence.add_tier(tier_one_seg(
            "basic_base",
            "S1",
            vec![bg(""), word(0, "hello"), bg(" "), word(1, "world"), bg("")],
        ));

        let mut mapping = TierMapping::new("basic_base".into(), "basic_target".into());
        mapping.add_entry(make_mapping_entry(0));
        mapping.add_entry(make_mapping_entry(1));
        sentence.add_mapping(mapping);

        assert!(validate_sentence(&sentence).is_ok());
    }

    // =====================================================================
    // validate_precomputed_word_counts — DEFERRED
    // =====================================================================
    // Python equivalent: llm2books/tests/test_final_schema.py
    //
    // Not yet ported because Rust's MappingEntry struct does not include
    // word-count fields (eng_wc, spa_wc).  The Python validator checks
    // tuple lengths (6-tuple for forward, 5-tuple for inverse); in Rust,
    // struct field presence is enforced at compile time.
    //
    // Tests to port when MappingEntry gains word-count fields:
    //   - test_validator_passes_on_correct_word_counts
    //   - test_validator_fails_on_missing_forward_map_count
    //   - test_validator_fails_on_missing_inverse_map_count
}
