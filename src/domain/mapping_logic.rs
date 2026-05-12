// src/domain/mapping_logic.rs
use crate::domain::mapping::{MappingEntry, TierMapping};
use crate::domain::primitives::WordData;
use crate::domain::token_stream::{Token, TokenStream};
use regex::Regex;
use std::collections::HashMap;

/// Represents a single parsed line from the LLM output.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedMapping {
    pub source_text: String,
    pub target_text: String,
    pub is_proper_noun: bool,
    pub is_no_sub: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LLMResponse {
    pub mappings: Vec<ParsedMapping>,
    pub validation_text: Option<String>,
}

/// Placeholder for the logic that takes a raw TokenStream and fuses tokens
/// based on LLM-provided groupings (e.g., "in the" -> [in, the]).
///
/// This version includes LEVENSHTEIN-based fuzzy matching to handle:
/// 1. Punctuation variations (e.g., "bad-looking" vs ["bad", "-", "looking"])
/// 2. Contractions (e.g., "don't" vs ["do", "n't"])
/// 3. Minor casing/spacing differences
pub fn fuse_tokens_from_groups(
    stream: &mut TokenStream,
    groups: &[String],
) -> Result<(), String> {
    let old_tokens = stream.tokens();
    let mut new_tokens = Vec::new();
    let mut current_idx = 0;

    for group in groups {
        let normalized_group = normalize_for_match(group);
        if normalized_group.is_empty() {
            // If the group is purely punctuation, just check literal match or skip
            // For now, treat as noise and continue
            continue;
        }

        // 1. Preserve preceding backgrounds until the next Word
        while current_idx < old_tokens.len() {
            match &old_tokens[current_idx] {
                Token::Background(_) => {
                    new_tokens.push(old_tokens[current_idx].clone());
                    current_idx += 1;
                }
                Token::Word(_) => break, // Found the start of the next potential match
            }
        }

        // 2. Lookahead Loop: Try fusing next 1..N tokens to match the group
        let mut tokens_consumed = 0;
        let mut min_distance = usize::MAX;
        
        // Search window: Look ahead up to 10 tokens (arbitrary limit for performance)
        // or until end of stream.
        let max_lookahead = std::cmp::min(current_idx + 15, old_tokens.len());
        let mut accumulating_norm_text = String::new();
        
        for i in current_idx..max_lookahead {
            let token = &old_tokens[i];
            
            // Build up the normalized text of the window
            match token {
                Token::Word(w) => {
                    accumulating_norm_text.push_str(&normalize_for_match(&w.text));
                },
                Token::Background(b) => {
                    // Backgrounds usually don't contribute to match unless they contain significant chars
                    // But normalize_for_match strips non-alphanumeric anyway.
                    accumulating_norm_text.push_str(&normalize_for_match(b));
                }
            }

            // Calculate distance
            let dist = levenshtein_distance(&normalized_group, &accumulating_norm_text);
            
            // Heuristic: If distance is 0, it's a perfect match -> break immediately
            if dist == 0 {
                min_distance = 0;
                tokens_consumed = (i - current_idx) + 1;
                break;
            }
            
            // If distance is small enough (e.g., <= 2 chars or 20%), consider it a candidate
            // But we keep looking in case a longer match is better (though unlikely for strict subset)
            if dist < min_distance {
                min_distance = dist;
                tokens_consumed = (i - current_idx) + 1;
            }
        }

        // Threshold: Allow up to 2 edits or 20% of length, whichever is larger
        let threshold = std::cmp::max(2, normalized_group.len() / 5);
        
        if min_distance > threshold {
             return Err(format!(
                "Could not find match for group '{}'. Best match dist: {}, threshold: {}.",
                group, min_distance, threshold
            ));
        }

        // 3. Fuse the identified chunk
        // Collect the tokens we decided on
        let mut chunk_tokens = Vec::new();
        for _ in 0..tokens_consumed {
            chunk_tokens.push(old_tokens[current_idx].clone());
            current_idx += 1;
        }

        let first_word = chunk_tokens
            .iter()
            .find_map(|t| match t {
                Token::Word(w) => Some(w),
                _ => None,
            })
            .expect("Matched chunk must contain at least one word"); // Should hold if logic is correct

        let new_id = first_word.id;
        let mut fused_text = String::new();
        let mut fused_lemmas = Vec::new();

        for t in chunk_tokens {
            match t {
                Token::Background(s) => fused_text.push_str(&s),
                Token::Word(w) => {
                    fused_text.push_str(&w.text);
                    fused_lemmas.extend(w.lemmas.clone());
                }
            }
        }

        fused_lemmas.sort();
        fused_lemmas.dedup();

        new_tokens.push(Token::Word(WordData::new(
            new_id,
            fused_text,
            fused_lemmas,
        )));
    }

    // 4. Copy any remaining tokens
    while current_idx < old_tokens.len() {
        new_tokens.push(old_tokens[current_idx].clone());
        current_idx += 1;
    }

    *stream = TokenStream::from_tokens(new_tokens);
    Ok(())
}

