//! Raw-source adaptation model.
//!
//! This is the data model behind the **Raw Source** tab and the
//! `File › Import Raw Text...` action.  It is a port of the ESCore "fable
//! harness" workflow into weavelang.
//!
//! ## Why a separate model?
//!
//! The normal `import source` path parses text into `Sentence`s and every
//! downstream tier honours that sentence index one-for-one.  The ESCore
//! adaptation loop is *free paraphrase*: it merges, splits, compresses, and
//! cuts whole passages, so no sentence-for-sentence correspondence exists.
//!
//! Raw source therefore lives in its own namespace: it is indexed (`R1`,
//! `R2`, ...) purely for display and chunking, and that index has **no**
//! relation to the final `S1..Sn` indexing produced when the adapted text is
//! promoted into the Source tab.
//!
//! ## Flow
//!
//! ```text
//!   File › Import Raw Text...      -> RawSource { units: [RawUnit, ...] }
//!   adapt draft   <unit>           -> unit.draft (v1)   + DRC score
//!   adapt squeeze <unit>           -> unit.draft (v2/3) + DRC score
//!   adapt run all                  -> draft + squeeze loop until PASS/floor
//!   adapt promote                  -> assembles %%META%% source text and
//!                                     runs the normal `import source` path
//! ```

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Raw sentences
// ---------------------------------------------------------------------------

/// One raw (usually English) sentence.
///
/// `id` is `R<n>` and is **local to the raw document**.  It exists so the
/// Raw Source tab can show, select, and re-chunk the imported text; it is
/// deliberately never carried into the adapted output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawSentence {
    pub id: String,
    pub text: String,
}

// ---------------------------------------------------------------------------
// DRC report
// ---------------------------------------------------------------------------

/// One rare lemma that raises the difficulty score.
///
/// Deliberately carries no replacement suggestion: a substitution that is
/// correct for one sentence is nonsense in another, and a blindly applied
/// table produced exactly that. The adapting model picks the replacement
/// from context instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offender {
    pub rank: u32,
    pub count: u32,
    pub lemma: String,
    /// `rank * count` — rare *and* repeated words hurt most.
    pub impact: u64,
}

/// Coverage-based comprehensible-input score (the "i" of i+1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IScore {
    pub coverage: f64,
    /// Master frequency rank of the word sitting at the coverage boundary.
    pub i_rank: f64,
    /// `i_rank` mapped through the UL curve.
    pub i_level: f64,
    pub plus1_tokens: u32,
    pub plus1_unique: u32,
    pub plus1_pct: f64,
}

/// Full Design Rule Check result for one adapted unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrcReport {
    pub overall_pass: bool,

    // --- text difficulty ---
    pub i_score: IScore,
    pub i_level_max: f64,
    pub i_pass: bool,

    // --- info-only user level ---
    pub avd: f64,
    pub p85_rank: f64,
    pub p95_rank: f64,
    pub ul_exact: f64,

    // --- corpus stats ---
    pub tokens: u32,
    pub in_freq_list: u32,
    pub unique_lemmas: u32,
    pub domain_adjusted_tokens: u32,
    pub domain_adjusted_lemmas: u32,

    // --- length gate ---
    pub submission_words: u32,
    pub source_words: u32,
    pub percent_of_source: f64,
    pub min_percent: f64,
    pub max_percent: f64,
    pub length_pass: bool,

    pub offenders: Vec<Offender>,
}

impl DrcReport {
    /// Rounded-down user level, as reported by the sister scoring tools.
    pub fn ul_floor(&self) -> i64 {
        self.ul_exact.floor() as i64
    }
}

// ---------------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------------

/// Where a unit stands in the draft → squeeze → pass loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdaptStatus {
    /// No draft has been produced yet.
    NotStarted,
    /// A draft exists but has not passed the DRC.
    Drafted,
    /// The DRC passed.
    Passing,
    /// The model reported `FLOOR REACHED`, or the pass ceiling was hit
    /// without a pass.  Needs human review.
    Floor,
}

impl AdaptStatus {
    pub fn label(self) -> &'static str {
        match self {
            AdaptStatus::NotStarted => "not started",
            AdaptStatus::Drafted => "drafted",
            AdaptStatus::Passing => "PASS",
            AdaptStatus::Floor => "floor",
        }
    }
}

