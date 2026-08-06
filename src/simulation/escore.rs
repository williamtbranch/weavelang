//! ESCore-style Design Rule Check (DRC) scorer.
//!
//! This is the Rust port of the ESCore `avd_ul_score.py` + `drc_free_flow.ps1`
//! + `write_metrics.ps1` toolchain that drives the graded-reader adaptation
//! loop.  It is used by the Raw Source tab's `adapt` pipeline.
//!
//! Pipeline:
//!  1. Strip `%%META ...%%` directive lines and normalize fancy quotes.
//!  2. Tokenize with the Python/spaCy bridge.
//!  3. Normalize each lemma (`normalize_spanish_lemma`), apply the manual
//!     override table, rescue verb forms spaCy mangles, drop proper nouns.
//!  4. Rank lemmas through the shared wlemma bucket map; lemmas absent from
//!     the frequency list get `UNKNOWN_RANK` (a deliberately conservative
//!     screen) rather than being dropped.
//!  5. Apply the book-level domain-lemma policy: `min(raw_rank, policy_rank)`.
//!  6. Compute the tail-weighted AVD with the 0.2% "Gregor" tally cap, map it
//!     to UL, and compute the coverage-based i-score (the real gate).
//!  7. Check the length ratio against the raw source and build the
//!     worst-offenders table.

use std::collections::{HashMap, HashSet};

use once_cell::sync::Lazy;
use regex::Regex;

use crate::domain::normalization::normalize_spanish_lemma;
use crate::domain::raw_source::{AdaptTarget, DomainLemma, DrcReport, IScore, Offender};
use crate::services::python_bridge::BridgeService;
use crate::simulation::calibrator::get_user_level_from_avd;
use crate::simulation::frequency_manager;

/// Lemmas rarer than this contribute to the offender table and are subject to
/// the "Gregor effect" tally cap.
pub const RARE_RANK_THRESHOLD: u32 = 400;
/// A rare lemma may occupy at most this fraction of running text for AVD.
pub const TALLY_CAP_FRACTION: f64 = 0.002;
/// Rank assigned to lemmas absent from the master frequency list.
pub const UNKNOWN_RANK: u32 = 20_000;
/// Number of offenders reported back to the model.
pub const OFFENDER_LIMIT: usize = 30;

// ---------------------------------------------------------------------------
// Static tables
// ---------------------------------------------------------------------------

/// Manual corrections for lemmas spaCy reliably gets wrong.  The key is the
/// *normalized lemma spaCy produced*; the value is the true dictionary lemma.
static LEMMA_OVERRIDES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        ("salgo", "salir"),
        ("pies", "pie"),
        ("duer", "dormir"),
        ("pondrar", "poner"),
        ("vuelir", "volver"),
    ]
    .into_iter()
    .collect()
});

/// Irregular singular imperative stems (accent-folded) → infinitive.  Only
/// consulted once an enclitic cluster has been stripped, which unambiguously
/// marks the token as a verb.
static IMPERATIVE_BASE_OVERRIDES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        ("di", "decir"),
        ("haz", "hacer"),
        ("pon", "poner"),
        ("ten", "tener"),
        ("sal", "salir"),
        ("ve", "ir"),
        ("ven", "venir"),
        ("se", "ser"),
        ("oye", "oir"),
    ]
    .into_iter()
    .collect()
});

/// Closed set of Spanish enclitic-pronoun clusters, longest first.
static ENCLITIC_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^(.{2,}?)(?:(?:me|te|se|nos|os)(?:lo|la|le|los|las|les)|me|te|se|lo|la|le|nos|os|los|las|les)$",
    )
    .expect("enclitic regex")
});

