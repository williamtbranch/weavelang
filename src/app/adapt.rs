//! Engine handlers for the raw-source adaptation loop (`adapt ...` commands).
//!
//! This is the weavelang port of the ESCore "fable harness": import raw
//! English text, adapt it to a target difficulty with an LLM under DRC
//! supervision, then promote the passing drafts into the Source tab as
//! generated Spanish source text.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::app::engine::Engine;
use crate::domain::raw_source::{
    parse_domain_lemmas, AdaptStatus, RawSentence, RawSource, RawUnit,
    DEFAULT_MAX_SENTENCES_PER_UNIT,
};
use crate::services::adapt_worker::{self, AdaptJobConfig, AdaptMode};
use crate::simulation::escore;

/// Config stage that supplies the model aliases for adaptation.
const ADAPT_STAGE: &str = "AdaptRawSource";

/// On-disk checkpoint of an adaptation run.
///
/// Adapting a full-length book is hours of wall time and real money, so every
/// finished unit is flushed to disk immediately.  An interruption — crash,
/// power cut, closing the window — costs at most the one unit that was in
/// flight, never the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdaptCheckpoint {
    /// Schema version, so a future format change can refuse an old file
    /// rather than silently mis-restoring paid-for drafts.
    version: u32,
    /// Content fingerprint of the raw text; must match to restore.
    fingerprint: String,
    /// Unix seconds, for the "restored from ..." message.
    saved_at: u64,
    raw: RawSource,
}

const CHECKPOINT_VERSION: u32 = 1;

/// Explicit `%%META chapter: Name%%` directive in a raw file.
static META_CHAPTER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^\s*%%META\s+chapter:\s*(.+?)\s*%%\s*$").expect("valid regex"));

/// Spelled-out ordinals/cardinals accepted after a structural word.
const WORD_NUMBERS: [&str; 26] = [
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
    "twelve", "first", "second", "third", "fourth", "fifth", "uno", "dos", "tres", "cuatro",
    "cinco", "seis", "siete", "ocho", "nueve",
];

/// Recognise a chapter heading line and return its display name.
///
/// Deliberately strict: a structural word (`Chapter`, `Part`, ...) must be
/// followed by a number, a Roman numeral, or a spelled-out number, so prose
/// like "Part of the crew went ashore." is not mistaken for a heading.
fn heading_name(line: &str) -> Option<String> {
    if let Some(caps) = META_CHAPTER_RE.captures(line) {
        return Some(caps[1].trim().to_string());
    }

    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.split_whitespace().count() > 12 {
        return None;
    }
    let strip = |w: &str| {
        w.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };

    let mut words = trimmed.split_whitespace();
    let first = strip(words.next()?);
    match first.as_str() {
        "prologue" | "epilogue" | "prólogo" | "prologo" | "epílogo" | "epilogo" => {
            Some(trimmed.to_string())
        }
        "chapter" | "book" | "part" | "capítulo" | "capitulo" | "parte" => {
            let second = strip(words.next()?);
            if second.is_empty() {
                return None;
            }
            let numbered = second.chars().all(|c| c.is_ascii_digit())
                || second.chars().all(|c| "ivxlcdm".contains(c))
                || WORD_NUMBERS.contains(&second.as_str());
            numbered.then(|| trimmed.to_string())
        }
        _ => None,
    }
}

impl Engine {
    // -----------------------------------------------------------------
    // Import
    // -----------------------------------------------------------------

    /// `import raw <path>` — load raw text into the Raw Source tab.
    ///
    /// This deliberately does **not** touch `state.document`: raw source is a
    /// parallel workspace whose `R1..Rn` indexing has no relation to the
    /// `S1..Sn` indexing produced later by `adapt promote`.
    pub(crate) fn execute_import_raw(
        &mut self,
        path: String,
        chunk: Option<usize>,
        fresh: bool,
    ) -> Result<String, String> {
        let resolved = self.resolve_path(&path);
        let content = fs::read_to_string(&resolved).map_err(|e| e.to_string())?;
        if content.trim().is_empty() {
            return Err("Raw source file is empty.".to_string());
        }

        let name = resolved
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Raw")
            .to_string();

        let max_sentences = chunk.unwrap_or(DEFAULT_MAX_SENTENCES_PER_UNIT).max(1);
        let units = split_into_units(&content, &name, max_sentences);
        if units.is_empty() {
            return Err("Raw source file produced no text.".to_string());
        }

        let mut raw = RawSource::new(name.clone(), units);
        raw.max_sentences_per_unit = max_sentences;
        // The raw text is the base language; the adaptation is written in the
        // project's target language.
        raw.language = self.state.project_languages.0.clone();
        raw.target_language = self.state.project_languages.1.clone();
        if raw.language == raw.target_language {
            // Source-is-target is the *output* state, not the input state.
            // A raw English book adapted into Spanish still starts as en->es.
            raw.language = "en".to_string();
        }

        let unit_count = raw.units.len();
        let chapter_count = raw.chapter_groups().len();
        let sentence_count = raw.total_sentences();

        // Resume: if a checkpoint for this exact text exists, adopt its
        // drafts. There is nothing to clobber — these units are brand new —
        // and silently re-buying finished chapters is the expensive mistake.
        let checkpoint_path = self.adapt_checkpoint_path(&name);
        let mut resume_note = String::new();
        if !fresh {
            if let Ok(checkpoint) = load_checkpoint(&checkpoint_path) {
                if checkpoint.fingerprint == raw.fingerprint() {
                    let drafted = checkpoint.raw.drafted_count();
                    if drafted > 0 {
                        resume_note = format!(
                            "\n  RESUMED from checkpoint saved {}: {} of {} unit(s) already \
                             drafted, {} still to do.\n  \
                             'adapt run all' skips finished units without spending LLM calls. \
                             Use --fresh to start over.",
                            describe_age(checkpoint.saved_at),
                            drafted,
                            checkpoint.raw.units.len(),
                            checkpoint.raw.remaining_count()
                        );
                        raw = checkpoint.raw;
                    }
                } else if checkpoint.raw.drafted_count() > 0 {
                    resume_note = format!(
                        "\n  NOTE: a checkpoint exists at {} but the raw text or chunk size \
                         changed, so its drafts no longer line up with these units. It was \
                         left untouched.",
                        checkpoint_path.display()
                    );
                }
            }
        }

        self.state.raw_source = Some(raw);
        self.state.show_raw_source_tab = true;

        Ok(format!(
            "Imported raw source '{}': {} chapter(s) split into {} unit(s), \
             {} sentence(s) (max {} per unit).{}\n  \
             Next: 'adapt domain <path>' (optional), then 'adapt run all'.",
            name, chapter_count, unit_count, sentence_count, max_sentences, resume_note
        ))
    }

