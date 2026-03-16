// src/services/llm_segmenter.rs
//
// Port of llm2books/stanza_segmenter.py::LLMSegmenter
//
// This module handles LLM-based semantic segmentation of sentences
// into smaller meaningful phrases. The prompt uses the standard Sn:-label
// format shared with all other LLM stages for consistency and testability.

use regex::Regex;
use std::collections::HashSet;

use crate::domain::segmentation::merge_short_segments;
use crate::services::llm_client::LlmService;
use crate::services::llm_logger::LlmLogger;
use crate::services::prompt_manager::PromptManager;

/// Minimum number of real words a segment must have before the merge pass
/// considers it "too short" and merges it with a neighbor.
const MIN_SEGMENT_WORDS: usize = 5;

/// Opening punctuation characters that influence where segment boundaries fall.
const OPENING_PUNCT: &[char] = &['"', '\u{201C}', '\u{2018}', '(', '[', '{', '\u{00A1}', '\u{00BF}'];

// ─── Public API ───────────────────────────────────────────────────────────────

/// Segments a single sentence (or short passage) into semantically meaningful
/// phrases using an LLM, then merges any that are too short.
///
/// This is the Rust equivalent of `LLMSegmenter.segment_sentence()`.
///
/// Returns `Ok(vec![])` if the input is empty.
/// Returns the text as a single-element vec if the LLM produces only one segment.
pub fn segment_sentence(
    sentence_text: &str,
    s_id: &str,
    llm: &LlmService,
    prompts: &PromptManager,
    logger: &LlmLogger,
    config: &crate::config::Config,
    base_lang: &str,
    target_lang: &str,
) -> Result<Vec<String>, String> {
    if sentence_text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let initial = get_initial_segments_from_llm(
        sentence_text,
        s_id,
        llm,
        prompts,
        logger,
        config,
        base_lang,
        target_lang,
    )?;

    if initial.len() <= 1 {
        return Ok(initial);
    }

    Ok(merge_short_segments(initial, MIN_SEGMENT_WORDS))
}

// ─── Internal: LLM call + validation + alignment ─────────────────────────────

/// Calls the LLM with the standard Sn:-label format and parses the response.
///
/// 1. Loads the `segment_sentence_universal` system prompt template.
/// 2. Builds the user prompt as `Sn: <text>` (consistent with all other stages).
/// 3. Calls the LLM.
/// 4. Parses the response: finds the `Sn:` header and collects subsequent lines.
/// 5. Validates word-content integrity (normalized alphanumeric comparison).
/// 6. Aligns segment boundaries back to the *original* text using anchor words.
fn get_initial_segments_from_llm(
    sentence_text: &str,
    s_id: &str,
    llm: &LlmService,
    prompts: &PromptManager,
    logger: &LlmLogger,
    config: &crate::config::Config,
    base_lang: &str,
    target_lang: &str,
) -> Result<Vec<String>, String> {
    // 1. Load system prompt (no substitution needed — it's a static template now)
    let system_prompt = prompts
        .get_prompt("segment_sentence_universal", base_lang, target_lang)
        .map_err(|e| format!("Failed to load segmentation prompt: {e}"))?;

    // 2. Resolve model from config
    let model_name = resolve_segmenter_model(config)?;

    // 3. Build user prompt with Sn: label (consistent with all other LLM stages)
    let user_prompt = format!("{}: {}", s_id, sentence_text);

    // 4. Set context so MockLlmProvider knows which canned file to read.
    llm.set_context("segment_sentence_universal");

    // 5. Call LLM
    let raw_response = llm.complete(&model_name, &system_prompt, &user_prompt)?;

    // Log
    let _ = logger.log_interaction(
        &format!("LLMSegmenter S_ID={s_id}"),
        &system_prompt,
        &user_prompt,
        &raw_response,
    );

    // 5. Parse the Sn:-labeled response to extract segment lines for this sentence
    let llm_segments = parse_labeled_segmentation_response(&raw_response, s_id)?;

    if llm_segments.is_empty() {
        return Ok(vec![sentence_text.to_string()]);
    }

    // 6. Validate word content
    validate_word_content(sentence_text, &llm_segments, s_id)?;

    // 7. Anchor-word alignment
    let aligned = align_segments_to_original(sentence_text, &llm_segments, s_id)?;

    Ok(aligned)
}

