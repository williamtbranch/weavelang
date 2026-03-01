// src/domain/standardize.rs
//
// Port of `llm2books/standardize.py`.
//
// Pure functions for segment reconstruction with separators and smart
// token-boundary editing.
//
// ## Python → Rust mapping
//
// | Rust function                         | Python function                       |
// |---------------------------------------|---------------------------------------|
// | `reconstruct_and_separate_segments`   | `reconstruct_and_separate_segments`   |
// | `smart_match_and_edit`                | `smart_match_and_edit`                |
//
// NOTE: `align_segment_boundaries` was not ported — it had zero callers in
// the production pipeline and exhibited data-lossy segment merging behaviour.

use crate::domain::token_stream::{Token, TokenStream};

// ---------------------------------------------------------------------------
// reconstruct_and_separate_segments
// ---------------------------------------------------------------------------

/// Simplified segment descriptor used by `reconstruct_and_separate_segments`.
///
/// In the Python pipeline, segments are raw dicts with `lookup_id`, `seg_id`,
/// and `text` keys.  In Rust this is a small struct so callers can map back to
/// their own types.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentText {
    /// The key used to look up simplified text (analogous to Python lookup_id).
    pub lookup_id: String,

    /// The segment text (updated by this function).
    pub text: String,
}

/// Takes a list of original segments and a map of simplified texts, then
/// rebuilds the segments with corrected separators and returns the new segment
/// list and the reconstructed full text.
///
/// A space is appended to every non-final segment whose text does not already
/// end with a space.  The full text has its trailing whitespace stripped.
///
/// Python equivalent: `standardize.reconstruct_and_separate_segments`
pub fn reconstruct_and_separate_segments(
    segments: &[SegmentText],
    simplified_text_map: &std::collections::HashMap<String, String>,
) -> (Vec<SegmentText>, String) {
    if segments.is_empty() {
        return (vec![], String::new());
    }

    let num_segments = segments.len();
    let mut new_segments = Vec::with_capacity(num_segments);
    let mut full_text_parts = Vec::with_capacity(num_segments);

    for (i, seg) in segments.iter().enumerate() {
        let clean_text = simplified_text_map
            .get(&seg.lookup_id)
            .cloned()
            .unwrap_or_else(|| seg.text.clone());

        let mut final_text = clean_text;

        // Add separator space for non-final segments that don't already have one.
        if i < num_segments - 1 && !final_text.ends_with(' ') {
            final_text.push(' ');
        }

        full_text_parts.push(final_text.clone());

        new_segments.push(SegmentText {
            lookup_id: seg.lookup_id.clone(),
            text: final_text,
        });
    }

    let full_text = full_text_parts.concat().trim_end().to_string();

    (new_segments, full_text)
}

// ---------------------------------------------------------------------------
// smart_match_and_edit
// ---------------------------------------------------------------------------

/// Attempts to match `match_string` within the local B-W-B neighbourhood of a
/// word token, dynamically adjusting token boundaries by pulling from or
/// pushing to adjacent background tokens.
///
/// The "search space" is the concatenation of the preceding background, the
/// word token, and the following background.  If `match_string` is found as a
/// substring of that search space, the word token's text is replaced with
/// `match_string` and the adjacent backgrounds absorb any leftover characters.
///
/// Returns `Some(new_stream)` on success, or `None` on failure.
///
/// Python equivalent: `standardize.smart_match_and_edit`
pub fn smart_match_and_edit(
    stream: &TokenStream,
    word_token_index: usize,
    match_string: &str,
) -> Option<TokenStream> {
    let tokens = stream.tokens();

    // 1. Basic sanity checks.
    if word_token_index >= tokens.len() {
        return None;
    }
    if !matches!(&tokens[word_token_index], Token::Word(_)) {
        return None;
    }

    // 2. Assemble the B-W-B neighbourhood.
    let b1_text = if word_token_index > 0 {
        match &tokens[word_token_index - 1] {
            Token::Background(s) => s.clone(),
            _ => return None, // Not a valid B-W-B structure.
        }
    } else {
        String::new()
    };

    let w_text = match &tokens[word_token_index] {
        Token::Word(w) => w.text.clone(),
        _ => unreachable!(), // checked above
    };

    let b2_text = if word_token_index + 1 < tokens.len() {
        match &tokens[word_token_index + 1] {
            Token::Background(s) => s.clone(),
            _ => return None, // Not a valid B-W-B structure.
        }
    } else {
        String::new()
    };

    let combined = format!("{b1_text}{w_text}{b2_text}");

    // 3. Find the match.
    let match_start = combined.find(match_string)?;
    let match_end = match_start + match_string.len();

    // 4. Calculate new boundaries.
    let new_b1 = &combined[..match_start];
    let new_w = match_string;
    let new_b2 = &combined[match_end..];

    // 5. Build the new token list.
    let mut new_tokens: Vec<Token> = Vec::with_capacity(tokens.len());

    for (i, token) in tokens.iter().enumerate() {
        if word_token_index > 0 && i == word_token_index - 1 {
            // Replace preceding background.
            new_tokens.push(Token::Background(new_b1.to_string()));
        } else if i == word_token_index {
            // Replace word token — preserve WordData identity.
            if let Token::Word(w) = token {
                let mut new_word = w.clone();
                new_word.text = new_w.to_string();
                new_tokens.push(Token::Word(new_word));
            }
        } else if i == word_token_index + 1 {
            // Replace following background.
            new_tokens.push(Token::Background(new_b2.to_string()));
        } else {
            new_tokens.push(token.clone());
        }
    }

    // Handle edge case: word is the first token (no preceding background).
    if word_token_index == 0 && !new_b1.is_empty() {
        new_tokens.insert(0, Token::Background(new_b1.to_string()));
    }

    // Handle edge case: word is the last token (no following background).
    if word_token_index == tokens.len() - 1 && !new_b2.is_empty() {
        new_tokens.push(Token::Background(new_b2.to_string()));
    }

    Some(TokenStream::from_tokens(new_tokens))
}