    // -----------------------------------------------------------------
    // Configuration
    // -----------------------------------------------------------------

    /// `adapt domain <path>` — load the approved-vocabulary policy.
    pub(crate) fn execute_adapt_domain(&mut self, path: String) -> Result<String, String> {
        let resolved = self.resolve_path(&path);
        let content = fs::read_to_string(&resolved).map_err(|e| e.to_string())?;
        let lemmas = parse_domain_lemmas(&content);
        if lemmas.is_empty() {
            return Err("No domain lemmas parsed from that file.".to_string());
        }
        let raw = self.raw_source_mut()?;
        let count = lemmas.len();
        raw.domain_lemmas = lemmas;
        Ok(format!(
            "Loaded {} approved domain lemma(s). Re-score with 'adapt score all'.",
            count
        ))
    }

    /// `adapt set <key> <value>` — adjust a gate threshold.
    pub(crate) fn execute_adapt_set(&mut self, key: String, value: String) -> Result<String, String> {
        let key_lc = key.to_lowercase();

        // `chunk` is structural, not a gate: it re-splits the raw text.
        if matches!(key_lc.as_str(), "chunk" | "chunk_size" | "sentences") {
            let n: usize = value.parse().map_err(|_| "Expected an integer.".to_string())?;
            return self.rechunk(n);
        }

        let raw = self.raw_source_mut()?;
        let t = &mut raw.target;
        match key_lc.as_str() {
            "coverage" => {
                let v: f64 = value.parse().map_err(|_| "Expected a number.".to_string())?;
                // Accept either 0.85 or 85.
                let v = if v > 1.0 { v / 100.0 } else { v };
                if !(0.5..=0.99).contains(&v) {
                    return Err("coverage must be between 0.50 and 0.99.".to_string());
                }
                t.coverage = v;
            }
            "ilevel" | "i_level" | "level" => {
                t.i_level_max = value.parse().map_err(|_| "Expected a number.".to_string())?;
            }
            "min" | "min_percent" => {
                t.min_percent = value.parse().map_err(|_| "Expected a number.".to_string())?;
            }
            "max" | "max_percent" => {
                t.max_percent = value.parse().map_err(|_| "Expected a number.".to_string())?;
            }
            "passes" | "squeezes" => {
                t.max_squeeze_passes = value.parse().map_err(|_| "Expected an integer.".to_string())?;
            }
            "gain" | "min_gain" => {
                t.min_gain = value.parse().map_err(|_| "Expected a number.".to_string())?;
            }
            other => {
                return Err(format!(
                    "Unknown gate '{other}'. Valid: coverage, ilevel, min, max, passes, gain, chunk."
                ))
            }
        }
        Ok(format!(
            "Gates: iLevel <= {:.1} at {:.0}% coverage, length {:.0}-{:.0}% of source, \
             max {} squeeze pass(es), min gain {:.2}.",
            t.i_level_max,
            t.coverage * 100.0,
            t.min_percent,
            t.max_percent,
            t.max_squeeze_passes,
            t.min_gain
        ))
    }

    /// Re-split the raw text at a new chunk size.
    ///
    /// Refuses when drafts exist: re-chunking changes unit boundaries, so the
    /// existing drafts would no longer correspond to their source.
    fn rechunk(&mut self, max_sentences: usize) -> Result<String, String> {
        if max_sentences == 0 {
            return Err("Chunk size must be at least 1.".to_string());
        }
        let raw = self.raw_source_mut()?;
        if raw.units.iter().any(|u| !u.draft.trim().is_empty()) {
            return Err(
                "Cannot re-chunk: drafts already exist. Re-import the raw file to change \
                 the chunk size, or keep the current units."
                    .to_string(),
            );
        }

        // Rebuild the chapter text from the existing units and re-split it.
        let mut rebuilt = String::new();
        for (chapter, idxs) in raw.chapter_groups() {
            rebuilt.push_str(&format!("%%META chapter: {}%%\n", chapter));
            for i in idxs {
                for s in &raw.units[i].sentences {
                    rebuilt.push_str(&s.text);
                    rebuilt.push('\n');
                }
            }
            rebuilt.push('\n');
        }

        let units = split_into_units(&rebuilt, &raw.name, max_sentences);
        if units.is_empty() {
            return Err("Re-chunking produced no units.".to_string());
        }
        raw.units = units;
        raw.max_sentences_per_unit = max_sentences;
        raw.selected_unit = 0;
        Ok(format!(
            "Re-chunked into {} unit(s) at max {} sentence(s) each.",
            raw.units.len(),
            max_sentences
        ))
    }