static WORD_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\p{L}+(?:['\u{2019}-]\p{L}+)*").expect("word regex"));

/// POS tags excluded from scoring outright — proper nouns must never count
/// toward difficulty.
const EXCLUDED_POS: &[&str] = &["PROPN"];
/// POS tags eligible for verb-form rescue.  `NOUN` is deliberately absent so
/// real nouns ending in enclitic-looking syllables are never stripped.
const RESCUE_POS: &[&str] = &["VERB", "AUX", "PROPN"];

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/// Drop `%%META ...%%` directive lines so header metadata never counts as prose.
pub fn strip_directives(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("%%"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replace guillemets and smart quotes with spaces.  A verb immediately after
/// `«` is otherwise mistagged NOUN, which breaks its lemma.
fn normalize_quotes(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{00ab}' | '\u{00bb}' | '\u{2039}' | '\u{203a}' | '\u{201c}' | '\u{201d}'
            | '\u{201e}' | '\u{201f}' => ' ',
            other => other,
        })
        .collect()
}

/// Count prose words the same way the ESCore length gate does.
pub fn count_words(text: &str) -> u32 {
    WORD_RE.find_iter(&strip_directives(text)).count() as u32
}

fn fold_diacritics(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

fn looks_like_infinitive(lemma: &str) -> bool {
    lemma.len() >= 2 && (lemma.ends_with("ar") || lemma.ends_with("er") || lemma.ends_with("ir"))
}

fn rank_of(lemma: &str) -> u32 {
    frequency_manager::rank_of_lemma_string(lemma).unwrap_or(UNKNOWN_RANK)
}

// ---------------------------------------------------------------------------
// Lemma extraction
// ---------------------------------------------------------------------------

/// Lemmatize a single word out of context.  With `require_verb` the result is
/// returned only when spaCy tags it as a verb in isolation, which rejects
/// non-verbs that merely happen to end in -ar/-er/-ir.
fn isolated_lemma(
    bridge: &BridgeService,
    lang: &str,
    word: &str,
    require_verb: bool,
    cache: &mut HashMap<(String, bool), String>,
) -> String {
    let key = (word.to_string(), require_verb);
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let mut result = String::new();
    if let Ok(tokens) = bridge.tokenize(word, lang) {
        for tok in tokens {
            if tok.is_punct || tok.is_space {
                continue;
            }
            if require_verb && tok.pos != "VERB" && tok.pos != "AUX" {
                break;
            }
            result = normalize_spanish_lemma(&tok.lemma);
            break;
        }
    }
    cache.insert(key, result.clone());
    result
}

/// Recover the correct lemma for a verb form spaCy mangled — enclitic
/// imperatives ("diselo", "hazlo", "ponme") and guillemet-mistagged verbs.
///
/// The rescued lemma is accepted **only** when it is a valid infinitive that
/// ranks strictly better than the current lemma, so a genuinely rare word can
/// never be rescued into looking common.
fn rescue_verb_lemma(
    bridge: &BridgeService,
    lang: &str,
    token_text: &str,
    token_pos: &str,
    current_rank: u32,
    cache: &mut HashMap<(String, bool), String>,
) -> Option<String> {
    if !RESCUE_POS.contains(&token_pos) {
        return None;
    }

    let caps = ENCLITIC_RE.captures(token_text);
    let Some(caps) = caps else {
        // Standalone conjugated forms (e.g. "salgo") can also be mangled.
        if token_pos == "VERB" || token_pos == "AUX" {
            let cand = isolated_lemma(bridge, lang, token_text, true, cache);
            if !cand.is_empty() && looks_like_infinitive(&cand) && rank_of(&cand) < current_rank {
                return Some(cand);
            }
        }
        return None;
    };

    let base = fold_diacritics(&caps[1].to_lowercase());
    let cand = match IMPERATIVE_BASE_OVERRIDES.get(base.as_str()) {
        Some(inf) => (*inf).to_string(),
        None => isolated_lemma(bridge, lang, &base, true, cache),
    };
    if cand.is_empty() || !looks_like_infinitive(&cand) {
        return None;
    }
    if rank_of(&cand) < current_rank {
        Some(cand)
    } else {
        None
    }
}

/// Tokenize and screen `text` down to the lemmas that count toward difficulty.
fn lemmas_for_text(
    bridge: &BridgeService,
    lang: &str,
    text: &str,
) -> Result<Vec<String>, String> {
    let tokens = bridge
        .tokenize(text.trim(), lang)
        .map_err(|e| format!("Tokenization failed: {}", e))?;

    let mut cache: HashMap<(String, bool), String> = HashMap::new();
    let mut out = Vec::with_capacity(tokens.len());

    for token in tokens {
        if token.is_punct || token.is_space {
            continue;
        }
        let mut lemma = normalize_spanish_lemma(&token.lemma);
        if let Some(fixed) = LEMMA_OVERRIDES.get(lemma.as_str()) {
            lemma = (*fixed).to_string();
        }
        let rank = if lemma.is_empty() {
            UNKNOWN_RANK
        } else {
            rank_of(&lemma)
        };

        // Only spend effort rescuing tokens that are currently penalized or
        // would otherwise be dropped; correct rankings stay untouched.
        if rank > RARE_RANK_THRESHOLD || token.pos == "PROPN" || lemma.is_empty() {
            if let Some(rescued) =
                rescue_verb_lemma(bridge, lang, &token.text, &token.pos, rank, &mut cache)
            {
                out.push(rescued);
                continue;
            }
        }
        if EXCLUDED_POS.contains(&token.pos.as_str()) || lemma.is_empty() {
            continue;
        }
        out.push(lemma);
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// `(rank, raw_count, lemma)` for every distinct lemma in the text.
type Detail = Vec<(u32, u32, String)>;

struct Tallies {
    /// `(rank, capped_count)` pairs feeding AVD.
    ranked: Vec<(u32, u32)>,
    detail: Detail,
    total_tokens: u32,
    in_freq_list: u32,
    domain_adjusted_tokens: u32,
    domain_adjusted_lemmas: u32,
}

fn build_tallies(lemmas: &[String], policy: &HashMap<String, u32>) -> Tallies {
    let total_tokens = lemmas.len() as u32;
    let tally_cap = ((total_tokens as f64 * TALLY_CAP_FRACTION).ceil() as u32).max(1);

    let mut counts: HashMap<&str, u32> = HashMap::new();
    for lemma in lemmas {
        *counts.entry(lemma.as_str()).or_insert(0) += 1;
    }

    let mut ranked = Vec::with_capacity(counts.len());
    let mut detail: Detail = Vec::with_capacity(counts.len());
    let mut in_freq_list = 0u32;
    let mut domain_adjusted_tokens = 0u32;
    let mut domain_adjusted: HashSet<&str> = HashSet::new();

    for (lemma, count) in counts {
        let raw_rank = rank_of(lemma);
        let mut rank = raw_rank;
        if let Some(policy_rank) = policy.get(lemma) {
            if *policy_rank < rank {
                rank = *policy_rank;
                domain_adjusted_tokens += count;
                domain_adjusted.insert(lemma);
            }
        }
        if raw_rank != UNKNOWN_RANK {
            in_freq_list += count;
        }

        // The Gregor cap applies to the AVD tallies only; `detail` keeps the
        // true occurrence counts so coverage math sees real densities.
        let capped = if rank > RARE_RANK_THRESHOLD && count > tally_cap {
            tally_cap
        } else {
            count
        };
        ranked.push((rank, capped));
        detail.push((rank, count, lemma.to_string()));
    }

    Tallies {
        ranked,
        detail,
        total_tokens,
        in_freq_list,
        domain_adjusted_tokens,
        domain_adjusted_lemmas: domain_adjusted.len() as u32,
    }
}

/// Tail-weighted AVD: `(p85 + 2 * p95) / 3`.  Returns `(avd, p85, p95)`.
fn calculate_avd(ranked: &[(u32, u32)]) -> (f64, f64, f64) {
    let total: u64 = ranked.iter().map(|(_, t)| *t as u64).sum();
    if total == 0 {
        return (0.0, 0.0, 0.0);
    }
    let p85_target = (total as f64 * 0.85).ceil() as u64;
    let p95_target = (total as f64 * 0.95).ceil() as u64;

    let mut sorted: Vec<(u32, u32)> = ranked.to_vec();
    sorted.sort_unstable_by_key(|(rank, _)| *rank);

    let mut cumulative: u64 = 0;
    let mut p85 = 0.0;
    let mut p95 = 0.0;
    let mut p85_found = false;
    for (rank, tally) in sorted {
        cumulative += tally as u64;
        if !p85_found && cumulative >= p85_target {
            p85 = rank as f64;
            p85_found = true;
        }
        if cumulative >= p95_target {
            p95 = rank as f64;
            break;
        }
    }
    ((p85 + 2.0 * p95) / 3.0, p85, p95)
}

/// Coverage-based comprehensible-input score.
///
/// Sort occurrences easiest-first, accumulate until cumulative coverage is
/// reached, and take the master rank at that boundary as `i_rank`: "how hard
/// is the hardest word a reader must know to comprehend `coverage` of the
/// text".  Everything strictly rarer is the unpunished +1 tail, so a rare word
/// repeated many times is *taught* rather than penalized.
fn calculate_i_score(detail: &Detail, coverage: f64) -> IScore {
    let total: u64 = detail.iter().map(|(_, t, _)| *t as u64).sum();
    if total == 0 {
        return IScore {
            coverage,
            i_rank: 0.0,
            i_level: 0.0,
            plus1_tokens: 0,
            plus1_unique: 0,
            plus1_pct: 0.0,
        };
    }

    let target = (total as f64 * coverage).ceil() as u64;
    let mut sorted: Vec<&(u32, u32, String)> = detail.iter().collect();
    sorted.sort_by_key(|(rank, _, _)| *rank);

    let mut cumulative: u64 = 0;
    let mut i_rank = 0.0f64;
    for (rank, tally, _) in sorted {
        cumulative += *tally as u64;
        if cumulative >= target {
            i_rank = *rank as f64;
            break;
        }
    }

    let plus1_tokens: u32 = detail
        .iter()
        .filter(|(rank, _, _)| (*rank as f64) > i_rank)
        .map(|(_, t, _)| *t)
        .sum();
    let plus1_unique = detail
        .iter()
        .filter(|(rank, _, _)| (*rank as f64) > i_rank)
        .count() as u32;

    IScore {
        coverage,
        i_rank,
        i_level: (get_user_level_from_avd(i_rank) * 10.0).round() / 10.0,
        plus1_tokens,
        plus1_unique,
        plus1_pct: (plus1_tokens as f64 / total as f64 * 1000.0).round() / 10.0,
    }
}

/// The rare lemmas that most raise difficulty, rarest first.
fn top_offenders(detail: &Detail, limit: usize) -> Vec<Offender> {
    let mut rare: Vec<&(u32, u32, String)> = detail
        .iter()
        .filter(|(rank, _, _)| *rank > RARE_RANK_THRESHOLD)
        .collect();
    rare.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    rare.into_iter()
        .take(limit)
        .map(|(rank, count, lemma)| Offender {
            rank: *rank,
            count: *count,
            lemma: lemma.clone(),
            impact: *rank as u64 * *count as u64,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the full DRC on an adapted text.
///
/// * `text` — the adapted (target-language) draft.
/// * `source_text` — the raw source, used for the length ratio gate.
/// * `lang` — language code passed to the tokenizer (normally `"es"`).
pub fn score(
    bridge: &BridgeService,
    lang: &str,
    text: &str,
    source_text: &str,
    domain: &[DomainLemma],
    target: &AdaptTarget,
) -> Result<DrcReport, String> {
    if frequency_manager::get_max_rank() == 0 {
        return Err("Frequency list not loaded. Cannot score.".to_string());
    }
    let cleaned = normalize_quotes(&strip_directives(text));
    if cleaned.trim().is_empty() {
        return Err("Draft is empty.".to_string());
    }

    let policy: HashMap<String, u32> = domain
        .iter()
        .map(|d| (d.lemma.clone(), d.rank))
        .collect();

    let lemmas = lemmas_for_text(bridge, lang, &cleaned)?;
    if lemmas.is_empty() {
        return Err("No scoreable word tokens found in draft.".to_string());
    }

    let tallies = build_tallies(&lemmas, &policy);
    let (avd, p85_rank, p95_rank) = calculate_avd(&tallies.ranked);
    let ul_exact = get_user_level_from_avd(avd);
    let i_score = calculate_i_score(&tallies.detail, target.coverage);
    let i_pass = i_score.i_level <= target.i_level_max;

    let submission_words = count_words(text);
    let source_words = count_words(source_text);
    let percent_of_source = if source_words == 0 {
        0.0
    } else {
        (submission_words as f64 / source_words as f64 * 10_000.0).round() / 100.0
    };
    let length_pass = source_words > 0
        && percent_of_source >= target.min_percent
        && percent_of_source <= target.max_percent;

    let unique_lemmas = tallies.detail.len() as u32;

    Ok(DrcReport {
        overall_pass: i_pass && length_pass,
        i_score,
        i_level_max: target.i_level_max,
        i_pass,
        avd: (avd * 100.0).round() / 100.0,
        p85_rank,
        p95_rank,
        ul_exact: (ul_exact * 10.0).round() / 10.0,
        tokens: tallies.total_tokens,
        in_freq_list: tallies.in_freq_list,
        unique_lemmas,
        domain_adjusted_tokens: tallies.domain_adjusted_tokens,
        domain_adjusted_lemmas: tallies.domain_adjusted_lemmas,
        submission_words,
        source_words,
        percent_of_source,
        min_percent: target.min_percent,
        max_percent: target.max_percent,
        length_pass,
        offenders: top_offenders(&tallies.detail, OFFENDER_LIMIT),
    })
}

// ---------------------------------------------------------------------------
// Report rendering
// ---------------------------------------------------------------------------

/// Render a DRC report in the fixed-format table the squeeze prompt expects.
///
/// The format is deliberately stable: the model is told it will receive
/// feedback in exactly this shape, and it reacts to the offender table.
pub fn render_report(report: &DrcReport, title: &str) -> String {
    let mut out = String::new();
    let status = if report.overall_pass { "PASS" } else { "FAIL" };
    let i_status = if report.i_pass { "PASS" } else { "FAIL" };
    let wc_status = if report.length_pass { "PASS" } else { "FAIL" };

    out.push_str(title);
    out.push_str("\nDRC Metrics Log\n\n");
    out.push_str("====================================================================\n");
    out.push_str(&format!("OVERALL STATUS: {}\n", status));
    out.push_str("====================================================================\n\n");

    out.push_str("1. TEXT DIFFICULTY GATES\n");
    out.push_str("------------------------\n");
    out.push_str(&format!("i-score (Primary Gate):    {}\n", i_status));
    out.push_str(&format!(
        "  - iLevel:                {:.1} (limit <= {:.1})\n",
        report.i_score.i_level, report.i_level_max
    ));
    out.push_str(&format!(
        "  - iRank:                 {:.0} ({:.0}% boundary word frequency rank)\n",
        report.i_score.i_rank,
        report.i_score.coverage * 100.0
    ));
    out.push_str(&format!(
        "  - Coverage Level:        {:.2} ({:.0}% of text in core, {:.0}% in +1 tail)\n",
        report.i_score.coverage,
        report.i_score.coverage * 100.0,
        (1.0 - report.i_score.coverage) * 100.0
    ));
    out.push_str(&format!(
        "  - +1 Tail Details:       {:.1}% of running text is unpunished vocabulary\n",
        report.i_score.plus1_pct
    ));
    out.push_str(&format!(
        "    * Rare Tokens:         {}\n",
        report.i_score.plus1_tokens
    ));
    out.push_str(&format!(
        "    * Unique Rare Lemmas:  {}\n\n",
        report.i_score.plus1_unique
    ));

    out.push_str(&format!(
        "User Level (Info-Only):    UL{} (not a gate)\n",
        report.ul_floor()
    ));
    out.push_str(&format!(
        "  - UL (exact decimal):    {:.1}\n",
        report.ul_exact
    ));
    out.push_str(&format!(
        "  - AVD (tail-weighted):   {:.2}\n",
        report.avd
    ));
    out.push_str(&format!("  - p85 Rank:              {:.0}\n", report.p85_rank));
    out.push_str(&format!("  - p95 Rank:              {:.0}\n", report.p95_rank));
    if report.domain_adjusted_lemmas > 0 {
        out.push_str(&format!(
            "  - Domain policy applied: {} lemma(s), {} token(s)\n",
            report.domain_adjusted_lemmas, report.domain_adjusted_tokens
        ));
    }
    out.push('\n');

    out.push_str("2. LENGTH GATE\n");
    out.push_str("--------------\n");
    out.push_str(&format!("Word Count Ratio:          {}\n", wc_status));
    out.push_str(&format!(
        "  - Adapted Submission:    {} words (excluding metadata)\n",
        report.submission_words
    ));
    out.push_str(&format!(
        "  - Raw Source:            {} words\n",
        report.source_words
    ));
    out.push_str(&format!(
        "  - Percentage:            {:.2}% of source (allowed range: {:.1}% - {:.1}%)\n\n",
        report.percent_of_source, report.min_percent, report.max_percent
    ));

    out.push_str("3. SIMPLIFICATION TARGETS\n");
    out.push_str("-------------------------\n");
    out.push_str(&format!(
        "Worst {} offenders - rare vocabulary raising the i-score / AVD, rarest first.\n",
        OFFENDER_LIMIT
    ));
    out.push_str("Replace or simplify these to lower difficulty. No replacement is\n");
    out.push_str("prescribed: choose one that fits the sentence and the story's voice.\n");
    out.push_str("(rank >10k = not in the frequency list; count = occurrences in this text.)\n");
    out.push_str(&format!(
        "    {:>6}  {:>5}  {}\n",
        "rank", "count", "lemma"
    ));
    if report.offenders.is_empty() {
        out.push_str("    * none\n");
    } else {
        for o in &report.offenders {
            let rank_label = if o.rank >= UNKNOWN_RANK {
                ">10k".to_string()
            } else {
                o.rank.to_string()
            };
            out.push_str(&format!(
                "    {:>6}  {:>5}  {}\n",
                rank_label, o.count, o.lemma
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_meta_directives_from_word_count() {
        let text = "%%META chapter: One%%\nHola mundo cruel.\n";
        assert_eq!(count_words(text), 3);
    }

    #[test]
    fn counts_apostrophes_as_one_word() {
        assert_eq!(count_words("don't stop"), 2);
    }

    #[test]
    fn avd_is_tail_weighted() {
        // 90 tokens at rank 10, 10 tokens at rank 1000.
        let ranked = vec![(10u32, 90u32), (1000u32, 10u32)];
        let (avd, p85, p95) = calculate_avd(&ranked);
        assert_eq!(p85, 10.0);
        assert_eq!(p95, 1000.0);
        assert!((avd - (10.0 + 2000.0) / 3.0).abs() < 1e-9);
    }

    #[test]
    fn i_score_leaves_rare_tail_unpunished() {
        let detail: Detail = vec![
            (10, 90, "el".to_string()),
            (9000, 10, "arponero".to_string()),
        ];
        let i = calculate_i_score(&detail, 0.85);
        // The 85% boundary lands inside the common word, so the rare tail is
        // reported but does not set the level.
        assert_eq!(i.i_rank, 10.0);
        assert_eq!(i.plus1_tokens, 10);
        assert_eq!(i.plus1_unique, 1);
        assert!((i.plus1_pct - 10.0).abs() < 1e-9);
    }

    #[test]
    fn offenders_sort_rarest_first() {
        let detail: Detail = vec![
            (500, 9, "lejos".to_string()),
            (8441, 1, "asomos".to_string()),
            (100, 50, "el".to_string()),
        ];
        let offenders = top_offenders(&detail, 30);
        assert_eq!(offenders.len(), 2);
        assert_eq!(offenders[0].lemma, "asomos");
        assert_eq!(offenders[1].lemma, "lejos");
        assert_eq!(offenders[1].impact, 4500);
    }
}
