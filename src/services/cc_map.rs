// src/services/cc_map.rs
//
// Interlinear closed-caption map export.
//
// When the pure-target output (`generate_weave b` → `<book>_ULbNN.txt`) is
// produced, this module writes a companion `<book>_ULbNN_cc.json` into
// `tts_files/`. The file carries, per sentence, the ordered basic_target
// word/phrase tokens with their base-language glosses from the inverse
// diglot mapping, plus punctuation tokens. `create_video.py` consumes it
// (via `cc_subtitles.py`) to burn word-synchronized interlinear captions
// into the final video.
//
// JSON schema (format 1):
// {
//   "format": 1,
//   "lang_spoken": "es",       // language of the TTS audio (target lang)
//   "lang_gloss": "en",        // language of the interlinear gloss (base lang)
//   "sentences": [
//     {
//       "n": 1,                          // 1-based sentence number
//       "text": "Si dejas el camino, …", // cleaned TTS text of the sentence
//       "tokens": [
//         {"w": "Si", "g": "If"},        // word/phrase cell: spoken + gloss
//         {"w": "puedes caer", "g": "you might fall"},
//         {"p": ","},                    // punctuation (attaches to previous cell)
//         ...
//       ]
//     }, ...
//   ]
// }

use crate::domain::mapping::MappingEntry;
use crate::domain::primitives::WordId;
use crate::domain::sentence::Sentence;
use crate::domain::token_stream::Token;
use crate::simulation::text_generator::clean_text_for_tts;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Build the CC map JSON value for a slice of sentences.
/// Sentence numbering is 1-based over the given slice (matching the woven
/// text file, one sentence per paragraph).
pub fn build_cc_map(sentences: &[Sentence], lang_spoken: &str, lang_gloss: &str) -> Value {
    let mut out_sentences: Vec<Value> = Vec::with_capacity(sentences.len());

    for (idx, sent) in sentences.iter().enumerate() {
        let mut tokens: Vec<Value> = Vec::new();
        let mut text = String::new();

        if let Some(tier) = sent.get_tier("basic_target") {
            text = clean_text_for_tts(&tier.full_text());

            // Inverse diglot mapping: basic_target → basic_base
            let entry_map: HashMap<WordId, &MappingEntry> = sent
                .mappings
                .iter()
                .find(|m| m.from_tier_id == "basic_target")
                .map(|m| m.entries.iter().map(|e| (e.source_word_id, e)).collect())
                .unwrap_or_default();

            for seg in &tier.segments {
                for tok in seg.stream.tokens() {
                    match tok {
                        Token::Background(bg) => {
                            let trimmed = bg.trim();
                            if !trimmed.is_empty() {
                                tokens.push(json!({ "p": trimmed }));
                            }
                        }
                        Token::Word(wd) => {
                            let gloss = entry_map
                                .get(&wd.id)
                                .map(|e| e.target_text.clone())
                                .unwrap_or_default();
                            tokens.push(json!({
                                "w": clean_text_for_tts(&wd.text),
                                "g": gloss,
                            }));
                        }
                    }
                }
            }
        }

        out_sentences.push(json!({
            "n": idx + 1,
            "text": text,
            "tokens": tokens,
        }));
    }

    json!({
        "format": 1,
        "lang_spoken": lang_spoken,
        "lang_gloss": lang_gloss,
        "sentences": out_sentences,
    })
}