    // -----------------------------------------------------------------
    // Scoring / reporting
    // -----------------------------------------------------------------

    /// `adapt score [unit]` — re-run the DRC locally, no LLM involved.
    pub(crate) fn execute_adapt_score(&mut self, unit: Option<String>) -> Result<String, String> {
        let bridge = self
            .state
            .bridge
            .clone()
            .ok_or("Python bridge not configured — scoring needs the lemmatizer.")?;
        let targets = self.resolve_units(&unit)?;
        let raw = self.raw_source_mut()?;
        let lang = raw.target_language.clone();
        let policy = raw.domain_lemmas.clone();
        let gates = raw.target;

        let mut out = String::new();
        let mut scored = 0;
        for idx in targets {
            let source_text = raw.units[idx].source_text();
            if raw.units[idx].draft.trim().is_empty() {
                out.push_str(&format!("  {} — no draft yet.\n", raw.units[idx].name));
                continue;
            }
            match escore::score(
                &bridge,
                &lang,
                &raw.units[idx].draft,
                &source_text,
                &policy,
                &gates,
            ) {
                Ok(report) => {
                    raw.units[idx].status = if report.overall_pass {
                        AdaptStatus::Passing
                    } else if raw.units[idx].status == AdaptStatus::Floor {
                        AdaptStatus::Floor
                    } else {
                        AdaptStatus::Drafted
                    };
                    out.push_str(&format!(
                        "  {} v{} — {} iLevel {:.1} / {:.1}, {} words ({:.1}%)\n",
                        raw.units[idx].name,
                        raw.units[idx].version,
                        if report.overall_pass { "PASS" } else { "FAIL" },
                        report.i_score.i_level,
                        report.i_level_max,
                        report.submission_words,
                        report.percent_of_source,
                    ));
                    raw.units[idx].report = Some(report);
                    scored += 1;
                }
                Err(e) => out.push_str(&format!("  {} — scoring failed: {}\n", raw.units[idx].name, e)),
            }
        }
        Ok(format!("Scored {} unit(s).\n{}", scored, out))
    }

    /// `adapt status` — one line per unit.
    pub(crate) fn execute_adapt_status_report(&self) -> Result<String, String> {
        let raw = self.raw_source()?;
        let mut out = format!(
            "Raw source '{}' — {} chapter(s) in {} unit(s) (max {} sentence(s) each), {}->{}\n\
             Gates: iLevel <= {:.1} at {:.0}% coverage, length {:.0}-{:.0}% of source\n\
             Approved domain lemmas: {}\n",
            raw.name,
            raw.chapter_groups().len(),
            raw.units.len(),
            raw.max_sentences_per_unit,
            raw.language,
            raw.target_language,
            raw.target.i_level_max,
            raw.target.coverage * 100.0,
            raw.target.min_percent,
            raw.target.max_percent,
            raw.domain_lemmas.len(),
        );
        out.push_str("  #  status      v  iLevel  UL  words   %src  unit\n");
        for (i, u) in raw.units.iter().enumerate() {
            match &u.report {
                Some(r) => out.push_str(&format!(
                    "  {:<2} {:<10} {:<2} {:>6.1} {:>3} {:>6} {:>6.1}  {}\n",
                    i + 1,
                    u.status.label(),
                    u.version,
                    r.i_score.i_level,
                    r.ul_floor(),
                    r.submission_words,
                    r.percent_of_source,
                    u.name,
                )),
                None => out.push_str(&format!(
                    "  {:<2} {:<10} {:<2} {:>6} {:>3} {:>6} {:>6}  {}\n",
                    i + 1,
                    u.status.label(),
                    u.version,
                    "-",
                    "-",
                    "-",
                    "-",
                    u.name,
                )),
            }
            if let Some(err) = &u.last_error {
                out.push_str(&format!("      error: {}\n", err));
            }
        }

        let checkpoint = self.adapt_checkpoint_path(&raw.name);
        out.push_str(&format!(
            "\n{} of {} unit(s) drafted, {} still to do.\nCheckpoint: {}\n",
            raw.drafted_count(),
            raw.units.len(),
            raw.remaining_count(),
            match load_checkpoint(&checkpoint) {
                Ok(c) => format!(
                    "{} (saved {}, {} unit(s))",
                    checkpoint.display(),
                    describe_age(c.saved_at),
                    c.raw.drafted_count()
                ),
                Err(_) => format!("{} (not written yet)", checkpoint.display()),
            }
        ));
        Ok(out)
    }

