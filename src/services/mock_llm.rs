// src/services/mock_llm.rs
//
// MockLlmProvider — returns canned LLM responses from files for deterministic
// integration testing.  See documentation/Integration_Testing_Plan.md §4.
//
// ## How it works
//
// 1. At construction, the mock is given a directory path containing canned
//    response files (one per LLM stage, named after the prompt, e.g.
//    `simplify_to_basic_english.txt`, `segment_sentence_universal.txt`).
//
// 2. Before each `complete()` call, the caller (LlmStageService or
//    llm_segmenter) calls `set_context(prompt_name)`.  The mock stores
//    this and uses it to resolve the correct canned file.
//
// 3. On `complete()`, the mock:
//    a. Opens `<responses_dir>/<context>.txt`.
//    b. Parses the user prompt to extract requested IDs (Sn, Sn_Sm, etc.).
//    c. Returns only the matching entries from the canned file.
//
// ## Canned file formats
//
// **Single-line stages** (simplify_to_basic_english, translate_text_basic,
// translate_text):
//   S1: Capítulo 0
//   S2: La metamorfosis
//   ...
//
// **Segment-level stage** (simplify_segments):
//   S5_S1: Una mañana, cuando Gregor Samsa despertó
//   S5_S2: de sueños malos, se encontró cambiado
//   ...
//
// **Multi-line segmentation** (segment_sentence_universal):
//   S1:
//   Capítulo 0
//
//   S2:
//   La metamorfosis
//   ...
//
// **Multi-line phrase maps** (generate_phrase_map, generate_inverse_phrase_map):
//   S1:
//   MAPPINGS:
//   Chapter -> Capítulo
//   0 -> cero
//   VALIDATION: Chapter 0
//
//   S2:
//   MAPPINGS:
//   ...

use crate::services::llm_client::LlmProvider;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Mock LLM provider that returns canned responses from files.
pub struct MockLlmProvider {
    /// Directory containing canned response files.
    responses_dir: PathBuf,
    /// Current prompt/stage context — set before each complete() call.
    current_context: String,
    /// Cache of parsed canned files: prompt_name → { id → response_text }.
    #[allow(dead_code)]
    cache: HashMap<String, CannedFile>,
}

/// A parsed canned response file.
#[derive(Debug)]
struct CannedFile {
    /// Map from ID (e.g. "S1", "S5_S2") to the full response text for that ID.
    entries: HashMap<String, String>,
    /// Whether this is a multi-line format (segmentation, phrase maps).
    is_multiline: bool,
}

impl MockLlmProvider {
    pub fn new(responses_dir: PathBuf) -> Self {
        Self {
            responses_dir,
            current_context: String::new(),
            cache: HashMap::new(),
        }
    }

    /// Load and parse a canned response file, caching the result.
    #[allow(dead_code)]
    fn get_or_load(&mut self, prompt_name: &str) -> Result<&CannedFile, String> {
        if !self.cache.contains_key(prompt_name) {
            let path = self.responses_dir.join(format!("{}.txt", prompt_name));
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("MockLlm: Failed to read canned file {:?}: {}", path, e))?;
            let canned = parse_canned_file(&content);
            self.cache.insert(prompt_name.to_string(), canned);
        }
        Ok(self.cache.get(prompt_name).unwrap())
    }
}

impl LlmProvider for MockLlmProvider {
    fn complete(
        &self,
        _model: &str,
        _system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        if self.current_context.is_empty() {
            return Err("MockLlm: set_context() was not called before complete()".to_string());
        }

        // We need a mutable borrow to lazily load, but the trait takes &self.
        // Work around this by re-parsing from disk if not cached (the cache is
        // an optimization, not a correctness requirement).  In practice,
        // set_context is called once per stage and all calls in that stage
        // hit the same file, so we parse at most once per stage.
        let path = self.responses_dir.join(format!("{}.txt", self.current_context));
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("MockLlm: Failed to read {:?}: {}", path, e))?;
        let canned = parse_canned_file(&content);

        // Extract requested IDs from the user prompt
        let requested_ids = extract_requested_ids(user_prompt);
        if requested_ids.is_empty() {
            // No IDs found — return the entire file content (fallback)
            return Ok(content);
        }