/// Export the CC map for `sentences` to `out_path` as pretty-printed JSON.
/// Returns the number of sentences that had at least one word token.
pub fn export_cc_map(
    sentences: &[Sentence],
    lang_spoken: &str,
    lang_gloss: &str,
    out_path: &Path,
) -> Result<usize, String> {
    let map = build_cc_map(sentences, lang_spoken, lang_gloss);

    let mapped_count = map["sentences"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|s| {
                    s["tokens"]
                        .as_array()
                        .map(|t| t.iter().any(|tok| tok.get("w").is_some()))
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);

    let json_str = serde_json::to_string_pretty(&map)
        .map_err(|e| format!("Failed to serialize CC map: {}", e))?;
    fs::write(out_path, json_str)
        .map_err(|e| format!("Failed to write CC map '{}': {}", out_path.display(), e))?;

    Ok(mapped_count)
}

/// Typeset the interlinear read-along PDF for an exported CC map by running
/// `cc_pdf.py` (project root). `title` is the display title (underscores
/// already converted to spaces); `chapter` adds a chapter heading beneath the
/// title in chapter mode. Returns the PDF path reported by the script.
pub fn export_reader_pdf(
    cc_path: &Path,
    project_root: &Path,
    title: Option<&str>,
    chapter: Option<&str>,
) -> Result<String, String> {
    use std::process::Command;

    let script = project_root.join("cc_pdf.py");
    if !script.exists() {
        return Err(format!("cc_pdf.py not found in '{}'", project_root.display()));
    }
    let python_exe = crate::services::av_producer::find_python(project_root);

    let mut cmd = Command::new(&python_exe);
    cmd.arg(&script).arg("--cc-map").arg(cc_path);
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        cmd.arg("--title").arg(t);
    }
    if let Some(ch) = chapter.map(str::trim).filter(|c| !c.is_empty()) {
        cmd.arg("--chapter").arg(ch);
    }
    cmd.current_dir(project_root);
    cmd.env("PYTHONUTF8", "1");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run cc_pdf.py ({}): {}", python_exe, e))?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "cc_pdf.py failed: {} {}",
            stdout.trim(),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let pdf_path = stdout
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("Interlinear reader PDF -> "))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "reader PDF".to_string());
    Ok(pdf_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mapping::TierMapping;
    use crate::domain::tier::Tier;

    fn make_sentence_with_mapping() -> Sentence {
        let mut s = Sentence::new("S1".to_string());

        // basic_target tier: "Si dejas el camino."
        let mut tier = Tier::new("basic_target".to_string());
        let seg = crate::domain::segment::Segment::new(
            "S1".to_string(),
            "Si dejas el camino.",
            vec![],
        );
        tier.segments.push(seg);
        s.add_tier(tier);

        // Inverse diglot mapping for each word id
        let mut mapping = TierMapping::new("basic_target".to_string(), "basic_base".to_string());
        let word_ids: Vec<WordId> = s
            .get_tier("basic_target")
            .unwrap()
            .segments[0]
            .stream
            .words_enumerated()
            .iter()
            .map(|(_, _, wd)| wd.id)
            .collect();
        let glosses = ["If", "you leave", "the", "path"];
        for (wid, g) in word_ids.iter().zip(glosses.iter()) {
            mapping.add_entry(MappingEntry::new(*wid, g.to_string(), vec![]));
        }
        s.add_mapping(mapping);
        s
    }

    #[test]
    fn cc_map_exports_word_and_punct_tokens() {
        let s = make_sentence_with_mapping();
        let map = build_cc_map(&[s], "es", "en");

        assert_eq!(map["format"], 1);
        assert_eq!(map["lang_spoken"], "es");
        let sents = map["sentences"].as_array().unwrap();
        assert_eq!(sents.len(), 1);
        let tokens = sents[0]["tokens"].as_array().unwrap();

        // Expect 4 word tokens + 1 punctuation token (the final period)
        let words: Vec<&Value> = tokens.iter().filter(|t| t.get("w").is_some()).collect();
        let puncts: Vec<&Value> = tokens.iter().filter(|t| t.get("p").is_some()).collect();
        assert_eq!(words.len(), 4, "tokens: {:?}", tokens);
        assert_eq!(puncts.len(), 1, "tokens: {:?}", tokens);
        assert_eq!(words[0]["w"], "Si");
        assert_eq!(words[0]["g"], "If");
        assert_eq!(words[1]["g"], "you leave");
        assert_eq!(puncts[0]["p"], ".");
    }

    #[test]
    fn cc_map_handles_missing_tier() {
        let s = Sentence::new("S1".to_string());
        let map = build_cc_map(&[s], "es", "en");
        let sents = map["sentences"].as_array().unwrap();
        assert_eq!(sents.len(), 1);
        assert!(sents[0]["tokens"].as_array().unwrap().is_empty());
    }
}