    /// `adapt report [unit]` — the full DRC report including offenders.
    pub(crate) fn execute_adapt_report(&self, unit: Option<String>) -> Result<String, String> {
        let targets = self.resolve_units(&unit)?;
        let raw = self.raw_source()?;
        let mut out = String::new();
        for idx in targets {
            let u = &raw.units[idx];
            match &u.report {
                Some(r) => out.push_str(&escore::render_report(r, &u.name)),
                None => out.push_str(&format!("{} — no report. Run 'adapt score'.\n", u.name)),
            }
            out.push('\n');
        }
        Ok(out)
    }

    /// `adapt revert [unit]` — undo the last draft/squeeze pass.
    pub(crate) fn execute_adapt_revert(&mut self, unit: Option<String>) -> Result<String, String> {
        let targets = self.resolve_units(&unit)?;
        let raw = self.raw_source_mut()?;
        let mut reverted = 0;
        for idx in targets {
            let u = &mut raw.units[idx];
            if let Some(prev) = u.history.pop() {
                u.draft = prev;
                u.version = u.version.saturating_sub(1).max(1);
                u.report = None;
                u.status = AdaptStatus::Drafted;
                reverted += 1;
            }
        }
        if reverted == 0 {
            return Err("Nothing to revert.".to_string());
        }
        Ok(format!(
            "Reverted {} unit(s). Re-score with 'adapt score'.",
            reverted
        ))
    }