        // Build response with only the requested entries
        let mut response_lines: Vec<String> = Vec::new();
        for id in &requested_ids {
            if let Some(text) = canned.entries.get(id.as_str()) {
                if canned.is_multiline {
                    response_lines.push(format!("{}:", id));
                    response_lines.push(text.clone());
                    response_lines.push(String::new()); // blank separator
                } else {
                    response_lines.push(format!("{}: {}", id, text));
                }
            }
            // Missing IDs are silently skipped — matches real LLM behavior
            // where the model might occasionally skip an ID.
        }

        Ok(response_lines.join("\n").trim_end().to_string())
    }

    fn set_context(&mut self, prompt_name: &str) {
        self.current_context = prompt_name.to_string();
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Extract requested sentence/segment IDs from a user prompt.
///
/// Looks for lines matching `ID: text` where ID is alphanumeric with
/// underscores, colons, or hyphens.  Returns the IDs in order.
fn extract_requested_ids(user_prompt: &str) -> Vec<String> {
    let id_re = Regex::new(r"^\s*([A-Za-z0-9_:-]+)\s*:\s*.+$").unwrap();
    let mut ids = Vec::new();
    for line in user_prompt.lines() {
        if let Some(cap) = id_re.captures(line) {
            let id = cap[1].trim().to_string();
            // Skip the "STRICT REQUIREMENT" pseudo-ID and other non-sentence IDs
            if id.starts_with('S') || id.starts_with('s') {
                ids.push(id);
            }
        }
    }
    ids
}

/// Parse a canned response file into a map of ID → response text.
///
/// Auto-detects the format:
/// - **Single-line:** `S1: text` — one entry per line
/// - **Multi-line:** `S1:` on its own line, followed by content lines until
///   the next `Sn:` header or end of file
fn parse_canned_file(content: &str) -> CannedFile {
    // Multi-line header: must start with S (sentence ID) and end with just ':'
    let header_re = Regex::new(r"^(S[A-Za-z0-9_-]*):\s*$").unwrap();
    // Single-line entry: ID followed by colon and content on the same line
    let single_line_re = Regex::new(r"^(S[A-Za-z0-9_-]*):\s+(.+)$").unwrap();

    // Detect format by checking if first non-empty line is a single-line entry
    // or a multi-line header.
    let first_content_line = content.lines().find(|l| !l.trim().is_empty());
    let is_multiline = match first_content_line {
        Some(line) => header_re.is_match(line),
        None => false,
    };

    let mut entries: HashMap<String, String> = HashMap::new();

    if is_multiline {
        // Multi-line format: S1:\n<content lines>\n\nS2:\n...
        let mut current_id: Option<String> = None;
        let mut current_lines: Vec<String> = Vec::new();

        for line in content.lines() {
            if let Some(cap) = header_re.captures(line) {
                // Save previous entry
                if let Some(id) = current_id.take() {
                    // Trim trailing empty lines
                    while current_lines.last().map_or(false, |l| l.trim().is_empty()) {
                        current_lines.pop();
                    }
                    entries.insert(id, current_lines.join("\n"));
                }
                current_id = Some(cap[1].to_string());
                current_lines.clear();
            } else if current_id.is_some() {
                current_lines.push(line.to_string());
            }
        }

        // Save last entry
        if let Some(id) = current_id {
            while current_lines.last().map_or(false, |l| l.trim().is_empty()) {
                current_lines.pop();
            }
            entries.insert(id, current_lines.join("\n"));
        }
    } else {
        // Single-line format: S1: text
        for line in content.lines() {
            if let Some(cap) = single_line_re.captures(line) {
                let id = cap[1].to_string();
                let text = cap[2].to_string();
                entries.insert(id, text);
            }
        }
    }

    CannedFile {
        entries,
        is_multiline,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_requested_ids() {
        let prompt = "STRICT REQUIREMENT: Provide exactly one line for each ID.\n\n\
                       S3: The old woman walked slowly.\n\
                       S4: He could not believe it.\n\
                       S5: The children played.";
        let ids = extract_requested_ids(prompt);
        assert_eq!(ids, vec!["S3", "S4", "S5"]);
    }

    #[test]
    fn test_extract_requested_ids_segment_level() {
        let prompt = "STRICT REQUIREMENT: ...\n\n\
                       S5_S1: cuando Gregor despertó\n\
                       S5_S2: se encontró cambiado\n\
                       S6_S1: Estaba sobre su espalda";
        let ids = extract_requested_ids(prompt);
        assert_eq!(ids, vec!["S5_S1", "S5_S2", "S6_S1"]);
    }

    #[test]
    fn test_parse_single_line_canned() {
        let content = "S1: Capítulo 0\nS2: La metamorfosis\nS3: por Franz Kafka\n";
        let canned = parse_canned_file(content);
        assert!(!canned.is_multiline);
        assert_eq!(canned.entries.len(), 3);
        assert_eq!(canned.entries["S1"], "Capítulo 0");
        assert_eq!(canned.entries["S3"], "por Franz Kafka");
    }

    #[test]
    fn test_parse_single_line_segment_level() {
        let content = "S5_S1: Una mañana\nS5_S2: de sueños malos\nS6_S1: Estaba sobre\n";
        let canned = parse_canned_file(content);
        assert!(!canned.is_multiline);
        assert_eq!(canned.entries.len(), 3);
        assert_eq!(canned.entries["S5_S1"], "Una mañana");
    }

    #[test]
    fn test_parse_multiline_segmentation() {
        let content = "\
S1:
Capítulo 0

S2:
La metamorfosis

S5:
Una mañana,
cuando Gregor despertó
de sueños intranquilos.
";
        let canned = parse_canned_file(content);
        assert!(canned.is_multiline);
        assert_eq!(canned.entries.len(), 3);
        assert_eq!(canned.entries["S1"], "Capítulo 0");
        assert_eq!(canned.entries["S2"], "La metamorfosis");
        assert_eq!(
            canned.entries["S5"],
            "Una mañana,\ncuando Gregor despertó\nde sueños intranquilos."
        );
    }

    #[test]
    fn test_parse_multiline_phrase_map() {
        let content = "\
S1:
MAPPINGS:
Chapter -> Capítulo
0 -> cero
VALIDATION: Chapter 0

S2:
MAPPINGS:
Metamorphosis -> {{Metamorfosis}}
VALIDATION: Metamorphosis
";
        let canned = parse_canned_file(content);
        assert!(canned.is_multiline);
        assert_eq!(canned.entries.len(), 2);
        assert!(canned.entries["S1"].contains("Chapter -> Capítulo"));
        assert!(canned.entries["S1"].contains("VALIDATION: Chapter 0"));
        assert!(canned.entries["S2"].contains("{{Metamorfosis}}"));
    }

    #[test]
    fn test_mock_complete_single_line() {
        let dir = std::env::temp_dir().join("mock_llm_test_single");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("translate_text.txt"),
            "S1: Capítulo 0\nS2: La metamorfosis\nS3: por Franz Kafka\nS4: Capítulo I\n",
        )
        .unwrap();

        let mut mock = MockLlmProvider::new(dir.clone());
        mock.set_context("translate_text");

        let result = mock
            .complete(
                "test-model",
                "system",
                "STRICT REQUIREMENT: ...\n\nS2: Metamorphosis\nS4: Chapter I",
            )
            .unwrap();

        assert!(result.contains("S2: La metamorfosis"));
        assert!(result.contains("S4: Capítulo I"));
        assert!(!result.contains("S1:"));
        assert!(!result.contains("S3:"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_mock_complete_multiline() {
        let dir = std::env::temp_dir().join("mock_llm_test_multi");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("segment_sentence_universal.txt"),
            "S1:\nCapítulo 0\n\nS2:\nLa\nmetamorfosis\n\nS3:\npor Franz Kafka\n",
        )
        .unwrap();

        let mut mock = MockLlmProvider::new(dir.clone());
        mock.set_context("segment_sentence_universal");

        let result = mock
            .complete("test-model", "system", "S2: some text to segment")
            .unwrap();

        assert!(result.contains("S2:"));
        assert!(result.contains("La\nmetamorfosis"));
        assert!(!result.contains("S1:"));
        assert!(!result.contains("S3:"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_mock_complete_segment_level() {
        let dir = std::env::temp_dir().join("mock_llm_test_seg");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("simplify_segments.txt"),
            "S5_S1: Una mañana\nS5_S2: de sueños malos\nS5_S3: en su cama\nS6_S1: Estaba\n",
        )
        .unwrap();

        let mut mock = MockLlmProvider::new(dir.clone());
        mock.set_context("simplify_segments");

        let result = mock
            .complete(
                "test-model",
                "system",
                "STRICT REQUIREMENT: ...\n\nS5_S1: text1\nS5_S3: text3\nS6_S1: text4",
            )
            .unwrap();

        assert!(result.contains("S5_S1: Una mañana"));
        assert!(result.contains("S5_S3: en su cama"));
        assert!(result.contains("S6_S1: Estaba"));
        assert!(!result.contains("S5_S2:"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
