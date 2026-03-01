// src/services/llm_overrides.rs
//
// Port of llm2books/llm_overrides.py
//
// Scans LLM log files for %%MANUAL_FIX%%...%%END_MANUAL_FIX%% blocks
// and parses them into an override map: { sentence_id -> mapping_content }.

use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

/// Parses the content between %%MANUAL_FIX%% and %%END_MANUAL_FIX%% markers,
/// extracting `ID -> mapping_content` pairs.
///
/// Each block may contain one or more sentence IDs. Each ID line looks like:
/// ```text
/// S42:
/// MAPPINGS:
/// apple -> manzana
/// dog -> perro
/// VALIDATION: apple dog
/// ```
///
/// Only the mapping lines between `MAPPINGS:` and `VALIDATION:` are kept.
/// If an ID appears in multiple blocks, the last one wins.
fn parse_fix_block(block: &str) -> HashMap<String, String> {
    // ID must contain at least one digit (e.g. S42, S18_A1) to distinguish
    // from keywords like MAPPINGS: and VALIDATION:
    let id_re = Regex::new(r"^\s*([A-Z0-9_]*\d[A-Z0-9_]*)\s*:").unwrap();
    let mut result = HashMap::new();

    let mut current_id: Option<String> = None;
    let mut buffer: Vec<String> = Vec::new();

    // Flush helper: extracts MAPPINGS content from a buffered block
    let flush = |id: &str, buf: &[String], out: &mut HashMap<String, String>| {
        let content_str = buf.join("\n");
        let content_str = content_str.trim();
        if let Some(after_mappings) = content_str.split("MAPPINGS:").nth(1) {
            let mappings_content = if let Some(before_validation) = after_mappings.split("VALIDATION:").next() {
                before_validation.trim()
            } else {
                after_mappings.trim()
            };
            if !mappings_content.is_empty() {
                out.insert(id.to_string(), mappings_content.to_string());
            }
        }
    };

    for line in block.lines() {
        if let Some(caps) = id_re.captures(line) {
            // Flush the previous ID, if any
            if let Some(ref prev_id) = current_id {
                flush(prev_id, &buffer, &mut result);
            }
            let new_id = caps[1].trim().to_string();
            // Start buffer with everything after the colon
            let after_colon = line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
            buffer = vec![after_colon];
            current_id = Some(new_id);
        } else if current_id.is_some() {
            buffer.push(line.to_string());
        }
    }

    // Flush the last block
    if let Some(ref last_id) = current_id {
        flush(last_id, &buffer, &mut result);
    }

    result
}

/// Reads a log file and extracts all manual overrides from
/// `%%MANUAL_FIX%%...%%END_MANUAL_FIX%%` blocks.
///
/// Returns a map from sentence ID (e.g. `"S42"`) to the raw mapping lines string.
/// If an ID appears in multiple blocks, the last definition wins.
///
/// Ported from `llm_overrides.py::load_manual_overrides`.
pub fn load_manual_overrides(log_file: &Path) -> HashMap<String, String> {
    let mut override_map = HashMap::new();

    let content = match std::fs::read_to_string(log_file) {
        Ok(c) => c,
        Err(_) => return override_map,
    };

    if !content.contains("%%MANUAL_FIX%%") {
        return override_map;
    }

    let block_re = Regex::new(r"(?s)%%MANUAL_FIX%%(.*?)%%END_MANUAL_FIX%%").unwrap();
    let mut blocks: Vec<String> = block_re
        .captures_iter(&content)
        .map(|c| c[1].to_string())
        .collect();

    // Legacy fallback: if we found the start marker but no end marker,
    // take everything after the start marker
    if blocks.is_empty() && !content.contains("%%END_MANUAL_FIX%%") {
        if let Some(after) = content.split("%%MANUAL_FIX%%").nth(1) {
            blocks.push(after.to_string());
        }
    }

    for block in &blocks {
        let parsed = parse_fix_block(block);
        override_map.extend(parsed); // last-wins semantics
    }

    override_map
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_single_fix_block() {
        let block = r#"
S42:
MAPPINGS:
apple -> manzana
dog -> perro
VALIDATION: apple dog
"#;
        let result = parse_fix_block(block);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("S42"));
        let content = &result["S42"];
        assert!(content.contains("apple -> manzana"));
        assert!(content.contains("dog -> perro"));
        assert!(!content.contains("VALIDATION"));
    }

    #[test]
    fn test_parse_multiple_ids_in_one_block() {
        let block = r#"
S10:
MAPPINGS:
cat -> gato
VALIDATION: cat

S11:
MAPPINGS:
house -> casa
VALIDATION: house
"#;
        let result = parse_fix_block(block);
        assert_eq!(result.len(), 2);
        assert!(result["S10"].contains("cat -> gato"));
        assert!(result["S11"].contains("house -> casa"));
    }

    #[test]
    fn test_parse_empty_block() {
        let result = parse_fix_block("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_block_without_mappings_keyword() {
        let block = r#"
S99:
just some random text
"#;
        let result = parse_fix_block(block);
        // No MAPPINGS: keyword → nothing extracted
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_from_file_with_markers() {
        let dir = std::env::temp_dir().join("weavelang_test_overrides");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("test_overrides.log");

        let content = r#"
Some normal log content...
%%MANUAL_FIX%%
S5:
MAPPINGS:
tree -> árbol
VALIDATION: tree
%%END_MANUAL_FIX%%
More log content...
%%MANUAL_FIX%%
S7:
MAPPINGS:
river -> río
VALIDATION: river
%%END_MANUAL_FIX%%
"#;
        std::fs::write(&file, content).unwrap();

        let overrides = load_manual_overrides(&file);
        assert_eq!(overrides.len(), 2);
        assert!(overrides["S5"].contains("tree -> árbol"));
        assert!(overrides["S7"].contains("river -> río"));

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_load_from_file_missing() {
        let result = load_manual_overrides(Path::new("/nonexistent/path.log"));
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_from_file_no_markers() {
        let dir = std::env::temp_dir().join("weavelang_test_no_markers");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("clean.log");
        std::fs::write(&file, "just normal log content\nno fix blocks here").unwrap();

        let result = load_manual_overrides(&file);
        assert!(result.is_empty());

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_legacy_no_end_marker() {
        let dir = std::env::temp_dir().join("weavelang_test_legacy");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("legacy.log");

        let content = r#"
Some log output...
%%MANUAL_FIX%%
S99:
MAPPINGS:
water -> agua
VALIDATION: water
"#;
        std::fs::write(&file, content).unwrap();

        let overrides = load_manual_overrides(&file);
        assert_eq!(overrides.len(), 1);
        assert!(overrides["S99"].contains("water -> agua"));

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_last_block_wins_for_duplicate_ids() {
        let dir = std::env::temp_dir().join("weavelang_test_dupes");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("dupes.log");

        let content = r#"
%%MANUAL_FIX%%
S1:
MAPPINGS:
old -> viejo
VALIDATION: old
%%END_MANUAL_FIX%%
%%MANUAL_FIX%%
S1:
MAPPINGS:
old -> antiguo
VALIDATION: old
%%END_MANUAL_FIX%%
"#;
        std::fs::write(&file, content).unwrap();

        let overrides = load_manual_overrides(&file);
        assert_eq!(overrides.len(), 1);
        assert!(overrides["S1"].contains("old -> antiguo"));

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
    }
}