    /// `adapt cancel` — ask the running job to stop.
    pub(crate) fn execute_adapt_cancel(&mut self) -> Result<String, String> {
        let Some(job) = &self.state.adapt_job else {
            return Err("No adaptation job is running.".to_string());
        };
        if let Some(flag) = &self.state.adapt_cancel {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if let Ok(mut guard) = job.lock() {
            guard.cancel_requested = true;
        }
        Ok("Cancellation requested. The current unit will finish first.".to_string())
    }

    // -----------------------------------------------------------------
    // The LLM loop
    // -----------------------------------------------------------------

    /// `adapt draft|squeeze|run [unit]` — spawn the background job.
    pub(crate) fn execute_adapt_job(
        &mut self,
        mode: AdaptMode,
        unit: Option<String>,
    ) -> Result<String, String> {
        if self.state.adapt_job.is_some() {
            return Err("An adaptation job is already running. Use 'adapt cancel' first.".to_string());
        }

        let targets = self.resolve_units(&unit)?;
        let bridge = self
            .state
            .bridge
            .clone()
            .ok_or("Python bridge not configured — the DRC needs the lemmatizer.")?;
        let llm = self.state.llm.clone().ok_or("LLM service not configured.")?;
        let prompts = self.state.prompts.clone().ok_or("Prompt manager not configured.")?;
        let logger = self.state.logger.clone().ok_or("LLM logger not configured.")?;
        let config = self.state.config.as_ref().ok_or("Config not loaded.")?;

        let stage = config.get_stage_config(ADAPT_STAGE).ok_or_else(|| {
            format!(
                "Stage '{}' not found in config.toml. Add:\n\
                 \n    [stages.{}]\n    primary_model = \"gemini-pro\"\n    \
                 fallback_model = \"sonnet\"\n    batch_size_in_items = 1\n",
                ADAPT_STAGE, ADAPT_STAGE
            )
        })?;
        let model = stage.primary_model.clone();
        let fallback_model = stage.fallback_model.clone();
        if config.get_model_config(&model).is_none() {
            return Err(format!(
                "Model alias '{}' is not defined in the [models] section of config.toml.",
                model
            ));
        }

        let raw = self.raw_source()?;
        let domain_block = raw.domain_vocab_block();
        let units: Vec<(usize, RawUnit)> =
            targets.iter().map(|i| (*i, raw.units[*i].clone())).collect();

        // Continuity: hand each unit the previous unit's finished draft.
        let prior_drafts: Vec<(usize, String)> = targets
            .iter()
            .filter(|i| **i > 0)
            .filter_map(|i| {
                let prev = &raw.units[*i - 1];
                (!prev.draft.trim().is_empty()).then(|| (*i, prev.draft.clone()))
            })
            .collect();

        let job_config = AdaptJobConfig {
            prompts,
            llm,
            logger,
            bridge,
            model: model.clone(),
            fallback_model,
            source_language: raw.language.clone(),
            target_language: raw.target_language.clone(),
            domain_lemmas: raw.domain_lemmas.clone(),
            domain_block,
            target: raw.target,
            mode,
            units,
            prior_drafts,
        };

        let count = job_config.units.len();
        let (state, cancel) = adapt_worker::spawn_adapt_job(job_config);
        self.state.adapt_job = Some(state);
        self.state.adapt_cancel = Some(cancel);

        Ok(format!(
            "Started adaptation ({:?}) on {} unit(s) with model '{}'.\n  \
             Watch progress in the Raw Source tab, or 'adapt status' when done.",
            mode, count, model
        ))
    }

    /// Drain finished units from a running job into the raw source.
    /// Called every frame by the GUI and after each terminal command.
    /// Returns any new log lines plus the final message once complete.
    pub fn poll_adapt_job(&mut self) -> (Vec<String>, Option<String>) {
        let Some(job) = self.state.adapt_job.clone() else {
            return (Vec::new(), None);
        };

        let (new_results, finished, result_message, log_len) = {
            let Ok(mut guard) = job.lock() else {
                return (Vec::new(), None);
            };
            let applied = guard.applied;
            let pending: Vec<_> = guard.results[applied..].to_vec();
            guard.applied = guard.results.len();
            (
                pending,
                guard.finished,
                guard.result_message.clone(),
                guard.log.len(),
            )
        };

        let applied_any = !new_results.is_empty();

        if let Some(raw) = self.state.raw_source.as_mut() {
            for result in new_results {
                if result.unit_index < raw.units.len() {
                    raw.units[result.unit_index] = result.unit;
                }
            }
        }

        let mut lines = Vec::new();

        // Bank the gains. A unit costs several LLM calls, so flushing after
        // each one is cheap relative to the work it protects.
        if applied_any {
            match self.save_adapt_checkpoint() {
                Ok(path) => {
                    if let Some(raw) = self.state.raw_source.as_ref() {
                        lines.push(format!(
                            "  checkpoint: {} of {} unit(s) saved to {}",
                            raw.drafted_count(),
                            raw.units.len(),
                            path.display()
                        ));
                    }
                }
                Err(e) => lines.push(format!(
                    "  WARNING: checkpoint failed ({e}) — progress is only in memory."
                )),
            }
        }

        if let Ok(guard) = job.lock() {
            while self.state.adapt_log_seen < log_len && self.state.adapt_log_seen < guard.log.len()
            {
                lines.push(guard.log[self.state.adapt_log_seen].clone());
                self.state.adapt_log_seen += 1;
            }
        }

        if finished {
            self.state.adapt_job = None;
            self.state.adapt_cancel = None;
            self.state.adapt_log_seen = 0;
            return (lines, result_message);
        }
        (lines, None)
    }

    // -----------------------------------------------------------------
    // Promotion
    // -----------------------------------------------------------------

    /// `adapt promote [--force]` — assemble the drafts into a source text
    /// file and run it through the normal source-import path.
    ///
    /// The generated file carries `source_language == target_language`, so
    /// `AppState::source_is_target()` becomes true, and one
    /// `%%META chapter:%%` directive per unit so chapters are auto-created.
    pub(crate) fn execute_adapt_promote(&mut self, force: bool) -> Result<String, String> {
        let (assembled, book_name, blocked, promoted, part_total) = {
            let raw = self.raw_source()?;
            let mut blocked: Vec<String> = Vec::new();
            let mut promoted = 0usize;
            let mut part_total = 0usize;

            let target_lang = raw.target_language.clone();
            let mut out = String::new();
            out.push_str(&format!("%%META book_name: {}%%\n", raw.name));
            out.push_str(&format!("%%META source_language: {}%%\n", target_lang));
            out.push_str(&format!("%%META target_language: {}%%\n", target_lang));
            out.push('\n');

            // One %%META chapter:%% block per chapter — all its parts are
            // concatenated, so the import-time split is invisible downstream.
            for (chapter, idxs) in raw.chapter_groups() {
                let missing: Vec<String> = idxs
                    .iter()
                    .filter(|i| raw.units[**i].draft.trim().is_empty())
                    .map(|i| raw.units[*i].name.clone())
                    .collect();
                if !missing.is_empty() {
                    // A chapter with a hole would splice unrelated prose
                    // together, so it is skipped even under --force.
                    blocked.push(format!("{} — no draft for {}", chapter, missing.join(", ")));
                    continue;
                }
                let failing: Vec<String> = idxs
                    .iter()
                    .filter(|i| raw.units[**i].status != AdaptStatus::Passing)
                    .map(|i| format!("{} ({})", raw.units[*i].name, raw.units[*i].status.label()))
                    .collect();
                if !failing.is_empty() && !force {
                    blocked.push(format!(
                        "{} — {} (use --force)",
                        chapter,
                        failing.join(", ")
                    ));
                    continue;
                }

                let title = raw.units[idxs[0]]
                    .title_line()
                    .unwrap_or(chapter.as_str())
                    .to_string();
                out.push_str(&format!(
                    "%%META chapter: {}%%\n",
                    sanitize_chapter_name(&title)
                ));
                for i in &idxs {
                    let unit = &raw.units[*i];
                    let body = unit.body();
                    let body = if body.trim().is_empty() {
                        unit.draft.trim().to_string()
                    } else {
                        body
                    };
                    out.push_str(body.trim());
                    out.push_str("\n\n");
                }
                promoted += 1;
                part_total += idxs.len();
            }
            (out, raw.name.clone(), blocked, promoted, part_total)
        };

        if promoted == 0 {
            return Err(format!(
                "Nothing to promote.\n  {}",
                if blocked.is_empty() {
                    "No units have drafts.".to_string()
                } else {
                    blocked.join("\n  ")
                }
            ));
        }

        let out_path = self.adapt_output_path(&book_name);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&out_path, &assembled).map_err(|e| e.to_string())?;

        // Keep the raw source around — promotion is repeatable.
        let raw_backup = self.state.raw_source.clone();
        let import_summary = self.execute_import_source(out_path.display().to_string())?;
        self.state.raw_source = raw_backup;
        self.state.show_raw_source_tab = true;

        let mut summary = format!(
            "Promoted {} chapter(s) from {} unit(s) to source text.\n  Generated: {}\n  {}",
            promoted,
            part_total,
            out_path.display(),
            import_summary.replace('\n', "\n  "),
        );
        summary.push_str(&format!(
            "\n  source_is_target: {}",
            self.state.source_is_target()
        ));
        if !blocked.is_empty() {
            summary.push_str("\n  skipped:");
            for b in &blocked {
                summary.push_str(&format!("\n    {}", b));
            }
        }
        Ok(summary)
    }