/// One adaptation unit — a chapter, or one **part** of a long chapter.
///
/// Each unit is scored and squeezed independently, exactly like an ESCore
/// chapter folder.  Long chapters are split into parts at import time so no
/// single LLM call has to hold the whole chapter: models lose the thread and
/// start dropping beats somewhere past ~50 sentences.
///
/// On promotion all parts of a chapter are concatenated back into one
/// `%%META chapter: <name>%%` block, so the split is invisible downstream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawUnit {
    /// Stable display/selector name, e.g. `Loomings (part 2/3)`.
    /// Never overwritten by the model.
    pub name: String,
    /// Owning chapter.  All parts of a chapter share this value and are
    /// merged back together on promotion.
    #[serde(default)]
    pub chapter: String,
    /// 1-based part number within the chapter.
    #[serde(default = "default_part")]
    pub part: usize,
    /// Total number of parts in the owning chapter.
    #[serde(default = "default_part")]
    pub part_count: usize,
    /// Raw sentences, indexed `R1..Rn` **within this unit's parent document**.
    pub sentences: Vec<RawSentence>,
    /// Current adapted (Spanish) draft.  Empty until the first draft pass.
    pub draft: String,
    /// Draft revision number: 1 = first draft, 2/3 = squeeze passes.
    pub version: u32,
    /// Most recent DRC result for `draft`.
    pub report: Option<DrcReport>,
    pub status: AdaptStatus,
    /// Previous drafts, oldest first, so a squeeze can be rolled back.
    pub history: Vec<String>,
    /// Last error seen while adapting this unit.
    pub last_error: Option<String>,
}