// ===========================================================================
// Tests — ported from:
//   llm2books/tests/test_standardize.py
//   llm2books/tests/test_smart_matcher.py
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::primitives::{WordData, WordId};
    use std::collections::HashMap;

    // -- Helpers ----------------------------------------------------------

    fn bg(s: &str) -> Token {
        Token::Background(s.to_string())
    }

    fn word(id: u64, text: &str) -> Token {
        Token::Word(WordData::new(WordId(id), text.to_string(), vec![]))
    }

    /// Build a simple B-W-B stream (mirrors Python's `make_stream`).
    fn make_stream(b1: &str, w: &str, b2: &str) -> TokenStream {
        TokenStream::from_tokens(vec![bg(b1), word(0, w), bg(b2)])
    }

    /// Index of the word token in a `make_stream`-built stream (always 1).
    const WORD_IDX: usize = 1;

    // =====================================================================
    // reconstruct_and_separate_segments
    // Python: test_standardize.py
    // =====================================================================

    /// Port of `test_reconstruct_adds_missing_spaces`.
    #[test]
    fn test_reconstruct_adds_missing_spaces() {
        let segments = vec![
            SegmentText {
                lookup_id: "S1_S1".into(),
                text: "Original one,".into(),
            },
            SegmentText {
                lookup_id: "S1_S2".into(),
                text: "Original two.".into(),
            },
        ];

        let mut map = HashMap::new();
        map.insert("S1_S1".to_string(), "Simplified one,".to_string()); // No trailing space
        map.insert("S1_S2".to_string(), "simplified two.".to_string());

        let (new_segs, full_text) = reconstruct_and_separate_segments(&segments, &map);

        assert_eq!(
            new_segs[0].text, "Simplified one, ",
            "Should add a space to the first segment"
        );
        assert_eq!(
            new_segs[1].text, "simplified two.",
            "Should not add a space to the last segment"
        );
        assert_eq!(full_text, "Simplified one, simplified two.");
    }

    /// Port of `test_reconstruct_preserves_existing_spaces`.
    #[test]
    fn test_reconstruct_preserves_existing_spaces() {
        let segments = vec![
            SegmentText {
                lookup_id: "S1_S1".into(),
                text: "Original one, ".into(),
            },
            SegmentText {
                lookup_id: "S1_S2".into(),
                text: "Original two.".into(),
            },
        ];

        let mut map = HashMap::new();
        map.insert("S1_S1".to_string(), "Simplified one, ".to_string()); // Has trailing space
        map.insert("S1_S2".to_string(), "simplified two.".to_string());

        let (new_segs, full_text) = reconstruct_and_separate_segments(&segments, &map);

        assert_eq!(
            new_segs[0].text, "Simplified one, ",
            "Should not add a second space"
        );
        assert_eq!(full_text, "Simplified one, simplified two.");
    }

    /// Port of `test_reconstruct_handles_single_segment`.
    #[test]
    fn test_reconstruct_handles_single_segment() {
        let segments = vec![SegmentText {
            lookup_id: "S1_S1".into(),
            text: "Original one.".into(),
        }];

        let mut map = HashMap::new();
        map.insert("S1_S1".to_string(), "Simplified one.".to_string());

        let (new_segs, full_text) = reconstruct_and_separate_segments(&segments, &map);

        assert_eq!(
            new_segs[0].text, "Simplified one.",
            "Should not add a space to a single segment"
        );
        assert_eq!(full_text, "Simplified one.");
    }

    #[test]
    fn test_reconstruct_empty_input() {
        let segments: Vec<SegmentText> = vec![];
        let map = HashMap::new();

        let (new_segs, full_text) = reconstruct_and_separate_segments(&segments, &map);

        assert!(new_segs.is_empty());
        assert!(full_text.is_empty());
    }

    // =====================================================================
    // smart_match_and_edit
    // Python: test_smart_matcher.py::TestSmartMatchAndEdit
    // =====================================================================

    /// Helper: extract the B-W-B values from a stream for easy assertion.
    fn bwb_values(stream: &TokenStream) -> (String, String, String) {
        let tokens = stream.tokens();
        assert_eq!(tokens.len(), 3, "Expected B-W-B stream");
        let b1 = match &tokens[0] {
            Token::Background(s) => s.clone(),
            _ => panic!("Expected Background at [0]"),
        };
        let w = match &tokens[1] {
            Token::Word(wd) => wd.text.clone(),
            _ => panic!("Expected Word at [1]"),
        };
        let b2 = match &tokens[2] {
            Token::Background(s) => s.clone(),
            _ => panic!("Expected Background at [2]"),
        };
        (b1, w, b2)
    }

    /// Port of `test_no_match_if_substring_not_found`.
    #[test]
    fn test_smart_match_no_match() {
        let stream = make_stream("ab", "cdef", "gh");
        let result = smart_match_and_edit(&stream, WORD_IDX, "xyz");
        assert!(result.is_none(), "Should fail if match string is not a substring");
    }

    /// Port of `test_perfect_match_succeeds_with_no_change`.
    #[test]
    fn test_smart_match_perfect_match() {
        let stream = make_stream("ab", "cdef", "gh");
        let result = smart_match_and_edit(&stream, WORD_IDX, "cdef").unwrap();
        let (b1, w, b2) = bwb_values(&result);
        assert_eq!((b1.as_str(), w.as_str(), b2.as_str()), ("ab", "cdef", "gh"));
    }

    /// Port of `test_pull_from_left_succeeds`.
    #[test]
    fn test_smart_match_pull_from_left() {
        let stream = make_stream("ab", "cdef", "gh");
        let result = smart_match_and_edit(&stream, WORD_IDX, "bcdef").unwrap();
        let (b1, w, b2) = bwb_values(&result);
        assert_eq!((b1.as_str(), w.as_str(), b2.as_str()), ("a", "bcdef", "gh"));
    }

    /// Port of `test_pull_from_right_succeeds`.
    #[test]
    fn test_smart_match_pull_from_right() {
        let stream = make_stream("ab", "cdef", "gh");
        let result = smart_match_and_edit(&stream, WORD_IDX, "cdefg").unwrap();
        let (b1, w, b2) = bwb_values(&result);
        assert_eq!((b1.as_str(), w.as_str(), b2.as_str()), ("ab", "cdefg", "h"));
    }

    /// Port of `test_pull_from_both_succeeds`.
    #[test]
    fn test_smart_match_pull_from_both() {
        let stream = make_stream("ab", "cdef", "gh");
        let result = smart_match_and_edit(&stream, WORD_IDX, "bcdefg").unwrap();
        let (b1, w, b2) = bwb_values(&result);
        assert_eq!((b1.as_str(), w.as_str(), b2.as_str()), ("a", "bcdefg", "h"));
    }

    /// Port of `test_push_to_both_succeeds`.
    #[test]
    fn test_smart_match_push_to_both() {
        let stream = make_stream("ab", "cdef", "gh");
        let result = smart_match_and_edit(&stream, WORD_IDX, "d").unwrap();
        let (b1, w, b2) = bwb_values(&result);
        assert_eq!((b1.as_str(), w.as_str(), b2.as_str()), ("abc", "d", "efgh"));
    }

    /// Port of `test_push_to_right_succeeds`.
    #[test]
    fn test_smart_match_push_to_right() {
        let stream = make_stream("ab", "cdef", "gh");
        let result = smart_match_and_edit(&stream, WORD_IDX, "cd").unwrap();
        let (b1, w, b2) = bwb_values(&result);
        assert_eq!((b1.as_str(), w.as_str(), b2.as_str()), ("ab", "cd", "efgh"));
    }

    /// Port of `test_push_to_left_succeeds`.
    #[test]
    fn test_smart_match_push_to_left() {
        let stream = make_stream("ab", "cdef", "gh");
        let result = smart_match_and_edit(&stream, WORD_IDX, "ef").unwrap();
        let (b1, w, b2) = bwb_values(&result);
        assert_eq!((b1.as_str(), w.as_str(), b2.as_str()), ("abcd", "ef", "gh"));
    }

    /// Port of `test_fails_if_word_token_not_found`.
    #[test]
    fn test_smart_match_fails_on_non_word_index() {
        // Stream with only a background token.
        let stream = TokenStream::from_tokens(vec![bg("abcdefgh")]);
        let result = smart_match_and_edit(&stream, 1, "cdef");
        assert!(result.is_none(), "Should fail if index is out of bounds");
    }

    /// Port of `test_fails_on_empty_pull`.
    #[test]
    fn test_smart_match_fails_on_empty_pull() {
        // Cannot pull "a" from an empty background.
        let stream = make_stream("", "cdef", "gh");
        let result = smart_match_and_edit(&stream, WORD_IDX, "acdef");
        assert!(
            result.is_none(),
            "Should fail if it requires pulling from an empty background"
        );
    }
}