    fn adapt_output_path(&self, book_name: &str) -> PathBuf {
        let file = format!("{}_adapted_source.txt", sanitize_file_stem(book_name));
        match &self.state.config {
            Some(cfg) => cfg.content_project_dir_path().join(file),
            None => PathBuf::from(file),
        }
    }

    // -----------------------------------------------------------------
    // Checkpointing
    // -----------------------------------------------------------------

    /// Where the resume checkpoint for `book_name` lives.
    ///
    /// Derived from the book name rather than the project file so that
    /// `import raw` can find it before anything else is loaded.
    pub(crate) fn adapt_checkpoint_path(&self, book_name: &str) -> PathBuf {
        let file = format!("{}_adapt_checkpoint.json", sanitize_file_stem(book_name));
        match &self.state.config {
            Some(cfg) => cfg.content_project_dir_path().join(file),
            None => PathBuf::from(file),
        }
    }

    /// Flush the current raw source to its checkpoint file.
    ///
    /// Written to a temporary file and renamed into place, so a crash during
    /// the write cannot destroy the previous checkpoint.
    pub(crate) fn save_adapt_checkpoint(&self) -> Result<PathBuf, String> {
        let raw = self.raw_source()?;
        let path = self.adapt_checkpoint_path(&raw.name);
        let checkpoint = AdaptCheckpoint {
            version: CHECKPOINT_VERSION,
            fingerprint: raw.fingerprint(),
            saved_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            raw: raw.clone(),
        };
        let bytes = serde_json::to_vec(&checkpoint)
            .map_err(|e| format!("Checkpoint serialisation failed: {e}"))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &bytes).map_err(|e| format!("Checkpoint write failed: {e}"))?;
        fs::rename(&tmp, &path).map_err(|e| format!("Checkpoint rename failed: {e}"))?;
        Ok(path)
    }

    /// `adapt checkpoint` — flush now, and say where it went.
    pub(crate) fn execute_adapt_checkpoint(&mut self) -> Result<String, String> {
        let path = self.save_adapt_checkpoint()?;
        let raw = self.raw_source()?;
        Ok(format!(
            "Checkpoint saved: {}\n  {} of {} unit(s) drafted, {} still to do.",
            path.display(),
            raw.drafted_count(),
            raw.units.len(),
            raw.remaining_count()
        ))
    }

    /// `adapt restore [path]` — reload a checkpoint over the current raw source.
    pub(crate) fn execute_adapt_restore(&mut self, path: Option<String>) -> Result<String, String> {
        if self.state.adapt_job.is_some() {
            return Err("An adaptation job is running. Use 'adapt cancel' first.".to_string());
        }
        let target = match path {
            Some(p) => self.resolve_path(&p),
            None => {
                let raw = self.raw_source()?;
                self.adapt_checkpoint_path(&raw.name)
            }
        };
        let checkpoint = load_checkpoint(&target)?;

        // Only refuse on a mismatch when there is something to mismatch with.
        if let Some(current) = self.state.raw_source.as_ref() {
            let current_fp = current.fingerprint();
            if current_fp != checkpoint.fingerprint {
                return Err(format!(
                    "Checkpoint does not match the loaded raw source (fingerprint {} vs {}).\n  \
                     The raw text or chunk size changed, so the saved drafts no longer line up \
                     with these units. Re-import the original raw file to resume.",
                    checkpoint.fingerprint, current_fp
                ));
            }
        }

        let drafted = checkpoint.raw.drafted_count();
        let remaining = checkpoint.raw.remaining_count();
        let total = checkpoint.raw.units.len();
        self.state.raw_source = Some(checkpoint.raw);
        self.state.show_raw_source_tab = true;
        Ok(format!(
            "Restored checkpoint from {}.\n  {} of {} unit(s) drafted, {} still to do.\n  \
             'adapt run all' will skip finished units without spending LLM calls.",
            target.display(),
            drafted,
            total,
            remaining
        ))
    }

    // -----------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------

    fn raw_source(&self) -> Result<&RawSource, String> {
        self.state
            .raw_source
            .as_ref()
            .ok_or_else(|| "No raw source loaded. Use 'import raw <path>' first.".to_string())
    }

    fn raw_source_mut(&mut self) -> Result<&mut RawSource, String> {
        self.state
            .raw_source
            .as_mut()
            .ok_or_else(|| "No raw source loaded. Use 'import raw <path>' first.".to_string())
    }

    /// `None` means every unit.
    fn resolve_units(&self, selector: &Option<String>) -> Result<Vec<usize>, String> {
        let raw = self.raw_source()?;
        if raw.units.is_empty() {
            return Err("Raw source has no units.".to_string());
        }
        match selector {
            None => Ok((0..raw.units.len()).collect()),
            Some(sel) => Ok(vec![raw.resolve_unit(sel)?]),
        }
    }
}

// ---------------------------------------------------------------------------
// Checkpoint I/O
// ---------------------------------------------------------------------------