fn default_part() -> usize {
    1
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

impl RawUnit {
    /// A chapter that fits in a single call.
    pub fn new(chapter: String, sentences: Vec<RawSentence>) -> Self {
        Self::new_part(chapter, 1, 1, sentences)
    }

    /// One part of a chapter that had to be split.
    pub fn new_part(
        chapter: String,
        part: usize,
        part_count: usize,
        sentences: Vec<RawSentence>,
    ) -> Self {
        let name = if part_count > 1 {
            format!("{} (part {}/{})", chapter, part, part_count)
        } else {
            chapter.clone()
        };
        Self {
            name,
            chapter,
            part,
            part_count,
            sentences,
            draft: String::new(),
            version: 0,
            report: None,
            status: AdaptStatus::NotStarted,
            history: Vec::new(),
            last_error: None,
        }
    }

    /// The raw text of this unit, reassembled for the LLM.
    pub fn source_text(&self) -> String {
        self.sentences
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Owning chapter, falling back to the unit name for units deserialised
    /// before chapters were tracked.
    pub fn chapter_name(&self) -> &str {
        if self.chapter.is_empty() {
            &self.name
        } else {
            &self.chapter
        }
    }

    /// True when a `run` pass has nothing left to spend money on: the unit
    /// either passes the DRC or has bottomed out at its floor.
    pub fn is_complete(&self) -> bool {
        !self.draft.trim().is_empty()
            && matches!(self.status, AdaptStatus::Passing | AdaptStatus::Floor)
    }

    /// Only the first part of a chapter carries the chapter title line.
    /// Continuation parts are pure prose so the merged chapter has exactly
    /// one heading.
    pub fn expects_title(&self) -> bool {
        self.part <= 1
    }

    /// First non-empty line of the draft — the model is required to emit a
    /// chapter title line there on first parts (see `adapt_draft.txt`).
    pub fn title_line(&self) -> Option<&str> {
        if !self.expects_title() {
            return None;
        }
        self.draft.lines().map(str::trim).find(|l| !l.is_empty())
    }

    /// The draft minus its title line, which becomes the chapter body.
    pub fn body(&self) -> String {
        if !self.expects_title() {
            return self.draft.trim().to_string();
        }
        let mut lines = self.draft.lines();
        // Skip leading blanks and the title line itself.
        let mut seen_title = false;
        let mut out: Vec<&str> = Vec::new();
        for line in lines.by_ref() {
            if !seen_title {
                if line.trim().is_empty() {
                    continue;
                }
                seen_title = true;
                continue;
            }
            out.push(line);
        }
        out.join("\n").trim().to_string()
    }
}

// ---------------------------------------------------------------------------
// Domain lemma policy
// ---------------------------------------------------------------------------

/// A book-level "approved vocabulary" entry.
///
/// The scorer applies `effective_rank = min(raw_rank, rank)`, and the same
/// list is injected into the model prompt so it knows those words are free to
/// use plainly instead of being paraphrased away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainLemma {
    pub lemma: String,
    pub rank: u32,
    pub gloss: Option<String>,
}

/// Default policy rank applied to a domain lemma with no explicit `=rank`.
pub const DEFAULT_DOMAIN_RANK: u32 = 320;

/// Parse a domain-lemma policy file.
///
/// ```text
/// ballena                # whale
/// capitan=260            # captain
/// arpon 320
/// ```
pub fn parse_domain_lemmas(text: &str) -> Vec<DomainLemma> {
    use crate::domain::normalization::normalize_spanish_lemma;

    let mut out = Vec::new();
    for raw_line in text.lines() {
        let (body, gloss) = match raw_line.split_once('#') {
            Some((b, g)) => (b, Some(g.trim().to_string())),
            None => (raw_line, None),
        };
        let body = body.trim();
        if body.is_empty() {
            continue;
        }

        let (lemma_part, rank_part) = if let Some((l, r)) = body.split_once('=') {
            (l.trim(), r.trim())
        } else {
            let mut parts = body.split_whitespace();
            let l = parts.next().unwrap_or("");
            match parts.next() {
                Some(r) if r.chars().all(|c| c.is_ascii_digit()) => (l, r),
                _ => (l, ""),
            }
        };

        let lemma = normalize_spanish_lemma(lemma_part);
        if lemma.is_empty() {
            continue;
        }
        let rank = rank_part
            .parse::<u32>()
            .ok()
            .filter(|r| *r > 0)
            .unwrap_or(DEFAULT_DOMAIN_RANK);

        out.push(DomainLemma {
            lemma,
            rank,
            gloss: gloss.filter(|g| !g.is_empty()),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Gate configuration
// ---------------------------------------------------------------------------

/// Gate thresholds for the adaptation loop.
///
/// Defaults mirror the ESCore Moby Dick book policy: the coverage-based
/// i-score is the primary gate, UL is reported but does not gate, and the
/// length gate is a ratio against the raw English source.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AdaptTarget {
    /// Fraction of running text that must be inside the comprehensible core.
    pub coverage: f64,
    /// Maximum allowed i-level for that core.  This is the real target level.
    pub i_level_max: f64,
    /// Minimum adapted word count as a percentage of the raw source.
    pub min_percent: f64,
    /// Maximum adapted word count as a percentage of the raw source.
    pub max_percent: f64,
    /// Anti-churn ceiling: draft + N squeeze passes.
    pub max_squeeze_passes: u32,
    /// Stop squeezing when a pass improves the i-level by less than this.
    pub min_gain: f64,
}

impl Default for AdaptTarget {
    fn default() -> Self {
        Self {
            coverage: 0.85,
            i_level_max: 24.5,
            min_percent: 60.0,
            max_percent: 120.0,
            max_squeeze_passes: 2,
            min_gain: 0.1,
        }
    }
}

// ---------------------------------------------------------------------------
// Raw source document
// ---------------------------------------------------------------------------

/// The whole imported raw document plus its adaptation state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawSource {
    /// Display name (file stem of the imported file).
    pub name: String,
    /// Language of the raw text.  Usually English.
    pub language: String,
    /// Language the adaptation is written in.  Usually Spanish.
    pub target_language: String,
    pub units: Vec<RawUnit>,
    pub domain_lemmas: Vec<DomainLemma>,
    pub target: AdaptTarget,
    /// Maximum raw sentences per unit.  Long chapters are split into parts at
    /// this granularity so no single LLM call has to hold too much text.
    #[serde(default = "default_max_sentences_per_unit")]
    pub max_sentences_per_unit: usize,
    /// Index of the unit currently shown in the Raw Source tab.
    #[serde(default)]
    pub selected_unit: usize,
}

/// Default chunk size.  Free paraphrase holds together well up to roughly
/// this many source sentences; past ~50 models start dropping beats and
/// silently compressing whole passages.
pub const DEFAULT_MAX_SENTENCES_PER_UNIT: usize = 40;

fn default_max_sentences_per_unit() -> usize {
    DEFAULT_MAX_SENTENCES_PER_UNIT
}

impl RawSource {
    pub fn new(name: String, units: Vec<RawUnit>) -> Self {
        Self {
            name,
            language: "en".to_string(),
            target_language: "es".to_string(),
            units,
            domain_lemmas: Vec::new(),
            target: AdaptTarget::default(),
            max_sentences_per_unit: DEFAULT_MAX_SENTENCES_PER_UNIT,
            selected_unit: 0,
        }
    }

    pub fn total_sentences(&self) -> usize {
        self.units.iter().map(|u| u.sentences.len()).sum()
    }

    /// Group consecutive units by their owning chapter.
    ///
    /// Promotion emits one `%%META chapter:%%` block per group, so a chapter
    /// that was split into parts comes back out whole.
    pub fn chapter_groups(&self) -> Vec<(String, Vec<usize>)> {
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, unit) in self.units.iter().enumerate() {
            let key = if unit.chapter.is_empty() {
                unit.name.clone()
            } else {
                unit.chapter.clone()
            };
            match groups.last_mut() {
                Some((name, idxs)) if *name == key => idxs.push(i),
                _ => groups.push((key, vec![i])),
            }
        }
        groups
    }

    /// Content fingerprint of the raw text and its chunking.
    ///
    /// A checkpoint is only safe to restore when this matches, because unit
    /// indices and sentence boundaries must line up exactly with the drafts
    /// that were paid for.  Deliberately hand-rolled FNV-1a rather than
    /// `DefaultHasher`, whose output is not stable across Rust releases.
    pub fn fingerprint(&self) -> String {
        let mut h = FNV_OFFSET;
        h = fnv1a(h, self.name.as_bytes());
        h = fnv1a(h, &(self.max_sentences_per_unit as u64).to_le_bytes());
        for unit in &self.units {
            h = fnv1a(h, unit.chapter.as_bytes());
            h = fnv1a(h, &[0xff]);
            for s in &unit.sentences {
                h = fnv1a(h, s.text.as_bytes());
                h = fnv1a(h, &[0x00]);
            }
        }
        format!("{:016x}", h)
    }

    /// How many units already carry a draft — i.e. how much money is banked.
    pub fn drafted_count(&self) -> usize {
        self.units
            .iter()
            .filter(|u| !u.draft.trim().is_empty())
            .count()
    }

    /// Units that a `run` pass would still spend LLM calls on.
    pub fn remaining_count(&self) -> usize {
        self.units
            .iter()
            .filter(|u| !u.is_complete())
            .count()
    }

    /// Resolve a unit selector: a 1-based number, or a case-insensitive
    /// prefix of the unit name.
    pub fn resolve_unit(&self, selector: &str) -> Result<usize, String> {
        let sel = selector.trim();
        if sel.is_empty() {
            return Err("Empty unit selector.".to_string());
        }
        if let Ok(n) = sel.parse::<usize>() {
            if n == 0 || n > self.units.len() {
                return Err(format!(
                    "Unit {} out of range (1..{}).",
                    n,
                    self.units.len()
                ));
            }
            return Ok(n - 1);
        }
        let lower = sel.to_lowercase();
        let matches: Vec<usize> = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.name.to_lowercase().starts_with(&lower))
            .map(|(i, _)| i)
            .collect();
        match matches.len() {
            0 => Err(format!("No unit matches '{}'.", sel)),
            1 => Ok(matches[0]),
            _ => Err(format!("'{}' matches {} units.", sel, matches.len())),
        }
    }

    /// Render the domain-lemma policy as the prompt block injected into the
    /// model's user message.  Returns `None` when the policy is empty.
    pub fn domain_vocab_block(&self) -> Option<String> {
        if self.domain_lemmas.is_empty() {
            return None;
        }
        let mut out = String::from(
            "\n\n--- APPROVED DOMAIN VOCABULARY (book-level policy) ---\n\
             The words below are PRE-APPROVED, book-specific vocabulary (English\n\
             glosses follow '#'). They are thematically central and become familiar\n\
             through repetition. Use them DIRECTLY and naturally wherever they fit:\n\
             \x20 - Do NOT paraphrase around them or invent a periphrasis instead.\n\
             \x20 - Do NOT define or gloss them inline; use them plainly in clear\n\
             \x20   context. Interlinear glossing is added downstream.\n\
             \x20 - They are already scored favorably, so using them plainly is free.\n\n",
        );
        for d in &self.domain_lemmas {
            match &d.gloss {
                Some(g) => out.push_str(&format!("{:<20} # {}\n", d.lemma, g)),
                None => out.push_str(&format!("{}\n", d.lemma)),
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_domain_lemma_forms() {
        let parsed = parse_domain_lemmas(
            "mar                 # sea\nballena=320         # whale\narpon 300\n\n# comment only\n",
        );
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].lemma, "mar");
        assert_eq!(parsed[0].rank, DEFAULT_DOMAIN_RANK);
        assert_eq!(parsed[0].gloss.as_deref(), Some("sea"));
        assert_eq!(parsed[1].lemma, "ballena");
        assert_eq!(parsed[1].rank, 320);
        assert_eq!(parsed[2].lemma, "arpon");
        assert_eq!(parsed[2].rank, 300);
    }

    #[test]
    fn splits_title_line_from_body() {
        let mut unit = RawUnit::new("One".into(), vec![]);
        unit.draft = "\nMoby Dick — Capítulo Uno: Asomos\n\nLlámame Ismael.\nEra invierno.".into();
        assert_eq!(unit.title_line(), Some("Moby Dick — Capítulo Uno: Asomos"));
        assert_eq!(unit.body(), "Llámame Ismael.\nEra invierno.");
    }

    #[test]
    fn resolves_units_by_number_and_prefix() {
        let src = RawSource::new(
            "Book".into(),
            vec![
                RawUnit::new("Loomings".into(), vec![]),
                RawUnit::new("The Carpet-Bag".into(), vec![]),
            ],
        );
        assert_eq!(src.resolve_unit("2").unwrap(), 1);
        assert_eq!(src.resolve_unit("loom").unwrap(), 0);
        assert!(src.resolve_unit("9").is_err());
        assert!(src.resolve_unit("nope").is_err());
    }

    fn sentence(text: &str) -> RawSentence {
        RawSentence {
            id: "R1".into(),
            text: text.into(),
        }
    }

    #[test]
    fn continuation_parts_carry_no_title_line() {
        let mut part2 = RawUnit::new_part("Loomings".into(), 2, 3, vec![]);
        assert_eq!(part2.name, "Loomings (part 2/3)");
        part2.draft = "Siguió caminando.\nEl mar estaba gris.".into();
        // No title is claimed, so nothing is stripped from the body.
        assert_eq!(part2.title_line(), None);
        assert_eq!(part2.body(), "Siguió caminando.\nEl mar estaba gris.");
    }

    #[test]
    fn groups_consecutive_parts_into_one_chapter() {
        let src = RawSource::new(
            "Book".into(),
            vec![
                RawUnit::new_part("One".into(), 1, 2, vec![]),
                RawUnit::new_part("One".into(), 2, 2, vec![]),
                RawUnit::new("Two".into(), vec![]),
            ],
        );
        let groups = src.chapter_groups();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], ("One".to_string(), vec![0, 1]));
        assert_eq!(groups[1], ("Two".to_string(), vec![2]));
    }

    #[test]
    fn fingerprint_tracks_text_and_chunking() {
        let base = RawSource::new(
            "Book".into(),
            vec![RawUnit::new("One".into(), vec![sentence("Call me Ishmael.")])],
        );
        let same = RawSource::new(
            "Book".into(),
            vec![RawUnit::new("One".into(), vec![sentence("Call me Ishmael.")])],
        );
        assert_eq!(base.fingerprint(), same.fingerprint());

        // Drafts are not part of the identity — a checkpoint must still match
        // the freshly imported text it was produced from.
        let mut drafted = same.clone();
        drafted.units[0].draft = "Llámame Ismael.".into();
        assert_eq!(base.fingerprint(), drafted.fingerprint());

        // Changing the text or the chunk size invalidates it.
        let mut edited = base.clone();
        edited.units[0].sentences[0].text = "Call me Ahab.".into();
        assert_ne!(base.fingerprint(), edited.fingerprint());

        let mut rechunked = base.clone();
        rechunked.max_sentences_per_unit = 10;
        assert_ne!(base.fingerprint(), rechunked.fingerprint());
    }

    #[test]
    fn counts_banked_and_remaining_work() {
        let mut src = RawSource::new(
            "Book".into(),
            vec![
                RawUnit::new("One".into(), vec![]),
                RawUnit::new("Two".into(), vec![]),
                RawUnit::new("Three".into(), vec![]),
            ],
        );
        src.units[0].draft = "hecho".into();
        src.units[0].status = AdaptStatus::Passing;
        src.units[1].draft = "a medias".into();
        src.units[1].status = AdaptStatus::Drafted;

        assert_eq!(src.drafted_count(), 2);
        // Only the passing unit is finished; the drafted one still needs a
        // squeeze, so a resumed run pays for two units, not three.
        assert_eq!(src.remaining_count(), 2);
        assert!(src.units[0].is_complete());
        assert!(!src.units[1].is_complete());
    }
}
