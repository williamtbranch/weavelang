//! Source-text parser.
//!
//! Reads a `.txt` source file containing `{S1: text}` sentences and
//! `%%META key: value%%` / `%%CHAPTER_MARKER%%` / `%%META lesson_marker%%`
//! preamble-style directives. Returns the parsed document plus a
//! `SourceMeta` snapshot of any directives encountered.
//!
//! Directive grammar (all directives are case-sensitive on the key):
//!
//! ```text
//! %%META source_language: <code>%%
//! %%META target_language: <code>%%
//! %%META book_name: <string>%%
//! %%META simple_mode: on|off%%
//! %%META frontier_enabled: on|off%%
//! %%META friendly_shielding: on|off%%
//! %%META lesson_realign: on|off%%
//! %%META friendly_lemma: <lemma>%%       (repeatable)
//! %%META teaching_mode: on|off%%         (preset; expands to simple_mode=on +
//!                                         frontier_enabled=off + asserts
//!                                         friendly_shielding=on +
//!                                         lesson_realign=on)
//! %%META source_is_basic: on|off%%       (assertion: source is already at
//!                                         basic/simple-reader level; engine
//!                                         skips the in-source-language
//!                                         simplify pass and copies `base` →
//!                                         the same-language basic tier
//!                                         verbatim. Requires simple_mode=on.)
//! %%META lm_entry: bas=<N>[, from=S<k>]%%   (absolute)
//! %%META lm_entry: bas=+<N>[, from=S<k>]%%  (relative bump from previous)
//! %%META lesson_progression: bas_start=<N>, step=<M>, per=lesson%%
//! %%META lesson_marker%%
//! %%META chapter: <name>%%             (creates a chapter anchored at the
//!                                       next sentence; repeatable)
//! %%CHAPTER_MARKER%%                    (legacy, ignored)
//! ```

use crate::domain::segment::Segment;
use crate::domain::sentence::Sentence;
use crate::domain::tier::Tier;
use crate::domain::token_stream::TokenStream;
use regex::Regex;
use std::error::Error;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One embedded level-map recipe entry parsed from `%%META lm_entry%%`
/// (or expanded from `lesson_progression` / `lesson_marker`).
///
/// `start_sentence_idx` is 0-based into the parsed document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedLmEntry {
    pub start_sentence_idx: usize,
    pub bas: u32,
}

/// One embedded chapter parsed from `%%META chapter: <name>%%`.
///
/// Indices are 1-based **inclusive** to match `app::state::Chapter`.
/// `end` is filled in during `resolve_directives` based on the next
/// chapter's `start` (or the document length for the last chapter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedChapter {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// Snapshot of all directive-driven metadata extracted from a source file.
///
/// Fields use `Option` for "optional / unspecified" so callers can tell
/// the difference between "not in the file" and "explicitly off".
#[derive(Debug, Clone, Default)]
pub struct SourceMeta {
    pub source_language: Option<String>,
    pub target_language: Option<String>,
    pub book_name: Option<String>,
    pub simple_mode: Option<bool>,
    pub frontier_enabled: Option<bool>,
    pub friendly_shielding_enabled: Option<bool>,
    pub lesson_realign_enabled: Option<bool>,
    pub friendly_lemmas: Vec<String>,
    /// Whether `%%META teaching_mode: on%%` appeared. Already expanded
    /// into the other fields; recorded here for telemetry / display.
    pub teaching_mode_requested: bool,
    /// Assertion that the source file is already authored at the basic
    /// (simple-reader) level. When `Some(true)`, the engine skips the
    /// in-source-language simplify pass and copies `base` → the
    /// same-language basic tier verbatim. Requires `simple_mode = on`.
    pub source_is_basic: Option<bool>,
    /// Resolved level-map entries (absolute `bas` values, sentence-anchored).
    pub lm_entries: Vec<EmbeddedLmEntry>,
    /// Resolved chapter ranges, sorted by `start`.
    pub chapters: Vec<EmbeddedChapter>,
    /// Non-fatal warnings (unknown keys, soft constraint violations).
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Parser entry point
// ---------------------------------------------------------------------------

pub fn parse_source_file(
    content: &str,
) -> Result<(Vec<Sentence>, SourceMeta), Box<dyn Error>> {
    let s_re = Regex::new(r"^\{S(\d+):\s*(.*)\}$").unwrap();

    let (cleaned, raw_directives) = extract_directives(content);

    let mut document: Vec<Sentence> = Vec::new();

    for raw_line in cleaned.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Sentence line.
        if let Some(caps) = s_re.captures(trimmed) {
            let s_id_num = caps.get(1).unwrap().as_str();
            let text = caps.get(2).unwrap().as_str();
            let s_id = format!("S{s_id_num}");
            let mut sentence = Sentence::new(s_id);
            let mut tier = Tier::new("base".to_string());
            let segment = Segment::from_stream(
                "S1".to_string(),
                TokenStream::new(text),
                vec![],
            );
            tier.add_segment(segment);
            sentence.add_tier(tier);
            document.push(sentence);
            continue;
        }

        // Unknown line — preserve old behaviour: silently skip.
    }