fn load_checkpoint(path: &Path) -> Result<AdaptCheckpoint, String> {
    if !path.exists() {
        return Err(format!("No checkpoint at {}.", path.display()));
    }
    let bytes = fs::read(path).map_err(|e| format!("Checkpoint read failed: {e}"))?;
    let checkpoint: AdaptCheckpoint =
        serde_json::from_slice(&bytes).map_err(|e| format!("Checkpoint is unreadable: {e}"))?;
    if checkpoint.version != CHECKPOINT_VERSION {
        return Err(format!(
            "Checkpoint schema v{} is not supported by this build (expected v{}).",
            checkpoint.version, CHECKPOINT_VERSION
        ));
    }
    Ok(checkpoint)
}

/// Age of a checkpoint, rendered for the restore message.
fn describe_age(saved_at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let secs = now.saturating_sub(saved_at);
    if secs < 90 {
        "just now".to_string()
    } else if secs < 5400 {
        format!("{} minute(s) ago", secs / 60)
    } else if secs < 172_800 {
        format!("{} hour(s) ago", secs / 3600)
    } else {
        format!("{} day(s) ago", secs / 86_400)
    }
}

// ---------------------------------------------------------------------------
// Raw text chunking
// ---------------------------------------------------------------------------

/// Split raw text into units on chapter headings, then split any chapter
/// longer than `max_sentences` into parts.
///
/// Falls back to a single chapter named after the file when no headings are
/// found, so short texts and single chapters just work.
pub(crate) fn split_into_units(
    content: &str,
    fallback_name: &str,
    max_sentences: usize,
) -> Vec<RawUnit> {
    let mut chapters: Vec<(String, Vec<String>)> = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in content.lines() {
        if let Some(name) = heading_name(line) {
            if current_name.is_some() || current_lines.iter().any(|l| !l.trim().is_empty()) {
                chapters.push((
                    current_name.take().unwrap_or_else(|| fallback_name.to_string()),
                    std::mem::take(&mut current_lines),
                ));
            }
            current_lines.clear();
            current_name = Some(name);
        } else {
            current_lines.push(line.to_string());
        }
    }
    if current_name.is_some() || current_lines.iter().any(|l| !l.trim().is_empty()) {
        chapters.push((
            current_name.unwrap_or_else(|| fallback_name.to_string()),
            current_lines,
        ));
    }

    let max_sentences = max_sentences.max(1);
    let mut counter = 0usize;
    let mut units = Vec::new();

    for (chapter, lines) in chapters {
        let body = lines.join("\n");
        if body.trim().is_empty() {
            continue;
        }
        let parts = chunk_paragraphs(&body, max_sentences);
        let part_count = parts.len();
        for (i, part) in parts.into_iter().enumerate() {
            let sentences: Vec<RawSentence> = part
                .into_iter()
                .map(|text| {
                    counter += 1;
                    RawSentence {
                        id: format!("R{}", counter),
                        text,
                    }
                })
                .collect();
            if sentences.is_empty() {
                continue;
            }
            units.push(RawUnit::new_part(
                chapter.clone(),
                i + 1,
                part_count,
                sentences,
            ));
        }
    }
    units
}

/// Break a chapter body into parts of at most `max_sentences` sentences,
/// cutting on paragraph boundaries.
///
/// Paragraph-aligned cuts matter: a part that starts mid-paragraph gives the
/// model no scene to anchor on, and the seam shows up in the merged chapter.
fn chunk_paragraphs(body: &str, max_sentences: usize) -> Vec<Vec<String>> {
    // Split on blank lines; a chapter with no blank lines is one paragraph.
    let mut paragraphs: Vec<Vec<String>> = Vec::new();
    let mut buffer: Vec<&str> = Vec::new();
    let mut flush = |buffer: &mut Vec<&str>, paragraphs: &mut Vec<Vec<String>>| {
        if buffer.iter().any(|l| !l.trim().is_empty()) {
            let sentences = split_sentences(&buffer.join("\n"));
            if !sentences.is_empty() {
                paragraphs.push(sentences);
            }
        }
        buffer.clear();
    };
    for line in body.lines() {
        if line.trim().is_empty() {
            flush(&mut buffer, &mut paragraphs);
        } else {
            buffer.push(line);
        }
    }
    flush(&mut buffer, &mut paragraphs);

    // A single paragraph longer than the budget still has to be cut, so break
    // it on sentence boundaries as a last resort.
    let mut pieces: Vec<Vec<String>> = Vec::new();
    for para in paragraphs {
        if para.len() <= max_sentences {
            pieces.push(para);
        } else {
            for chunk in para.chunks(max_sentences) {
                pieces.push(chunk.to_vec());
            }
        }
    }

    let mut parts: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for piece in pieces {
        if !current.is_empty() && current.len() + piece.len() > max_sentences {
            parts.push(std::mem::take(&mut current));
        }
        current.extend(piece);
    }
    if !current.is_empty() {
        parts.push(current);
    }

    // Avoid a runt final part — a two-sentence LLM call has no arc to hold on
    // to and reads as a seam. Fold it back into its predecessor, but only when
    // that keeps the part within half again of the budget.
    if parts.len() > 1 {
        let threshold = (max_sentences / 4).max(1);
        let tail_len = parts.last().map(|p| p.len()).unwrap_or(0);
        let prev_len = parts[parts.len() - 2].len();
        if tail_len <= threshold && prev_len + tail_len <= max_sentences * 3 / 2 {
            let tail = parts.pop().unwrap();
            if let Some(prev) = parts.last_mut() {
                prev.extend(tail);
            }
        }
    }
    parts
}