/// Parse an LLM segmentation response in labeled format.
///
/// The expected format is:
/// ```text
/// S1:
/// segment line 1
/// segment line 2
/// ...
///
/// S2:
/// segment line 1
/// ...
/// ```
///
/// Returns the segment lines for the requested `s_id`.
fn parse_labeled_segmentation_response(
    response: &str,
    s_id: &str,
) -> Result<Vec<String>, String> {
    // Require at least one digit so content lines like "diciendo:" or
    // "leche:" are not mistaken for section headers.
    let header_re = Regex::new(r"^\s*([A-Za-z]+\d+)\s*:\s*$").unwrap();
    
    let mut segments_for_id: Vec<String> = Vec::new();
    let mut found = false;

    for line in response.lines() {
        if let Some(cap) = header_re.captures(line) {
            let id = cap[1].trim().to_string();
            if found {
                // We already collected segments for our ID and hit the next header — done
                break;
            }
            if id == s_id {
                found = true;
            }
            continue;
        }

        if found {
            let trimmed = line.trim().to_string();
            if !trimmed.is_empty() {
                segments_for_id.push(trimmed);
            }
        }
    }

    if !found {
        return Err(format!(
            "Segmentation response did not contain expected label '{s_id}'"
        ));
    }

    Ok(segments_for_id)
}

/// Resolves the concrete model name string (e.g. "claude-3-haiku-20240307") from
/// the config chain: `stages.Segmenter.primary_model` → `models.<key>.name`.
fn resolve_segmenter_model(config: &crate::config::Config) -> Result<String, String> {
    let stage_cfg = config
        .get_stage_config("Segmenter")
        .ok_or("No [stages.Segmenter] in config")?;

    let model_key = &stage_cfg.primary_model;

    // Validate the alias exists in [models], but return the alias itself —
    // RoutingLlmProvider::complete() expects the alias, not the provider name.
    let _model_cfg = config
        .get_model_config(model_key)
        .ok_or(format!(
            "Model key '{model_key}' (from stages.Segmenter.primary_model) not found in [models]"
        ))?;

    Ok(model_key.clone())
}

// ─── Validation ───────────────────────────────────────────────────────────────

/// Port of the Python word-content validation.
///
/// Normalizes both the original text and the joined LLM segments to
/// lowercase alphanumeric characters and compares them. If they differ,
/// the LLM modified word content (hallucinated / dropped words).
fn validate_word_content(
    original: &str,
    llm_segments: &[String],
    s_id: &str,
) -> Result<(), String> {
    let original_norm = normalize_alphanumeric(original);
    let llm_norm = normalize_alphanumeric(&llm_segments.join(""));

    if original_norm != llm_norm {
        return Err(format!(
            "LLM content mismatch for S_ID {s_id}. LLM modified word content.\n  \
             original_norm: {original_norm}\n  llm_norm:      {llm_norm}"
        ));
    }
    Ok(())
}

