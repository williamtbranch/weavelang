// src/services/tier_processor.rs
//
// Orchestrates the post-generation processing of LLM-produced text:
//   1. Semantic segmentation (LLM-based via llm_segmenter)
//   2. SpaCy tokenization of each segment (via PythonBridge)
//   3. Assembly into Vec<Segment> ready to store in a Tier
//
// This is the glue that ensures every generated tier has proper
// TokenStreams with lemmas and POS data, not just regex-split tokens.

use crate::domain::segment::Segment;
use crate::domain::token_stream::TokenStream;
use crate::services::llm_client::LlmService;
use crate::services::llm_logger::LlmLogger;
use crate::services::llm_segmenter;
use crate::services::prompt_manager::PromptManager;
use crate::services::python_bridge::BridgeService;

/// Full processing pipeline for a single tier's text:
///   LLM segmentation → per-segment tokenization → Vec<Segment>
///
/// `s_id`: the sentence ID (e.g. "S42"), used for logging / error messages.
/// `lang_code`: the language of the text (e.g. "en", "es"), passed to SpaCy.
///
/// Falls back gracefully:
/// - If config is None, skips LLM segmentation and treats the whole text as one segment.
/// - If bridge is None, uses the regex-based `TokenStream::new()` (no lemmas).
pub fn process_tier_text(
    text: &str,
    s_id: &str,
    lang_code: &str,
    bridge: Option<&BridgeService>,
    llm: Option<&LlmService>,
    prompts: Option<&PromptManager>,
    logger: Option<&LlmLogger>,
    config: Option<&crate::config::Config>,
    base_lang: &str,
    target_lang: &str,
) -> Result<Vec<Segment>, String> {
    if text.trim().is_empty() {
        return Ok(vec![Segment::new("S1".to_string(), "", vec![])]);
    }

    // Step 1: Segment (LLM-based if all services are available)
    let segment_texts = segment_text(
        text, s_id, llm, prompts, logger, config, base_lang, target_lang,
    );

    // Step 2: Tokenize each segment and build Segment objects
    let mut segments = Vec::with_capacity(segment_texts.len());
    for (i, seg_text) in segment_texts.iter().enumerate() {
        let seg_id = format!("S{}", i + 1);
        let stream = tokenize_segment(seg_text, lang_code, bridge);
        segments.push(Segment::from_stream(seg_id, stream, vec![]));
    }

    Ok(segments)
}

/// Lighter-weight version: tokenize only (no LLM segmentation).
/// This is useful when the text is already a single segment
/// (e.g., during manual edits or when re-tokenizing existing tiers).
pub fn tokenize_only(
    text: &str,
    lang_code: &str,
    bridge: Option<&BridgeService>,
) -> Vec<Segment> {
    if text.trim().is_empty() {
        return vec![Segment::new("S1".to_string(), "", vec![])];
    }
    let stream = tokenize_segment(text, lang_code, bridge);
    vec![Segment::from_stream("S1".to_string(), stream, vec![])]
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Determine the language code for a given tier ID.
/// Base-language tiers (base, basic_base) use the base language code.
/// Target-language tiers (advanced_target, moderate_target, basic_target) use the target code.
pub fn lang_for_tier(tier_id: &str, base_lang: &str, target_lang: &str) -> String {
    match tier_id {
        "base" | "basic_base" => base_lang.to_string(),
        "advanced_target" | "moderate_target" | "basic_target" => target_lang.to_string(),
        t if t.starts_with("MAPPING:") => base_lang.to_string(),
        _ => base_lang.to_string(),
    }
}

/// Run LLM segmentation if all services are present, otherwise return the text as-is.
fn segment_text(
    text: &str,
    s_id: &str,
    llm: Option<&LlmService>,
    prompts: Option<&PromptManager>,
    logger: Option<&LlmLogger>,
    config: Option<&crate::config::Config>,
    base_lang: &str,
    target_lang: &str,
) -> Vec<String> {
    if let (Some(llm), Some(prompts), Some(logger), Some(config)) = (llm, prompts, logger, config) {
        match llm_segmenter::segment_sentence(
            text, s_id, llm, prompts, logger, config, base_lang, target_lang,
        ) {
            Ok(segments) if !segments.is_empty() => segments,
            Ok(_) => vec![text.to_string()],
            Err(e) => {
                eprintln!("[TierProcessor] LLM segmentation failed for {s_id}: {e}. Using single segment.");
                vec![text.to_string()]
            }
        }
    } else {
        // No LLM services → single segment (graceful degradation)
        vec![text.to_string()]
    }
}

/// Tokenize a single segment string into a TokenStream.
/// Uses PythonBridge (SpaCy) if available, otherwise falls back to regex.
fn tokenize_segment(text: &str, lang_code: &str, bridge: Option<&BridgeService>) -> TokenStream {
    if let Some(bridge) = bridge {
        match bridge.tokenize(text, lang_code) {
            Ok(raw_tokens) => TokenStream::from_raw_spacy(raw_tokens, text),
            Err(e) => {
                eprintln!("[TierProcessor] SpaCy tokenization failed: {e}. Falling back to regex.");
                TokenStream::new(text)
            }
        }
    } else {
        TokenStream::new(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_empty_text() {
        let result = process_tier_text("", "S1", "en", None, None, None, None, None, "en", "es").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "S1");
    }

    #[test]
    fn test_process_without_services_falls_back() {
        let result = process_tier_text(
            "Hello world, this is a test.",
            "S1",
            "en",
            None, None, None, None, None,
            "en", "es",
        ).unwrap();
        // Without any services, we get a single segment with regex tokenization
        assert_eq!(result.len(), 1);
        assert!(result[0].full_text().contains("Hello"));
    }

    #[test]
    fn test_tokenize_only_without_bridge() {
        let segments = tokenize_only("The cat sat.", "en", None);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].full_text(), "The cat sat.");
    }
}