/// Naive sentence splitter for raw display/chunking only.
///
/// Raw sentences are never mapped one-for-one to output sentences, so this
/// does not need to be linguistically exact — the whole unit is what gets
/// sent to the model.
pub(crate) fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;

    while i < chars.len() {
        if !matches!(chars[i], '.' | '!' | '?') {
            i += 1;
            continue;
        }
        // Absorb trailing quotes/brackets so `"Go away!"` stays together.
        let mut j = i + 1;
        while j < chars.len() && matches!(chars[j], '"' | '\'' | '”' | '’' | ')') {
            j += 1;
        }
        // A break needs whitespace (or EOF) after the terminator...
        let ends = j >= chars.len() || chars[j].is_whitespace();
        // ...and the next word must not be a lowercase continuation, which is
        // how dialogue tags (`"Go away!" he said.`) are recognised.
        let continues = chars[j..]
            .iter()
            .find(|c| !c.is_whitespace())
            .is_some_and(|c| c.is_lowercase());

        let piece: String = chars[start..j].iter().collect();
        if ends && !continues && !piece.trim().is_empty() && !is_abbreviation(piece.trim()) {
            out.push(piece.trim().to_string());
            start = j;
        }
        i = j;
    }

    let tail: String = chars[start..].iter().collect();
    if !tail.trim().is_empty() {
        out.push(tail.trim().to_string());
    }
    out
}

/// True when the fragment ends with a common title abbreviation.
fn is_abbreviation(fragment: &str) -> bool {
    const ABBREVS: [&str; 8] = ["Mr.", "Mrs.", "Ms.", "Dr.", "St.", "Prof.", "vs.", "etc."];
    ABBREVS.iter().any(|a| fragment.ends_with(a))
}

/// `%%META chapter: <name>%%` must survive the source parser intact.
fn sanitize_chapter_name(name: &str) -> String {
    name.replace("%%", " ").trim().to_string()
}

fn sanitize_file_stem(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_chapter_headings() {
        let text = "CHAPTER 1. Loomings\nCall me Ishmael. Some years ago.\n\n\
                    CHAPTER 2. The Carpet-Bag\nI stuffed a shirt.";
        let units = split_into_units(text, "Book", 40);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].name, "CHAPTER 1. Loomings");
        assert_eq!(units[0].sentences.len(), 2);
        assert_eq!(units[1].name, "CHAPTER 2. The Carpet-Bag");
        // Raw ids are continuous across units.
        assert_eq!(units[1].sentences[0].id, "R3");
    }

    #[test]
    fn falls_back_to_one_unit_without_headings() {
        let units = split_into_units("Just some prose. And more.", "Fable", 40);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "Fable");
        assert_eq!(units[0].sentences.len(), 2);
    }

    #[test]
    fn honours_explicit_meta_chapter_directives() {
        let text = "%%META chapter: Uno%%\nHola. Adios.\n%%META chapter: Dos%%\nOtra vez.";
        let units = split_into_units(text, "Book", 40);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].name, "Uno");
        assert_eq!(units[1].name, "Dos");
    }

    #[test]
    fn splits_long_chapters_into_named_parts() {
        // Six paragraphs of two sentences each, budget of two per unit.
        let body: String = (1..=6)
            .map(|n| format!("Uno {n}. Dos {n}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let text = format!("CHAPTER 1. Long\n{body}");
        let units = split_into_units(&text, "Book", 2);

        assert_eq!(units.len(), 6);
        assert_eq!(units[0].name, "CHAPTER 1. Long (part 1/6)");
        assert_eq!(units[5].name, "CHAPTER 1. Long (part 6/6)");
        // Every part belongs to the same chapter and is merged back on promote.
        assert!(units.iter().all(|u| u.chapter == "CHAPTER 1. Long"));
        assert!(units.iter().all(|u| u.sentences.len() == 2));
        // Only the first part carries the chapter title line.
        assert!(units[0].expects_title());
        assert!(!units[1].expects_title());
    }

    #[test]
    fn cuts_on_paragraph_boundaries_and_absorbs_runts() {
        // Paragraphs of 3 and 1 sentences with a budget of 3: the lone
        // trailing sentence is folded back rather than becoming its own call.
        let text = "A one. A two. A three.\n\nB one.";
        let parts = chunk_paragraphs(text, 3);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 4);
    }

    #[test]
    fn splits_a_single_oversized_paragraph() {
        let text = "A. B. C. D. E.";
        let parts = chunk_paragraphs(text, 2);
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn does_not_split_on_titles() {
        let s = split_sentences("Mr. Smith went home. He was tired.");
        assert_eq!(s, vec!["Mr. Smith went home.", "He was tired."]);
    }

    #[test]
    fn keeps_trailing_quotes_with_the_sentence() {
        let s = split_sentences("\"Go away!\" he said. Then he left.");
        assert_eq!(s, vec!["\"Go away!\" he said.", "Then he left."]);
    }

    #[test]
    fn sanitizes_chapter_names_for_the_directive() {
        assert_eq!(sanitize_chapter_name("Uno %% Dos"), "Uno   Dos");
        assert_eq!(sanitize_file_stem("Moby Dick!"), "Moby_Dick_");
    }
}