/// Lowercase, keep only `[a-z0-9]`.
fn normalize_alphanumeric(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

// ─── Anchor-word alignment ────────────────────────────────────────────────────

/// Port of the Python anchor-word alignment algorithm.
///
/// For each LLM segment (except the last), we:
/// 1. Extract the last "word" token from the segment.
/// 2. Find that anchor word in the *original* text (from `current_search_offset`).
/// 3. Advance past trailing non-alphanumeric / whitespace characters.
/// 4. Slice the original text at that boundary.
///
/// The last segment always gets the remainder of the original text.
///
/// This ensures the returned segments are slices of the *original* characters,
/// not the LLM's potentially-altered characters.
fn align_segments_to_original(
    sentence_text: &str,
    llm_segments: &[String],
    s_id: &str,
) -> Result<Vec<String>, String> {
    let opening_punct: HashSet<char> = OPENING_PUNCT.iter().cloned().collect();
    let word_re = Regex::new(r"[\w']+").unwrap();
    let _chars: Vec<char> = sentence_text.chars().collect();

    let mut final_segments: Vec<String> = Vec::new();
    let mut current_offset: usize = 0; // byte offset into sentence_text

    for (i, segment_chunk) in llm_segments.iter().enumerate() {
        if segment_chunk.trim().is_empty() {
            continue;
        }

        // Last segment: take the rest
        if i == llm_segments.len() - 1 {
            let remaining = &sentence_text[current_offset..];
            if !remaining.is_empty() {
                final_segments.push(remaining.to_string());
            }
            break;
        }

        // Find the last word in this LLM segment (anchor word)
        let chunk_words: Vec<&str> = word_re.find_iter(segment_chunk).map(|m| m.as_str()).collect();
        if chunk_words.is_empty() {
            continue;
        }
        let anchor_word = *chunk_words.last().unwrap();

        // Normalize quotes for matching
        let normalized_anchor = normalize_quotes(anchor_word);
        let search_slice = &sentence_text[current_offset..];
        let normalized_slice = normalize_quotes(search_slice);

        // Find anchor in the normalized slice
        let anchor_escaped = regex::escape(&normalized_anchor);
        let anchor_re = Regex::new(&anchor_escaped)
            .map_err(|e| format!("Bad anchor regex for '{anchor_word}': {e}"))?;

        let mat = anchor_re
            .find(&normalized_slice)
            .ok_or_else(|| {
                format!(
                    "Segmenter Integrity Check FAILED for S_ID {s_id}: \
                     Could not find anchor word '{anchor_word}'."
                )
            })?;

        // Convert back to absolute byte offset
        let slice_end_byte_relative = mat.end();
        let mut abs_byte = current_offset + slice_end_byte_relative;

        // Advance past non-alphanumeric characters (but stop at opening punctuation)
        while abs_byte < sentence_text.len() {
            let ch = get_char_at_byte(sentence_text, abs_byte);
            match ch {
                Some(c) if c.is_alphanumeric() => break,
                Some(c) if opening_punct.contains(&c) => break,
                Some(c) => abs_byte += c.len_utf8(),
                None => break,
            }
        }

        // Advance past whitespace
        while abs_byte < sentence_text.len() {
            let ch = get_char_at_byte(sentence_text, abs_byte);
            match ch {
                Some(c) if c.is_whitespace() => abs_byte += c.len_utf8(),
                _ => break,
            }
        }

        final_segments.push(sentence_text[current_offset..abs_byte].to_string());
        current_offset = abs_byte;
    }

    Ok(final_segments)
}

/// Normalize curly quotes to straight quotes for matching.
fn normalize_quotes(text: &str) -> String {
    text.replace('\u{2019}', "'").replace('\u{2018}', "'")
}

/// Get the char at a given byte offset in a string.
fn get_char_at_byte(s: &str, byte_offset: usize) -> Option<char> {
    if byte_offset >= s.len() {
        return None;
    }
    s[byte_offset..].chars().next()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_alphanumeric() {
        assert_eq!(normalize_alphanumeric("Hello, World! 123"), "helloworld123");
        assert_eq!(normalize_alphanumeric(""), "");
        assert_eq!(normalize_alphanumeric("..."), "");
    }

    #[test]
    fn test_validate_word_content_ok() {
        let original = "The cat sat on the mat.";
        let segments = vec![
            "The cat sat".to_string(),
            "on the mat.".to_string(),
        ];
        assert!(validate_word_content(original, &segments, "S1").is_ok());
    }

    #[test]
    fn test_validate_word_content_mismatch() {
        let original = "The cat sat on the mat.";
        let segments = vec![
            "The dog sat".to_string(), // "cat" → "dog"
            "on the mat.".to_string(),
        ];
        assert!(validate_word_content(original, &segments, "S1").is_err());
    }

    #[test]
    fn test_align_segments_basic() {
        let original = "The cat sat on the mat.";
        let llm_segments = vec![
            "The cat sat".to_string(),
            "on the mat.".to_string(),
        ];
        let result = align_segments_to_original(original, &llm_segments, "S1").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "The cat sat ");
        assert_eq!(result[1], "on the mat.");
    }

    #[test]
    fn test_align_segments_with_punctuation() {
        let original = "he thought, but that was something he was unable to do";
        let llm_segments = vec![
            "he thought,".to_string(),
            "but that was something".to_string(),
            "he was unable to do".to_string(),
        ];
        let result = align_segments_to_original(original, &llm_segments, "S1").unwrap();
        assert_eq!(result.len(), 3);
        // "he thought, " — trailing comma and space absorbed
        assert_eq!(result[0], "he thought, ");
        assert_eq!(result[1], "but that was something ");
        assert_eq!(result[2], "he was unable to do");
    }

    #[test]
    fn test_align_preserves_original_characters() {
        // LLM might output straight quotes but original has curly
        let original = "\u{201C}Hello,\u{201D} she said.";
        let llm_segments = vec![
            "\"Hello,\"".to_string(),
            "she said.".to_string(),
        ];
        // The alignment should still work because we normalize quotes
        // and the final segments come from the original text
        let result = align_segments_to_original(original, &llm_segments, "S1");
        // This may or may not succeed depending on anchor word matching
        // "Hello" should be findable in both
        assert!(result.is_ok());
    }

    #[test]
    fn test_normalize_quotes() {
        assert_eq!(normalize_quotes("don\u{2019}t"), "don't");
        assert_eq!(normalize_quotes("\u{2018}hello\u{2019}"), "'hello'");
    }

    #[test]
    fn test_merge_integration() {
        // A single very short segment should survive if it's the only one
        let segments = vec!["Hi.".to_string()];
        let merged = merge_short_segments(segments, MIN_SEGMENT_WORDS);
        assert_eq!(merged.len(), 1);
    }

    // ─── Tests for Sn:-labeled response parsing ──────────────────────────────

    #[test]
    fn test_parse_labeled_response_single_sentence() {
        let response = "S1:\nThe cat sat\non the mat.";
        let result = parse_labeled_segmentation_response(response, "S1").unwrap();
        assert_eq!(result, vec!["The cat sat", "on the mat."]);
    }

    #[test]
    fn test_parse_labeled_response_multi_sentence() {
        let response = "S1:\nThe cat sat\non the mat.\n\nS2:\nHe ran quickly\nthrough the alley.";
        let result = parse_labeled_segmentation_response(response, "S2").unwrap();
        assert_eq!(result, vec!["He ran quickly", "through the alley."]);
    }

    #[test]
    fn test_parse_labeled_response_extracts_correct_id() {
        let response = "S3:\nline one\nline two\n\nS4:\nother line";
        let result = parse_labeled_segmentation_response(response, "S3").unwrap();
        assert_eq!(result, vec!["line one", "line two"]);
    }

    #[test]
    fn test_parse_labeled_response_missing_id() {
        let response = "S1:\nThe cat sat\non the mat.";
        let result = parse_labeled_segmentation_response(response, "S99");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("S99"));
    }

    #[test]
    fn test_parse_labeled_response_ignores_blank_lines_within() {
        // Blank lines between segments within the same ID are skipped
        let response = "S1:\nline one\n\nline two";
        let result = parse_labeled_segmentation_response(response, "S1").unwrap();
        assert_eq!(result, vec!["line one", "line two"]);
    }

    #[test]
    fn test_parse_labeled_response_stops_at_next_header() {
        let response = "S1:\nalpha\nbeta\nS2:\ngamma";
        let result = parse_labeled_segmentation_response(response, "S1").unwrap();
        assert_eq!(result, vec!["alpha", "beta"]);
    }
}