/// Parses an LLM response string to extract mapping pairs and validation text.
///
/// Expected format:
/// S1:
/// MAPPINGS:
/// Source -> Target
/// ...
/// VALIDATION: Source Source
pub fn parse_llm_mapping(raw: &str) -> LLMResponse {
    // Strip any thinking/usage header injected by the LLM client.
    // The header ends at "--- END THINKING ---\n"; if present, skip past it.
    // Also skip standalone "--- USAGE: ... ---" lines.
    let stripped: std::borrow::Cow<str> = if let Some(pos) = raw.find("--- END THINKING ---") {
        let after = &raw[pos + "--- END THINKING ---".len()..];
        std::borrow::Cow::Borrowed(after.trim_start_matches('\n'))
    } else if raw.starts_with("--- USAGE:") {
        // No thinking block, but there's a usage line — skip the first line
        match raw.find('\n') {
            Some(nl) => std::borrow::Cow::Borrowed(&raw[nl + 1..]),
            None => std::borrow::Cow::Borrowed(raw),
        }
    } else {
        std::borrow::Cow::Borrowed(raw)
    };
    let raw = stripped.as_ref();

    let mut mappings = Vec::new();
    let mut validation_text = None;

    // Regex matches prefixes like "S1:", "id S1:", "1:", "s1:"
    // (?i) = case insensitive
    // ^ = start of string
    // (?:id\s+)? = optional "id" followed by whitespace
    // [\w]+ = identifier (alphanumeric)
    // :\s* = colon followed by optional whitespace
    let prefix_re = Regex::new(r"(?i)^(?:id\s+)?[\w]+:\s*").unwrap();
    
    // Match {{Name}} OR {Name}
    // Looks for a sequence wrapped in at least one set of braces.
    let proper_noun_re = Regex::new(r"\{+([^}]+)\}+").unwrap();

    for line in raw.lines() {
        let trimmed_line = line.trim();
        
        // Handle VALIDATION section
        if trimmed_line.starts_with("VALIDATION:") {
            validation_text = Some(trimmed_line["VALIDATION:".len()..].trim().to_string());
            continue;
        }
        
        // Skip empty lines or headers like "MAPPINGS:"
        // We also want to skip lines that look like "S1:" which don't have "->"
        if trimmed_line.is_empty() || trimmed_line == "MAPPINGS:" {
            continue;
        }
        
        // Only process lines containing the arrow separator
        if let Some((source_part, target_part)) = line.split_once("->") {
            // Helper to clean whitespace and remove ID prefixes
            let clean = |text: &str| -> String {
                let trimmed = text.trim();
                prefix_re.replace(trimmed, "").trim().to_string()
            };

            let source = clean(source_part);
            let raw_target = clean(target_part);

            // Check for proper noun formatting {{...}} or {...}
            let (target, is_proper_noun) = if let Some(caps) = proper_noun_re.captures(&raw_target) {
                // Return the inner text without braces
                (caps[1].trim().to_string(), true)
            } else {
                (raw_target, false)
            };
            
            // Check for NO_SUB token (case-insensitive)
            let is_no_sub = target.eq_ignore_ascii_case("NO_SUB");

            // Ensure we don't return empty strings if a line was just "->" or "S1: ->"
            if !source.is_empty() && !target.is_empty() {
                mappings.push(ParsedMapping {
                    source_text: source,
                    target_text: target,
                    is_proper_noun,
                    is_no_sub,
                });
            }
        }
    }

    // Fix common LLM error: possessives split across two lines
    // e.g. "Hugson -> {{Hugson}}" + "s -> NO_SUB" => "Hugson's -> {{Hugson}}"
    let mappings = fix_possessive_splits(mappings);

    LLMResponse {
        mappings,
        validation_text,
    }
}

/// Detects and corrects a common LLM error where possessive nouns are split
/// across two mapping lines. For example:
///   Hugson -> {{Hugson}}
///   s -> NO_SUB
/// becomes:
///   Hugson's -> {{Hugson}}
fn fix_possessive_splits(mappings: Vec<ParsedMapping>) -> Vec<ParsedMapping> {
    let mut fixed = Vec::with_capacity(mappings.len());
    let mut i = 0;
    while i < mappings.len() {
        if i + 1 < mappings.len() {
            let next = &mappings[i + 1];
            if (next.source_text == "s" || next.source_text == "'s") && next.is_no_sub {
                let mut merged = mappings[i].clone();
                merged.source_text = format!("{}'s", merged.source_text);
                eprintln!(
                    "Auto-fixed possessive split: '{}' + '{}' merged to '{}'",
                    mappings[i].source_text,
                    next.source_text,
                    merged.source_text
                );
                fixed.push(merged);
                i += 2;
                continue;
            }
        }
        fixed.push(mappings[i].clone());
        i += 1;
    }
    fixed
}

/// Orchestrates the full process: Parse -> Fuse -> Map
///
/// This function:
/// 1. Parses the raw LLM output.
/// 2. Fuses the token stream to match the LLM's source groupings.
/// 3. Generates a TierMapping linking the fused words to the LLM's target text.
pub fn apply_llm_mapping(
    stream: &mut TokenStream,
    llm_output: &str,
    source_tier_id: &str,
    target_tier_id: &str,
) -> Result<TierMapping, String> {
    // 1. Parse
    let llm_response = parse_llm_mapping(llm_output);
    let mappings = llm_response.mappings;
    
    // Optional: Check VALIDATION string against groups?
    // Current requirement is just to parse it. 
    // Validation is implicitly handled by fuse_tokens_from_groups failing if reconstruction fails.

    let groups: Vec<String> = mappings.iter().map(|m| m.source_text.clone()).collect();

    // 2. Fuse (this modifies the stream in place)
    // This will return an error if the stream content doesn't match the groups
    fuse_tokens_from_groups(stream, &groups)?;

    // 3. Create Mapping
    let mut tier_mapping = TierMapping::new(source_tier_id.to_string(), target_tier_id.to_string());

    // Iterate through the stream's WORD tokens and align them with the parsed mappings.
    let word_tokens: Vec<&WordData> = stream
        .tokens()
        .iter()
        .filter_map(|t| match t {
            Token::Word(w) => Some(w),
            _ => None,
        })
        .collect();

    if word_tokens.len() != mappings.len() {
        return Err(format!(
            "Mismatch after fusion: Stream has {} words, Mappings has {} entries.",
            word_tokens.len(),
            mappings.len()
        ));
    }

    for (word_data, parsed) in word_tokens.iter().zip(mappings.iter()) {
        let mut entry = MappingEntry::new(
            word_data.id,
            parsed.target_text.clone(),
            vec![], // TODO: Target lemmas usually come from a separate NLP pass or LLM
        );
        entry.is_proper_noun = parsed.is_proper_noun;
        
        // Handle NO_SUB logic
        if parsed.is_no_sub {
            entry.is_viable = false;
        }
        
        tier_mapping.add_entry(entry);
    }

    Ok(tier_mapping)
}