    let meta = resolve_directives(&raw_directives, &document);
    Ok((document, meta))
}

/// One raw, un-resolved directive captured during pre-extraction.
#[derive(Debug, Clone)]
pub struct RawDirective {
    pub line_no: usize,
    pub key: String,
    pub value: String,
    /// Running sentence index at the moment this directive appeared in
    /// the source file (i.e. how many `{Sn:}` lines preceded it).
    /// Used as the default anchor for `lm_entry` / `lesson_marker`
    /// when no `from=Sk` is supplied.
    pub anchor_idx: usize,
    /// Byte offset into the **cleaned** content (with directive lines
    /// stripped) where the prose immediately following this directive
    /// begins. Used by the prose-bridge path to split content into
    /// per-chapter chunks so each chapter's start sentence index can
    /// be computed correctly.
    pub cleaned_offset: usize,
}

/// Strip `%%META ...%%` and `%%CHAPTER_MARKER%%` lines from `content`,
/// returning the cleaned text plus the list of raw directives encountered.
///
/// A directive line is one whose *trimmed* content matches the directive
/// grammar; mixed-in directives within prose lines are NOT recognised
/// (they would never have been recognised by the previous parser either).
///
/// Both `%%META key: value%%` and `%%META key=value%%` (and the bare
/// `%%META key%%`) are accepted to be forgiving of authoring style.
pub fn extract_directives(content: &str) -> (String, Vec<RawDirective>) {
    let s_re = Regex::new(r"^\{S(\d+):\s*(.*)\}$").unwrap();
    // Accept either `:` or `=` as the key/value separator, or no value at
    // all (e.g. `%%META lesson_marker%%`).
    let meta_re = Regex::new(
        r"^%%META\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s*[:=]\s*|\s+)?(.*?)\s*%%$",
    )
    .unwrap();

    let mut cleaned = String::with_capacity(content.len());
    let mut directives: Vec<RawDirective> = Vec::new();
    let mut sentence_count: usize = 0;

    for (line_no, raw_line) in content.lines().enumerate() {
        let line_no = line_no + 1;
        let trimmed = raw_line.trim();

        if trimmed == "%%CHAPTER_MARKER%%" {
            continue;
        }

        if let Some(caps) = meta_re.captures(trimmed) {
            let key = caps.get(1).unwrap().as_str().to_string();
            let value = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("").to_string();
            directives.push(RawDirective {
                line_no,
                key,
                value,
                anchor_idx: sentence_count,
                cleaned_offset: cleaned.len(),
            });
            continue;
        }

        if s_re.is_match(trimmed) {
            sentence_count += 1;
        }

        cleaned.push_str(raw_line);
        cleaned.push('\n');
    }

    (cleaned, directives)
}