/// Calculates the Levenshtein distance between two strings.
/// Used for fuzzy matching LLM groups against token stream content.
fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let v1: Vec<char> = s1.chars().collect();
    let v2: Vec<char> = s2.chars().collect();
    let l1 = v1.len();
    let l2 = v2.len();

    let mut matrix = vec![vec![0; l2 + 1]; l1 + 1];

    for i in 0..=l1 {
        matrix[i][0] = i;
    }
    for j in 0..=l2 {
        matrix[0][j] = j;
    }

    for i in 1..=l1 {
        for j in 1..=l2 {
            let cost = if v1[i - 1] == v2[j - 1] { 0 } else { 1 };
            matrix[i][j] = std::cmp::min(
                std::cmp::min(matrix[i - 1][j] + 1, matrix[i][j - 1] + 1),
                matrix[i - 1][j - 1] + cost,
            );
        }
    }

    matrix[l1][l2]
}

/// Helper to normalize text for comparison (lowercase, remove non-alphanumeric).
fn normalize_for_match(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

// ---------------------------------------------------------------------------
// Multi-block LLM response parsing  (ported from llm_utils.py)
// ---------------------------------------------------------------------------

/// Parses a multi-ID structured LLM response into a map of `{ sentence_id -> content }`.
///
/// The input looks like:
/// ```text
/// S1:
/// MAPPINGS:
/// apple -> manzana
/// dog -> perro
/// VALIDATION: apple dog
///
/// S2:
/// MAPPINGS:
/// cat -> gato
/// VALIDATION: cat
/// ```
///
/// For each expected ID the function locates its block, then extracts only
/// the lines between `MAPPINGS:` and `VALIDATION:`.
///
/// Ported from `llm_utils.py::_parse_structured_llm_response`.
pub fn parse_structured_llm_response(
    raw_text: &str,
    expected_ids: &[&str],
) -> HashMap<String, String> {
    let mut parsed = HashMap::new();

    // Build a regex that matches ID lines at the start of a line
    let id_pattern: String = expected_ids
        .iter()
        .map(|id| regex::escape(id))
        .collect::<Vec<_>>()
        .join("|");

    let id_line_re = match Regex::new(&format!(r"(?m)^\s*({id_pattern})\s*:")) {
        Ok(re) => re,
        Err(_) => return parsed,
    };

    // Find all ID positions and split manually (no lookahead needed)
    let id_positions: Vec<(usize, String)> = id_line_re
        .captures_iter(raw_text)
        .map(|c| (c.get(0).unwrap().start(), c[1].to_string()))
        .collect();

    for (pos_idx, (start, id)) in id_positions.iter().enumerate() {
        let end = if pos_idx + 1 < id_positions.len() {
            id_positions[pos_idx + 1].0
        } else {
            raw_text.len()
        };
        let block = &raw_text[*start..end];

        let lines: Vec<&str> = block.lines().collect();
        if lines.is_empty() {
            continue;
        }

        let current_id = id.as_str();

        // Find MAPPINGS: section, collect until VALIDATION:
        let mut in_mappings = false;
        let mut buffer = Vec::new();

        for line in &lines {
            let trimmed_upper = line.trim().to_uppercase();
            if trimmed_upper.starts_with("MAPPINGS:") {
                in_mappings = true;
                // Check if there's content on the same line as "MAPPINGS:"
                let after = line.trim().get("MAPPINGS:".len()..);
                if let Some(rest) = after {
                    let rest = rest.trim();
                    if !rest.is_empty() {
                        buffer.push(rest.to_string());
                    }
                }
                continue;
            }
            if in_mappings {
                if trimmed_upper.starts_with("VALIDATION:") {
                    break;
                }
                buffer.push(line.to_string());
            }
        }

        if !buffer.is_empty() {
            parsed.insert(current_id.to_string(), buffer.join("\n").trim().to_string());
        }
    }

    parsed
}

/// Parses a simple single-line `ID: value` LLM response format.
///
/// Each line is expected to be `identifier: content`, and the function
/// returns a map of `{ id -> content }`.
///
/// Ported from `llm_utils.py::_parse_singleline_llm_response`.
pub fn parse_singleline_llm_response(raw_text: &str) -> HashMap<String, String> {
    let line_re = Regex::new(r"^\s*([^:]+):\s*(.*)$").unwrap();
    let mut parsed = HashMap::new();

    for line in raw_text.lines() {
        if let Some(caps) = line_re.captures(line) {
            let id = caps[1].trim().to_string();
            let value = caps[2].trim().to_string();
            parsed.insert(id, value);
        }
    }

    parsed
}

/// Validates that a parsed multi-line response has no empty translations.
///
/// Checks every mapping line (containing `->`) and ensures the right-hand side
/// is non-empty. Returns `Err` with a descriptive message listing the bad lines.
///
/// Ported from `llm_utils.py::validate_parsed_llm_response`.
pub fn validate_parsed_response(parsed_data: &HashMap<String, String>) -> Result<(), String> {
    for (s_id, content) in parsed_data {
        let mut bad_lines = Vec::new();
        for line in content.lines() {
            if line.contains("->") {
                let parts: Vec<&str> = line.splitn(2, "->").collect();
                if parts.len() < 2 || parts[1].trim().is_empty() {
                    bad_lines.push(line.trim().to_string());
                }
            }
        }
        if !bad_lines.is_empty() {
            return Err(format!(
                "Validation failed for S_ID '{}'. Found {} mapping lines with empty translations:\n  - {}",
                s_id,
                bad_lines.len(),
                bad_lines.join("\n  - ")
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Friendly-lemma shielding
// ---------------------------------------------------------------------------

/// Apply friendly-lemma shielding to a candidate lemma list.
///
/// "Friendly" lemmas are author-marked words that, when present in a
/// `MappingEntry.target_lemmas` candidate set, must be the *only* lemmas
/// kept — so a lesson-target word can never be substituted away by a
/// non-friendly synonym during weave.
///
/// Rules:
/// 1. If `enabled` is false, return `lemmas` unchanged.
/// 2. If `friendly_set` is empty, return `lemmas` unchanged.
/// 3. Compute the intersection of `lemmas` (mapped through `key_for_lemma`)
///    with `friendly_set`. If empty, return `lemmas` unchanged.
/// 4. Otherwise drop every non-friendly lemma; if multiple friendly lemmas
///    remain, keep only the one with the lowest frequency rank
///    (`rank_for_wlemma` returning `None` is treated as `u32::MAX`).
///    Ties are broken by original order in `lemmas`.
///
/// `friendly_set` entries and the output of `key_for_lemma` are expected
/// to be in the same key space (typically wlemma stems — see
/// `documentation/Wlemma_Migration_Plan.md`).
pub fn apply_friendly_shielding<F, G>(
    lemmas: Vec<String>,
    friendly_set: &std::collections::HashSet<String>,
    enabled: bool,
    rank_for_wlemma: F,
    key_for_lemma: G,
) -> Vec<String>
where
    F: Fn(&str) -> Option<u32>,
    G: Fn(&str) -> String,
{
    if !enabled || friendly_set.is_empty() || lemmas.is_empty() {
        return lemmas;
    }
    let friendly_indices: Vec<usize> = lemmas
        .iter()
        .enumerate()
        .filter(|(_, l)| friendly_set.contains(&key_for_lemma(l)))
        .map(|(i, _)| i)
        .collect();
    if friendly_indices.is_empty() {
        return lemmas;
    }
    if friendly_indices.len() == 1 {
        return vec![lemmas[friendly_indices[0]].clone()];
    }
    // Multiple friendly hits: pick lowest rank, ties → earliest index.
    let best = friendly_indices
        .into_iter()
        .min_by_key(|&i| {
            let rank = rank_for_wlemma(&key_for_lemma(&lemmas[i])).unwrap_or(u32::MAX);
            (rank, i)
        })
        .unwrap();
    vec![lemmas[best].clone()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::token_stream::Token;
    use crate::domain::primitives::{WordData, WordId};

    // Helper to build a stream for testing
    fn make_stream(words: Vec<&str>) -> TokenStream {
        // Simple B-W-B-W constructor for tests
        let mut tokens = Vec::new();
        tokens.push(Token::Background("".to_string()));
        for (i, w) in words.iter().enumerate() {
            tokens.push(Token::Word(WordData::new(
                WordId(i as u64),
                w.to_string(),
                vec![],
            )));
            tokens.push(Token::Background(" ".to_string()));
        }
        TokenStream::from_tokens(tokens)
    }

    #[test]
    fn test_refactor_fuses_basic_words() {
        // Scenario: "in the garden" -> LLM says groups are ["in the", "garden"]
        // Expectation: "in" and "the" should become one token "in the".
        let mut stream = make_stream(vec!["in", "the", "garden"]);
        // IDs: in=0, the=1, garden=2
        let groups = vec!["in the".to_string(), "garden".to_string()];

        let result = fuse_tokens_from_groups(&mut stream, &groups);

        assert!(result.is_ok());
        assert_eq!(stream.word_count(), 2);
        
        let tokens = stream.tokens();
        // Expected structure: B("") -> W("in the") -> B(" ") -> W("garden") -> B(" ")
        // Note: The fusion consumes the intermediate background " ".
        
        // Find the first word token
        let w1 = tokens.iter().find_map(|t| match t {
            Token::Word(w) => Some(w),
            _ => None,
        }).expect("Should have word token");
        
        assert_eq!(w1.text, "in the");
        assert_eq!(w1.id, WordId(0)); // Inherits ID from "in"
        
        // Check full text reconstruction
        assert_eq!(stream.full_text(), "in the garden "); 
    }

    #[test]
    fn test_refactor_handles_internal_punctuation_preservation() {
        // Scenario: "Ay, Dios" (Source has comma, LLM group usually drops it or keeps it)
        // Original: [Ay] [, ] [Dios]
        // Group: "Ay Dios"
        // Result should preserve the comma inside the fused word value: "Ay, Dios"

        // Setup complex stream manually
        let mut tokens = vec![
            Token::Background("".to_string()),
            Token::Word(WordData::new(WordId(0), "Ay".to_string(), vec!["ay".to_string()])),
            Token::Background(", ".to_string()),
            Token::Word(WordData::new(WordId(1), "Dios".to_string(), vec!["dios".to_string()])),
            Token::Background("! ".to_string()),
            Token::Word(WordData::new(WordId(2), "He".to_string(), vec!["he".to_string()])),
            Token::Background(" ".to_string()),
            Token::Word(WordData::new(WordId(3), "thought".to_string(), vec!["think".to_string()])),
            Token::Background(".".to_string()),
        ];
        let mut stream = TokenStream::from_tokens(tokens);
        let groups = vec!["Ay Dios".to_string(), "He".to_string(), "thought".to_string()];

        let result = fuse_tokens_from_groups(&mut stream, &groups);
        assert!(result.is_ok());

        assert_eq!(stream.word_count(), 3);
        
        let words: Vec<&WordData> = stream.tokens().iter().filter_map(|t| match t {
            Token::Word(w) => Some(w),
            _ => None,
        }).collect();

        // Check "Ay, Dios" fusion
        assert_eq!(words[0].text, "Ay, Dios");
        assert_eq!(words[0].id, WordId(0));
        // Check lemmas are merged
        assert!(words[0].lemmas.contains(&"ay".to_string()));
        assert!(words[0].lemmas.contains(&"dios".to_string()));

        // Check reconstruction
        assert_eq!(stream.full_text(), "Ay, Dios! He thought.");
    }
    
    #[test]
    fn test_refactor_to_eat_example() {
        // The user's specific example
        // "they were almost ready to eat" -> "to eat" becomes one atom
        let mut stream = make_stream(vec!["they", "were", "almost", "ready", "to", "eat"]);
        // IDs: 0..5. "to"=4, "eat"=5
        
        let groups = vec![
            "they".to_string(), 
            "were".to_string(), 
            "almost".to_string(), 
            "ready".to_string(), 
            "to eat".to_string()
        ];
        
        let result = fuse_tokens_from_groups(&mut stream, &groups);
        assert!(result.is_ok());
        
        let words: Vec<&WordData> = stream.tokens().iter().filter_map(|t| match t {
            Token::Word(w) => Some(w),
            _ => None,
        }).collect();
        
        assert_eq!(words.len(), 5);
        assert_eq!(words[4].text, "to eat");
        assert_eq!(words[4].id, WordId(4)); // Inherits from "to"
    }

    #[test]
    fn test_parse_basic_mapping() {
        let raw = "S1: The cat -> El gato\nS2: The dog -> El perro";
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 2);
        assert_eq!(result.mappings[0].source_text, "The cat");
        assert_eq!(result.mappings[0].target_text, "El gato");
        assert_eq!(result.mappings[1].source_text, "The dog");
        assert_eq!(result.mappings[1].target_text, "El perro");
        assert!(result.validation_text.is_none());
    }

    #[test]
    fn test_parse_varied_prefixes() {
        let raw = "id S1: Hello -> Hola\n2: Bye -> Adios\nno_prefix -> sin_prefijo";
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 3);
        assert_eq!(result.mappings[0].source_text, "Hello");
        assert_eq!(result.mappings[1].source_text, "Bye");
        assert_eq!(result.mappings[2].source_text, "no_prefix");
    }

    #[test]
    fn test_ignore_noise() {
        let raw = "Here is the mapping:\nS1: One -> Uno\nNote: This is a note.";
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 1);
        assert_eq!(result.mappings[0].source_text, "One");
    }

    #[test]
    fn test_parse_proper_nouns() {
        let raw = "S1: Alice -> {{Alice}}\nS2: The -> El";
        let result = parse_llm_mapping(raw);
        
        assert_eq!(result.mappings.len(), 2);
        assert_eq!(result.mappings[0].source_text, "Alice");
        assert_eq!(result.mappings[0].target_text, "Alice"); // Braces stripped
        assert!(result.mappings[0].is_proper_noun);
        
        assert_eq!(result.mappings[1].source_text, "The");
        assert_eq!(result.mappings[1].target_text, "El");
        assert!(!result.mappings[1].is_proper_noun);
    }

    #[test]
    fn test_parse_proper_nouns_flexible() {
        let raw = "S1: Alice -> {{Alice}}\nS2: Bob -> {Bob}";
        let result = parse_llm_mapping(raw);
        
        assert_eq!(result.mappings.len(), 2);
        
        // Check double braces
        assert_eq!(result.mappings[0].source_text, "Alice");
        assert_eq!(result.mappings[0].target_text, "Alice"); 
        assert!(result.mappings[0].is_proper_noun);
        
        // Check single braces
        assert_eq!(result.mappings[1].source_text, "Bob");
        assert_eq!(result.mappings[1].target_text, "Bob");
        assert!(result.mappings[1].is_proper_noun);
    }
    
    #[test]
    fn test_parse_block_format() {
        let raw = r#"
S1:
MAPPINGS:
Hello -> Hola
World -> Mundo
VALIDATION: Hello World
"#;
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 2);
        assert_eq!(result.validation_text, Some("Hello World".to_string()));
        assert_eq!(result.mappings[0].source_text, "Hello");
        assert_eq!(result.mappings[1].source_text, "World");
    }
    
    #[test]
    fn test_parse_no_sub() {
        let raw = "did -> NO_SUB";
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 1);
        assert_eq!(result.mappings[0].target_text, "NO_SUB");
        assert!(result.mappings[0].is_no_sub);
    }

    #[test]
    fn test_apply_llm_mapping_full_flow() {
        // Setup original stream: "The king had a very good garden."
        let mut stream = make_stream(vec!["The", "king", "had", "a", "very", "good", "garden"]);
        // IDs: 0..6

        // LLM output fuses "very good" -> "muy buen"
        let llm_output = "
S1:
MAPPINGS:
The -> El
king -> rey
had -> tenía
a -> un
very good -> muy buen
garden -> jardín
VALIDATION: The king had a very good garden
";

        let result = apply_llm_mapping(&mut stream, llm_output, "source", "target");
        assert!(result.is_ok());

        let mapping = result.unwrap();
        assert_eq!(mapping.entries.len(), 6);

        // Check fusion
        let words: Vec<&WordData> = stream.tokens().iter().filter_map(|t| match t {
            Token::Word(w) => Some(w),
            _ => None,
        }).collect();
        assert_eq!(words.len(), 6);
        assert_eq!(words[4].text, "very good"); // Was ID 4 ("very") and 5 ("good"), now ID 4

        // Check mapping
        assert_eq!(mapping.entries[4].source_word_id, WordData::new(WordId(4), "".into(), vec![]).id);
        assert_eq!(mapping.entries[4].target_text, "muy buen");
    }

    #[test]
    fn test_refactor_fuzzy_match() {
        // Scenario: LLM says "bad looking" (no hyphen), stream has "bad-looking" (with hyphen)
        // Stream: "bad", "-", "looking"
        let mut tokens = vec![
            Token::Word(WordData::new(WordId(0), "bad".to_string(), vec![])),
            Token::Background("-".to_string()),
            Token::Word(WordData::new(WordId(1), "looking".to_string(), vec![])),
        ];
        let mut stream = TokenStream::from_tokens(tokens);
        let groups = vec!["bad looking".to_string()];

        let result = fuse_tokens_from_groups(&mut stream, &groups);
        assert!(result.is_ok());

        let words: Vec<&WordData> = stream.tokens().iter().filter_map(|t| match t {
            Token::Word(w) => Some(w),
            _ => None,
        }).collect();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "bad-looking"); // Should fuse correctly despite mismatch
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("flaw", "flaws"), 1);
        assert_eq!(levenshtein_distance("ros", "horse"), 3); // r->h, o->o, s->r, +se
        assert_eq!(levenshtein_distance("", "abc"), 3);
    }

    #[test]
    fn test_apply_inverse_mapping_flow() {
        // Scenario: Inverse Mapping (Spanish -> English)
        // Stream: "El gato negro es grande"
        let mut stream = make_stream(vec!["El", "gato", "negro", "es", "grande"]);
        // IDs: 0..4

        // LLM output fuses "El gato" -> "The cat"
        // Note: In inverse mapping, the "Source" of the operation is Spanish (Stream),
        // and the "Target" of the operation is English (Translation).
        let llm_output = "
S1:
MAPPINGS:
El gato -> The cat
negro -> black
es -> is
grande -> big
VALIDATION: El gato negro es grande
";

        let result = apply_llm_mapping(&mut stream, llm_output, "basic_target", "basic_base");
        assert!(result.is_ok());

        let mapping = result.unwrap();
        assert_eq!(mapping.entries.len(), 4);

        // Check fusion
        let words: Vec<&WordData> = stream.tokens().iter().filter_map(|t| match t {
            Token::Word(w) => Some(w),
            _ => None,
        }).collect();
        assert_eq!(words.len(), 4);
        
        // "El gato" should be fused
        assert_eq!(words[0].text, "El gato"); 
        assert_eq!(words[0].id, WordId(0)); // Inherits from "El"

        // Check mapping
        // Entry 0: "El gato" (ID 0) -> "The cat"
        assert_eq!(mapping.entries[0].source_word_id, WordId(0));
        assert_eq!(mapping.entries[0].target_text, "The cat");
        
        // Entry 1: "negro" (ID 2) -> "black"
        assert_eq!(mapping.entries[1].source_word_id, WordId(2));
        assert_eq!(mapping.entries[1].target_text, "black");
    }

    #[test]
    fn test_fix_possessive_split_bare_s() {
        // LLM splits "Hugson's" into "Hugson" + "s -> NO_SUB"
        let raw = "Hugson -> {{Hugson}}\ns -> NO_SUB\nstation -> estación";
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 2);
        assert_eq!(result.mappings[0].source_text, "Hugson's");
        assert_eq!(result.mappings[0].target_text, "Hugson");
        assert!(result.mappings[0].is_proper_noun);
        assert_eq!(result.mappings[1].source_text, "station");
    }

    #[test]
    fn test_fix_possessive_split_apostrophe_s() {
        // LLM splits "Alice's" into "Alice" + "'s -> NO_SUB"
        let raw = "Alice -> {{Alicia}}\n's -> NO_SUB\nname -> nombre";
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 2);
        assert_eq!(result.mappings[0].source_text, "Alice's");
        assert_eq!(result.mappings[0].target_text, "Alicia");
        assert!(result.mappings[0].is_proper_noun);
        assert_eq!(result.mappings[1].source_text, "name");
    }

    #[test]
    fn test_no_false_positive_possessive_fix() {
        // "s" with a real translation should NOT be merged
        let raw = "some -> algún\ns -> ese\nword -> palabra";
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 3);
        assert_eq!(result.mappings[0].source_text, "some");
        assert_eq!(result.mappings[1].source_text, "s");
        assert_eq!(result.mappings[1].target_text, "ese");
    }

    #[test]
    fn test_parse_strips_thinking_header() {
        // Response with usage + thinking header from GeminiClient
        let raw = "--- USAGE: prompt=500tok  answer=100tok  thinking=800tok  total=1400tok ---\n\
                    --- THINKING ---\n\
                    Let me think about how to map dog -> perro correctly...\n\
                    --- END THINKING ---\n\
                    The -> El\n\
                    dog -> perro";
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 2);
        assert_eq!(result.mappings[0].source_text, "The");
        assert_eq!(result.mappings[1].source_text, "dog");
        assert_eq!(result.mappings[1].target_text, "perro");
    }

    #[test]
    fn test_parse_strips_usage_only_header() {
        // Response with only usage line (no thinking)
        let raw = "--- USAGE: prompt=500tok  answer=100tok  thinking=0tok  total=600tok ---\n\
                    The -> El\n\
                    cat -> gato";
        let result = parse_llm_mapping(raw);
        assert_eq!(result.mappings.len(), 2);
        assert_eq!(result.mappings[0].source_text, "The");
        assert_eq!(result.mappings[1].target_text, "gato");
    }

    // -----------------------------------------------------------------
    // Friendly-lemma shielding
    // -----------------------------------------------------------------

    fn fset(items: &[&str]) -> std::collections::HashSet<String> {
        items.iter().map(|s| s.to_lowercase()).collect()
    }

    /// No overlap with friendly set — list returned unchanged.
    #[test]
    fn shielding_no_overlap_passthrough() {
        let lemmas = vec!["mother".to_string(), "mom".to_string()];
        let friendly = fset(&["dog", "cat"]);
        let out = apply_friendly_shielding(lemmas.clone(), &friendly, true, |_| None, |l| l.to_lowercase());
        assert_eq!(out, lemmas);
    }

    /// Single friendly hit — only that lemma survives.
    #[test]
    fn shielding_single_overlap_keeps_only_friendly() {
        let lemmas = vec![
            "madre".to_string(),
            "mama".to_string(),
            "progenitora".to_string(),
        ];
        let friendly = fset(&["madre"]);
        let out = apply_friendly_shielding(lemmas, &friendly, true, |_| None, |l| l.to_lowercase());
        assert_eq!(out, vec!["madre".to_string()]);
    }

    /// Multiple friendly hits — pick the lowest rank.
    #[test]
    fn shielding_multi_overlap_picks_lowest_rank() {
        let lemmas = vec!["amigo".to_string(), "compa".to_string(), "ruido".to_string()];
        let friendly = fset(&["amigo", "compa"]);
        let ranks = |l: &str| -> Option<u32> {
            match l {
                "amigo" => Some(50),
                "compa" => Some(20),
                _ => None,
            }
        };
        let out = apply_friendly_shielding(lemmas, &friendly, true, ranks, |l| l.to_lowercase());
        assert_eq!(out, vec!["compa".to_string()]);
    }

    /// Missing rank treated as u32::MAX → ranked candidate wins over unranked.
    #[test]
    fn shielding_missing_rank_treated_as_max() {
        let lemmas = vec!["alpha".to_string(), "beta".to_string()];
        let friendly = fset(&["alpha", "beta"]);
        let ranks = |l: &str| -> Option<u32> {
            match l {
                "beta" => Some(100),
                _ => None,
            }
        };
        let out = apply_friendly_shielding(lemmas, &friendly, true, ranks, |l| l.to_lowercase());
        assert_eq!(out, vec!["beta".to_string()]);
    }

    /// All friendly, all unranked → first-index wins (tie-break).
    #[test]
    fn shielding_tie_break_uses_earliest_index() {
        let lemmas = vec!["a".to_string(), "b".to_string()];
        let friendly = fset(&["a", "b"]);
        let out = apply_friendly_shielding(lemmas, &friendly, true, |_| None, |l| l.to_lowercase());
        assert_eq!(out, vec!["a".to_string()]);
    }

    /// Disabled flag → bypass.
    #[test]
    fn shielding_disabled_passthrough() {
        let lemmas = vec!["madre".to_string(), "mama".to_string()];
        let friendly = fset(&["madre"]);
        let out = apply_friendly_shielding(lemmas.clone(), &friendly, false, |_| None, |l| l.to_lowercase());
        assert_eq!(out, lemmas);
    }

    /// Empty friendly set → bypass (regardless of enabled flag).
    #[test]
    fn shielding_empty_friendly_set_passthrough() {
        let lemmas = vec!["madre".to_string()];
        let friendly: std::collections::HashSet<String> = std::collections::HashSet::new();
        let out = apply_friendly_shielding(lemmas.clone(), &friendly, true, |_| None, |l| l.to_lowercase());
        assert_eq!(out, lemmas);
    }

    /// Case-folded comparison — friendly entries match upper/lower variants.
    #[test]
    fn shielding_is_case_insensitive() {
        let lemmas = vec!["Madre".to_string(), "Mama".to_string()];
        let friendly = fset(&["madre"]);
        let out = apply_friendly_shielding(lemmas, &friendly, true, |_| None, |l| l.to_lowercase());
        assert_eq!(out, vec!["Madre".to_string()]);
    }

    /// TT6 (Phase 5): with wlemma keys, a friendly entry stored as the
    /// base-form stem (`niñ`) shields a hallucinated inflected candidate
    /// (`niños`). Pre-migration this missed because the friendly set was
    /// case-folded only — `friendly_set.contains("niños") == false`.
    #[test]
    fn shielding_with_wlemma_keys_matches_inflected_candidate() {
        // Friendly set keyed by Spanish-snowball stems.
        use crate::domain::stemmer::Stemmer;
        let stemmer = crate::domain::stemmer::SpanishSnowball::new();
        let friendly: std::collections::HashSet<String> = ["niño", "amigo"]
            .iter()
            .map(|w| stemmer.stem(&w.to_lowercase()))
            .collect();
        // spaCy hallucinated: the candidate lemma is the surface form.
        let lemmas = vec!["niños".to_string(), "progenitora".to_string()];
        let key = |l: &str| stemmer.stem(&l.trim().to_lowercase());
        let out = apply_friendly_shielding(lemmas, &friendly, true, |_| None, key);
        assert_eq!(out, vec!["niños".to_string()]);
    }
}

// ---------------------------------------------------------------------------
// Tests for multi-block parsing & validation  (ported from test_llm_utils.py)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod llm_response_tests {
    use super::*;

    #[test]
    fn test_parse_structured_single_id() {
        let raw = r#"
S18:
MAPPINGS:
In spite of -> A pesar de
this -> este
talk -> charla
VALIDATION: In spite of this talk
"#;
        let result = parse_structured_llm_response(raw, &["S18"]);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("S18"));
        let content = &result["S18"];
        assert!(content.contains("In spite of -> A pesar de"));
        assert!(content.contains("this -> este"));
        assert!(content.contains("talk -> charla"));
        // VALIDATION line should be excluded
        assert!(!content.contains("VALIDATION"));
    }

    #[test]
    fn test_parse_structured_multiple_ids() {
        let raw = r#"
S1:
MAPPINGS:
apple -> manzana
VALIDATION: apple

S2:
MAPPINGS:
dog -> perro
cat -> gato
VALIDATION: dog cat
"#;
        let result = parse_structured_llm_response(raw, &["S1", "S2"]);
        assert_eq!(result.len(), 2);
        assert!(result["S1"].contains("apple -> manzana"));
        assert!(result["S2"].contains("dog -> perro"));
        assert!(result["S2"].contains("cat -> gato"));
    }

    #[test]
    fn test_parse_structured_ignores_unknown_ids() {
        let raw = r#"
S1:
MAPPINGS:
hello -> hola
VALIDATION: hello

S999:
MAPPINGS:
bye -> adios
VALIDATION: bye
"#;
        let result = parse_structured_llm_response(raw, &["S1"]);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("S1"));
        assert!(!result.contains_key("S999"));
    }

    #[test]
    fn test_parse_structured_inverse_ids() {
        // Inverse maps use IDs like "S18_A1"
        let raw = r#"
S18_A1:
MAPPINGS:
A pesar de -> In spite of
este -> this
VALIDATION: A pesar de este
"#;
        let result = parse_structured_llm_response(raw, &["S18_A1"]);
        assert_eq!(result.len(), 1);
        assert!(result["S18_A1"].contains("A pesar de -> In spite of"));
    }

    #[test]
    fn test_parse_singleline() {
        let raw = "S1: Hello world\nS2: Goodbye\nS3: Test phrase";
        let result = parse_singleline_llm_response(raw);
        assert_eq!(result.len(), 3);
        assert_eq!(result["S1"], "Hello world");
        assert_eq!(result["S2"], "Goodbye");
        assert_eq!(result["S3"], "Test phrase");
    }

    #[test]
    fn test_parse_singleline_skips_invalid() {
        let raw = "S1: Valid line\nno colon here\nS2: Also valid";
        let result = parse_singleline_llm_response(raw);
        assert_eq!(result.len(), 2);
    }

    // --- Validation tests (ported from test_llm_utils.py) ---

    #[test]
    fn test_validate_passes_good_response() {
        let mut data = HashMap::new();
        data.insert("S1".to_string(), "apple -> manzana\ndog -> perro".to_string());
        assert!(validate_parsed_response(&data).is_ok());
    }

    #[test]
    fn test_validate_fails_on_empty_forward_translation() {
        // Direct port of test_validator_fails_on_empty_spanish_forward_mapping
        let raw = r#"
S18:
MAPPINGS:
In spite of -> A pesar de
this -> este
talk -> 
VALIDATION: In spite of this talk
"#;
        let parsed = parse_structured_llm_response(raw, &["S18"]);
        let result = validate_parsed_response(&parsed);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Found 1 mapping lines with empty translations"));
        assert!(err.contains("talk ->"));
    }

    #[test]
    fn test_validate_fails_on_empty_inverse_translation() {
        // Direct port of test_validator_fails_on_empty_english_inverse_mapping
        let raw = r#"
S18_A1:
MAPPINGS:
A pesar de -> In spite of
este -> this
matrimonio -> marriage
conversación -> 
VALIDATION: some validation text here
"#;
        let parsed = parse_structured_llm_response(raw, &["S18_A1"]);
        let result = validate_parsed_response(&parsed);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Validation failed for S_ID 'S18_A1'"));
        assert!(err.contains("Found 1 mapping lines with empty translations"));
        assert!(err.contains("conversación ->"));
    }

    #[test]
    fn test_validate_multiple_bad_lines() {
        let mut data = HashMap::new();
        data.insert(
            "S5".to_string(),
            "apple -> manzana\ndog -> \ncat -> ".to_string(),
        );
        let result = validate_parsed_response(&data);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Found 2 mapping lines with empty translations"));
    }
}