/// Resolve a list of pre-extracted directives against a parsed document
/// into a `SourceMeta`. Index-anchored entries (`lm_entry` without
/// `from=Sk`, and `lesson_marker`) use each directive's captured
/// `anchor_idx` (the running sentence count when the directive
/// appeared) so interleaved authoring still produces correct indices.
/// For the bridge/prose path, all directives are at the top of the
/// file, anchor_idx=0, which targets the start of the document.
pub fn resolve_directives(directives: &[RawDirective], document: &[Sentence]) -> SourceMeta {
    let mut meta = SourceMeta::default();
    let mut progression: Option<LessonProgression> = None;
    let mut last_bas: Option<u32> = None;

    for d in directives {
        apply_directive(
            &d.key,
            &d.value,
            d.line_no,
            d.anchor_idx,
            document,
            &mut meta,
            &mut progression,
            &mut last_bas,
        );
    }

    meta.lm_entries.sort_by_key(|e| e.start_sentence_idx);

    // source_is_basic only meaningful when simple_mode is on. Don't
    // unset the user's flag — keep it for telemetry — but warn so the
    // contradiction surfaces in the import log.
    if meta.source_is_basic == Some(true) && meta.simple_mode != Some(true) {
        meta.warnings.push(
            "source_is_basic: on requires simple_mode: on; flag will be ignored at stage dispatch"
                .to_string(),
        );
    }

    // Resolve chapter end indices.
    // Chapters were pushed in source order with start = anchor_idx + 1.
    // Each chapter ends one before the next chapter starts; the last
    // chapter ends at document.len() (1-based inclusive).
    if !meta.chapters.is_empty() {
        meta.chapters.sort_by_key(|c| c.start);
        let doc_end = document.len();
        let n = meta.chapters.len();
        for i in 0..n {
            let end = if i + 1 < n {
                meta.chapters[i + 1].start.saturating_sub(1)
            } else {
                doc_end
            };
            meta.chapters[i].end = end;
        }
        // Drop empty/invalid chapters (e.g. directive after last sentence).
        meta.chapters.retain(|c| c.start >= 1 && c.end >= c.start);
    }

    meta
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct LessonProgression {
    bas_start: u32,
    step: u32,
    seeded: bool,
}

fn apply_directive(
    key: &str,
    value: &str,
    line_no: usize,
    anchor_idx: usize,
    document: &[Sentence],
    meta: &mut SourceMeta,
    progression: &mut Option<LessonProgression>,
    last_bas: &mut Option<u32>,
) {
    match key {
        "source_language" => meta.source_language = Some(value.to_string()),
        "target_language" => meta.target_language = Some(value.to_string()),
        "book_name" => meta.book_name = Some(value.to_string()),
        "simple_mode" => match parse_on_off(value) {
            Some(b) => meta.simple_mode = Some(b),
            None => meta.warnings.push(format!(
                "line {line_no}: simple_mode expects on|off, got {value:?}"
            )),
        },
        "source_is_basic" => match parse_on_off(value) {
            Some(b) => meta.source_is_basic = Some(b),
            None => meta.warnings.push(format!(
                "line {line_no}: source_is_basic expects on|off, got {value:?}"
            )),
        },
        "frontier_enabled" => match parse_on_off(value) {
            Some(b) => meta.frontier_enabled = Some(b),
            None => meta.warnings.push(format!(
                "line {line_no}: frontier_enabled expects on|off, got {value:?}"
            )),
        },
        "friendly_shielding" => match parse_on_off(value) {
            Some(b) => meta.friendly_shielding_enabled = Some(b),
            None => meta.warnings.push(format!(
                "line {line_no}: friendly_shielding expects on|off, got {value:?}"
            )),
        },
        "lesson_realign" => match parse_on_off(value) {
            Some(b) => meta.lesson_realign_enabled = Some(b),
            None => meta.warnings.push(format!(
                "line {line_no}: lesson_realign expects on|off, got {value:?}"
            )),
        },
        "friendly_lemma" => {
            if value.is_empty() {
                meta.warnings.push(format!(
                    "line {line_no}: friendly_lemma value is empty, ignoring"
                ));
            } else {
                meta.friendly_lemmas.push(value.to_lowercase());
            }
        }
        "teaching_mode" => match parse_on_off(value) {
            Some(true) => {
                meta.teaching_mode_requested = true;
                meta.simple_mode = Some(true);
                meta.frontier_enabled = Some(false);
                match meta.friendly_shielding_enabled {
                    Some(false) => meta.warnings.push(format!(
                        "line {line_no}: teaching_mode: on but friendly_shielding is off (kept user value)"
                    )),
                    _ => meta.friendly_shielding_enabled = Some(true),
                }
                match meta.lesson_realign_enabled {
                    Some(false) => meta.warnings.push(format!(
                        "line {line_no}: teaching_mode: on but lesson_realign is off (kept user value)"
                    )),
                    _ => meta.lesson_realign_enabled = Some(true),
                }
            }
            Some(false) => {
                // No-op per design: does NOT unset the underlying flags.
            }
            None => meta.warnings.push(format!(
                "line {line_no}: teaching_mode expects on|off, got {value:?}"
            )),
        },
        "lm_entry" => {
            if meta.simple_mode != Some(true) {
                meta.warnings.push(format!(
                    "line {line_no}: lm_entry requires simple_mode=on, ignoring"
                ));
                return;
            }
            match parse_lm_entry(value, document, *last_bas, anchor_idx) {
                Ok(entry) => {
                    *last_bas = Some(entry.bas);
                    meta.lm_entries.push(entry);
                }
                Err(e) => meta.warnings.push(format!("line {line_no}: lm_entry: {e}")),
            }
        }
        "lesson_progression" => {
            if meta.simple_mode != Some(true) {
                meta.warnings.push(format!(
                    "line {line_no}: lesson_progression requires simple_mode=on, ignoring"
                ));
                return;
            }
            match parse_lesson_progression(value) {
                Ok(p) => *progression = Some(p),
                Err(e) => meta.warnings.push(format!(
                    "line {line_no}: lesson_progression: {e}"
                )),
            }
        }
        "lesson_marker" => {
            if let Some(p) = progression.as_mut() {
                let next_idx = anchor_idx;
                let bas = if !p.seeded {
                    p.seeded = true;
                    p.bas_start
                } else {
                    last_bas.unwrap_or(p.bas_start).saturating_add(p.step)
                };
                *last_bas = Some(bas);
                meta.lm_entries.push(EmbeddedLmEntry {
                    start_sentence_idx: next_idx,
                    bas,
                });
            } else {
                meta.warnings.push(format!(
                    "line {line_no}: lesson_marker without lesson_progression, ignoring"
                ));
            }
        }
        "chapter" => {
            if value.is_empty() {
                meta.warnings.push(format!(
                    "line {line_no}: chapter requires a name, ignoring"
                ));
                return;
            }
            // anchor_idx counts sentences seen *before* this directive line,
            // so the next sentence's 1-based index is anchor_idx + 1.
            let start = anchor_idx + 1;
            if meta.chapters.iter().any(|c| c.name == value) {
                meta.warnings.push(format!(
                    "line {line_no}: duplicate chapter name {value:?}, ignoring"
                ));
                return;
            }
            meta.chapters.push(EmbeddedChapter {
                name: value.to_string(),
                start,
                end: 0, // filled in by resolve_directives
            });
        }
        _ => meta.warnings.push(format!(
            "line {line_no}: unknown META key {key:?}, ignoring"
        )),
    }
}

fn parse_on_off(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn parse_lm_entry(
    value: &str,
    document: &[Sentence],
    last_bas: Option<u32>,
    anchor_idx: usize,
) -> Result<EmbeddedLmEntry, String> {
    let mut bas: Option<u32> = None;
    let mut from_idx: Option<usize> = None;
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (k, v) = part
            .split_once('=')
            .ok_or_else(|| format!("expected key=value in {part:?}"))?;
        let k = k.trim();
        let v = v.trim();
        match k {
            "bas" => {
                if let Some(rest) = v.strip_prefix('+') {
                    let bump: u32 = rest
                        .parse()
                        .map_err(|_| format!("bas=+N: cannot parse {rest:?}"))?;
                    let prev = last_bas.ok_or_else(|| {
                        "bas=+N requires a previous absolute lm_entry".to_string()
                    })?;
                    bas = Some(prev.saturating_add(bump));
                } else {
                    bas = Some(
                        v.parse::<u32>()
                            .map_err(|_| format!("bas: cannot parse {v:?}"))?,
                    );
                }
            }
            "from" => {
                let id = v.trim_start_matches('S');
                let id_full = format!("S{id}");
                let idx = document
                    .iter()
                    .position(|s| s.id == id_full)
                    .ok_or_else(|| {
                        format!("from={id_full}: no sentence with that id parsed yet")
                    })?;
                from_idx = Some(idx);
            }
            _ => return Err(format!("unknown key {k:?} in lm_entry")),
        }
    }
    let bas = bas.ok_or_else(|| "missing bas=...".to_string())?;
    let start_sentence_idx = from_idx.unwrap_or(anchor_idx);
    Ok(EmbeddedLmEntry {
        start_sentence_idx,
        bas,
    })
}

fn parse_lesson_progression(value: &str) -> Result<LessonProgression, String> {
    let mut bas_start: Option<u32> = None;
    let mut step: Option<u32> = None;
    let mut per_lesson_seen = false;
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (k, v) = part
            .split_once('=')
            .ok_or_else(|| format!("expected key=value in {part:?}"))?;
        let k = k.trim();
        let v = v.trim();
        match k {
            "bas_start" => {
                bas_start = Some(
                    v.parse::<u32>()
                        .map_err(|_| format!("bas_start: cannot parse {v:?}"))?,
                );
            }
            "step" => {
                step = Some(
                    v.parse::<u32>()
                        .map_err(|_| format!("step: cannot parse {v:?}"))?,
                );
            }
            "per" => {
                if v != "lesson" {
                    return Err(format!("per: only 'lesson' supported, got {v:?}"));
                }
                per_lesson_seen = true;
            }
            _ => return Err(format!("unknown key {k:?} in lesson_progression")),
        }
    }
    let bas_start = bas_start.ok_or_else(|| "missing bas_start=...".to_string())?;
    let step = step.ok_or_else(|| "missing step=...".to_string())?;
    if !per_lesson_seen {
        return Err("missing per=lesson".to_string());
    }
    Ok(LessonProgression {
        bas_start,
        step,
        seeded: false,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> (Vec<Sentence>, SourceMeta) {
        parse_source_file(s).expect("parse failed")
    }

    #[test]
    fn legacy_no_directives() {
        let (doc, meta) = parse("{S1: Hello.}\n{S2: World.}\n");
        assert_eq!(doc.len(), 2);
        assert!(meta.lm_entries.is_empty());
        assert!(meta.warnings.is_empty());
        assert_eq!(meta.simple_mode, None);
    }

    #[test]
    fn chapter_marker_still_ignored() {
        let (doc, meta) = parse("%%CHAPTER_MARKER%%\n{S1: Hi.}\n");
        assert_eq!(doc.len(), 1);
        assert!(meta.warnings.is_empty());
    }

    #[test]
    fn basic_meta_keys() {
        let src = "\
%%META source_language: es%%
%%META target_language: es%%
%%META book_name: Lessons%%
%%META simple_mode: on%%
%%META friendly_lemma: de%%
%%META friendly_lemma: TÚ%%
%%META friendly_shielding: off%%
{S1: De oro.}
";
        let (doc, meta) = parse(src);
        assert_eq!(doc.len(), 1);
        assert_eq!(meta.source_language.as_deref(), Some("es"));
        assert_eq!(meta.target_language.as_deref(), Some("es"));
        assert_eq!(meta.book_name.as_deref(), Some("Lessons"));
        assert_eq!(meta.simple_mode, Some(true));
        assert_eq!(meta.friendly_shielding_enabled, Some(false));
        assert_eq!(meta.friendly_lemmas, vec!["de", "tú"]);
        assert!(meta.warnings.is_empty());
    }

    #[test]
    fn unknown_key_warns() {
        let (_, meta) = parse("%%META wibble: x%%\n{S1: A.}\n");
        assert!(!meta.warnings.is_empty(), "expected warning");
        assert!(meta.warnings[0].contains("wibble"));
    }

    #[test]
    fn teaching_mode_expands_preset() {
        let (_, meta) = parse("%%META teaching_mode: on%%\n{S1: A.}\n");
        assert!(meta.teaching_mode_requested);
        assert_eq!(meta.simple_mode, Some(true));
        assert_eq!(meta.frontier_enabled, Some(false));
        assert_eq!(meta.friendly_shielding_enabled, Some(true));
        assert_eq!(meta.lesson_realign_enabled, Some(true));
    }

    #[test]
    fn teaching_mode_warns_when_friendly_shielding_explicitly_off() {
        let src = "\
%%META friendly_shielding: off%%
%%META teaching_mode: on%%
{S1: A.}
";
        let (_, meta) = parse(src);
        assert_eq!(meta.friendly_shielding_enabled, Some(false));
        assert!(meta
            .warnings
            .iter()
            .any(|w| w.contains("friendly_shielding is off")));
    }

            #[test]
            fn teaching_mode_warns_when_lesson_realign_explicitly_off() {
            let src = "\
        %%META lesson_realign: off%%
        %%META teaching_mode: on%%
        {S1: A.}
        ";
            let (_, meta) = parse(src);
            assert_eq!(meta.lesson_realign_enabled, Some(false));
            assert!(meta
                .warnings
                .iter()
                .any(|w| w.contains("lesson_realign is off")));
            }

    #[test]
    fn teaching_mode_off_is_noop() {
        let src = "\
%%META simple_mode: on%%
%%META teaching_mode: off%%
{S1: A.}
";
        let (_, meta) = parse(src);
        assert!(!meta.teaching_mode_requested);
        assert_eq!(meta.simple_mode, Some(true));
    }

    #[test]
    fn lm_entry_absolute_anchors_to_next_sentence() {
        let src = "\
%%META simple_mode: on%%
%%META lm_entry: bas=10%%
{S1: A.}
{S2: B.}
%%META lm_entry: bas=20%%
{S3: C.}
";
        let (_, meta) = parse(src);
        assert_eq!(
            meta.lm_entries,
            vec![
                EmbeddedLmEntry { start_sentence_idx: 0, bas: 10 },
                EmbeddedLmEntry { start_sentence_idx: 2, bas: 20 },
            ]
        );
    }

    #[test]
    fn lm_entry_relative_bumps_from_previous() {
        let src = "\
%%META simple_mode: on%%
%%META lm_entry: bas=5%%
{S1: A.}
%%META lm_entry: bas=+1%%
{S2: B.}
%%META lm_entry: bas=+3%%
{S3: C.}
";
        let (_, meta) = parse(src);
        assert_eq!(
            meta.lm_entries,
            vec![
                EmbeddedLmEntry { start_sentence_idx: 0, bas: 5 },
                EmbeddedLmEntry { start_sentence_idx: 1, bas: 6 },
                EmbeddedLmEntry { start_sentence_idx: 2, bas: 9 },
            ]
        );
    }

    #[test]
    fn lm_entry_from_resolves_explicit_sentence() {
        let src = "\
%%META simple_mode: on%%
{S1: A.}
{S2: B.}
{S3: C.}
%%META lm_entry: bas=7, from=S2%%
";
        let (_, meta) = parse(src);
        assert_eq!(
            meta.lm_entries,
            vec![EmbeddedLmEntry { start_sentence_idx: 1, bas: 7 }]
        );
    }

    #[test]
    fn lm_entry_rejected_when_simple_mode_off() {
        let (_, meta) = parse("%%META lm_entry: bas=10%%\n{S1: A.}\n");
        assert!(meta.lm_entries.is_empty());
        assert!(meta.warnings.iter().any(|w| w.contains("simple_mode=on")));
    }

    #[test]
    fn lesson_progression_with_markers() {
        let src = "\
%%META simple_mode: on%%
%%META lesson_progression: bas_start=1, step=1, per=lesson%%
%%META lesson_marker%%
{S1: A.}
{S2: B.}
%%META lesson_marker%%
{S3: C.}
%%META lesson_marker%%
{S4: D.}
";
        let (_, meta) = parse(src);
        assert_eq!(
            meta.lm_entries,
            vec![
                EmbeddedLmEntry { start_sentence_idx: 0, bas: 1 },
                EmbeddedLmEntry { start_sentence_idx: 2, bas: 2 },
                EmbeddedLmEntry { start_sentence_idx: 3, bas: 3 },
            ]
        );
    }

    #[test]
    fn lesson_marker_without_progression_warns() {
        let src = "\
%%META simple_mode: on%%
%%META lesson_marker%%
{S1: A.}
";
        let (_, meta) = parse(src);
        assert!(meta.lm_entries.is_empty());
        assert!(meta
            .warnings
            .iter()
            .any(|w| w.contains("without lesson_progression")));
    }

    #[test]
    fn chapter_directive_creates_chapters() {
        let src = "\
%%META chapter: intro%%
{S1: A.}
{S2: B.}
%%META chapter: middle%%
{S3: C.}
{S4: D.}
{S5: E.}
%%META chapter: end%%
{S6: F.}
";
        let (doc, meta) = parse(src);
        assert_eq!(doc.len(), 6);
        assert!(meta.warnings.is_empty(), "warnings: {:?}", meta.warnings);
        assert_eq!(meta.chapters.len(), 3);
        assert_eq!(meta.chapters[0].name, "intro");
        assert_eq!((meta.chapters[0].start, meta.chapters[0].end), (1, 2));
        assert_eq!(meta.chapters[1].name, "middle");
        assert_eq!((meta.chapters[1].start, meta.chapters[1].end), (3, 5));
        assert_eq!(meta.chapters[2].name, "end");
        assert_eq!((meta.chapters[2].start, meta.chapters[2].end), (6, 6));
    }

    #[test]
    fn chapter_directive_empty_name_warns() {
        let src = "\
%%META chapter: %%
{S1: A.}
";
        let (_, meta) = parse(src);
        assert!(meta.chapters.is_empty());
        assert!(meta.warnings.iter().any(|w| w.contains("chapter requires a name")));
    }

    #[test]
    fn chapter_directive_duplicate_name_warns() {
        let src = "\
%%META chapter: foo%%
{S1: A.}
%%META chapter: foo%%
{S2: B.}
";
        let (_, meta) = parse(src);
        assert_eq!(meta.chapters.len(), 1);
        assert!(meta.warnings.iter().any(|w| w.contains("duplicate chapter name")));
    }

    #[test]
    fn chapter_directive_after_last_sentence_dropped() {
        // Anchor would be sentence index 1 (1-based), but document only has
        // one sentence, so the chapter has start=2,end=1 → invalid → dropped.
        let src = "\
{S1: A.}
%%META chapter: trailing%%
";
        let (_, meta) = parse(src);
        assert_eq!(meta.chapters.len(), 0);
    }
}
