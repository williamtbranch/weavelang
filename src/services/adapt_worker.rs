//! Background worker for the raw-source adaptation loop.
//!
//! Runs the ESCore "fable harness" cycle — draft, score, squeeze, re-score —
//! on a background thread so the GUI stays responsive.  Progress and results
//! are published through a shared [`AdaptJobState`], mirroring the existing
//! AV-job pattern in `app::state::AvJobState`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::domain::raw_source::{AdaptStatus, AdaptTarget, DomainLemma, RawUnit};
use crate::services::llm_client::LlmService;
use crate::services::llm_logger::LlmLogger;
use crate::services::prompt_manager::PromptManager;
use crate::services::python_bridge::BridgeService;
use crate::simulation::escore;

/// Prompt template names, resolved through `PromptManager` so a project can
/// override them per language pair.
const PROMPT_SPEC: &str = "adapt_spec";
const PROMPT_DRAFT: &str = "adapt_draft";
const PROMPT_SQUEEZE: &str = "adapt_squeeze";

/// Sentinel the model emits when no productive substitutions remain.
const FLOOR_SENTINEL: &str = "FLOOR REACHED";

/// Cap on how much of the previous chapter is replayed for continuity.
const PRIOR_CONTEXT_WORD_CAP: usize = 1200;

/// What the job should do to each unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptMode {
    /// One draft pass, replacing any existing draft.
    Draft,
    /// One squeeze pass against the current draft and its DRC report.
    Squeeze,
    /// Draft if needed, then squeeze until the DRC passes, the improvement
    /// stalls, or the anti-churn ceiling is hit.
    Run,
}

impl AdaptMode {
    fn label(self) -> &'static str {
        match self {
            AdaptMode::Draft => "draft",
            AdaptMode::Squeeze => "squeeze",
            AdaptMode::Run => "run",
        }
    }
}

/// A finished unit, ready for the GUI to swap into `RawSource::units`.
#[derive(Debug, Clone)]
pub struct AdaptUnitResult {
    pub unit_index: usize,
    pub unit: RawUnit,
}

/// Shared progress/result state for a running adaptation job.
#[derive(Debug, Default)]
pub struct AdaptJobState {
    pub log: Vec<String>,
    pub cancel_requested: bool,
    pub finished: bool,
    pub label: String,
    pub done_units: usize,
    pub total_units: usize,
    /// Completed units, appended as they finish.
    pub results: Vec<AdaptUnitResult>,
    /// How many of `results` the GUI has already applied.
    pub applied: usize,
    pub result_message: Option<String>,
}

impl AdaptJobState {
    fn push_log(state: &Arc<Mutex<Self>>, line: impl Into<String>) {
        if let Ok(mut guard) = state.lock() {
            guard.log.push(line.into());
        }
    }

    fn cancelled(state: &Arc<Mutex<Self>>) -> bool {
        state
            .lock()
            .map(|guard| guard.cancel_requested)
            .unwrap_or(false)
    }
}

/// Everything the worker needs, bundled so the spawn signature stays readable.
pub struct AdaptJobConfig {
    pub prompts: PromptManager,
    pub llm: LlmService,
    pub logger: LlmLogger,
    pub bridge: BridgeService,
    pub model: String,
    pub fallback_model: Option<String>,
    /// Language of the raw source (prompt-pair lookup + display).
    pub source_language: String,
    /// Language the adaptation is written in; also the scoring language.
    pub target_language: String,
    pub domain_lemmas: Vec<DomainLemma>,
    /// Pre-rendered "APPROVED DOMAIN VOCABULARY" block, if any.
    pub domain_block: Option<String>,
    pub target: AdaptTarget,
    pub mode: AdaptMode,
    /// Units to process, as `(index_in_raw_source, unit)`.
    pub units: Vec<(usize, RawUnit)>,
    /// Drafts of the units preceding each selected unit, for continuity.
    /// Keyed by the unit's index in `RawSource::units`.
    pub prior_drafts: Vec<(usize, String)>,
}

/// Spawn the adaptation job.  Returns the shared state plus a cancel flag.
pub fn spawn_adapt_job(
    config: AdaptJobConfig,
) -> (Arc<Mutex<AdaptJobState>>, Arc<AtomicBool>) {
    let total = config.units.len();
    let state = Arc::new(Mutex::new(AdaptJobState {
        label: format!("Adapt ({}) — {} unit(s)", config.mode.label(), total),
        total_units: total,
        ..Default::default()
    }));
    let cancel = Arc::new(AtomicBool::new(false));

    let thread_state = Arc::clone(&state);
    let thread_cancel = Arc::clone(&cancel);

    thread::spawn(move || {
        run_job(config, &thread_state, &thread_cancel);
        if let Ok(mut guard) = thread_state.lock() {
            guard.finished = true;
            if guard.result_message.is_none() {
                guard.result_message = Some(format!(
                    "Adaptation finished: {}/{} unit(s) processed.",
                    guard.done_units, guard.total_units
                ));
            }
        }
    });

    (state, cancel)
}

fn run_job(
    config: AdaptJobConfig,
    state: &Arc<Mutex<AdaptJobState>>,
    cancel: &Arc<AtomicBool>,
) {
    let AdaptJobConfig {
        prompts,
        llm,
        logger,
        bridge,
        model,
        fallback_model,
        source_language,
        target_language,
        domain_lemmas,
        domain_block,
        target,
        mode,
        units,
        prior_drafts,
    } = config;

    let spec = match prompts.get_prompt(PROMPT_SPEC, &source_language, &target_language) {
        Ok(text) => render_vars(&text, &target_language, &target, 0, ""),
        Err(e) => {
            fail(state, format!("Failed to load '{}': {}", PROMPT_SPEC, e));
            return;
        }
    };

    // Drafts produced during this job, keyed by unit index, used to give the
    // next unit real continuity context.
    let mut produced: HashMap<usize, String> = prior_drafts
        .iter()
        .filter_map(|(idx, text)| idx.checked_sub(1).map(|prev| (prev, text.clone())))
        .collect();

    for (unit_index, mut unit) in units {
        if cancel.load(Ordering::SeqCst) || AdaptJobState::cancelled(state) {
            AdaptJobState::push_log(state, "Cancelled.");
            break;
        }

        // Continuity context is the draft of the preceding unit: the one this
        // job just produced if it processed it, otherwise the one that already
        // existed. Without the former a fresh `run all` would hand every unit
        // an empty context, because none of them had drafts when it was queued.
        let prior = unit_index
            .checked_sub(1)
            .and_then(|prev| produced.get(&prev))
            .map(String::as_str)
            .unwrap_or("");

        let outcome = process_unit(
            &mut unit,
            unit_index,
            mode,
            &spec,
            &prompts,
            &llm,
            &logger,
            &bridge,
            &model,
            fallback_model.as_deref(),
            &source_language,
            &target_language,
            &domain_lemmas,
            domain_block.as_deref(),
            &target,
            prior,
            state,
            cancel,
        );

        if let Err(err) = outcome {
            unit.last_error = Some(err.clone());
            AdaptJobState::push_log(state, format!("[{}] ERROR: {}", unit.name, err));
        }

        if !unit.draft.trim().is_empty() {
            produced.insert(unit_index, unit.draft.clone());
        }

        if let Ok(mut guard) = state.lock() {
            guard.results.push(AdaptUnitResult { unit_index, unit });
            guard.done_units += 1;
        }
    }
}

fn fail(state: &Arc<Mutex<AdaptJobState>>, message: String) {
    if let Ok(mut guard) = state.lock() {
        guard.log.push(message.clone());
        guard.result_message = Some(message);
    }
}

#[allow(clippy::too_many_arguments)]
fn process_unit(
    unit: &mut RawUnit,
    unit_index: usize,
    mode: AdaptMode,
    spec: &str,
    prompts: &PromptManager,
    llm: &LlmService,
    logger: &LlmLogger,
    bridge: &BridgeService,
    model: &str,
    fallback_model: Option<&str>,
    source_language: &str,
    target_language: &str,
    domain_lemmas: &[DomainLemma],
    domain_block: Option<&str>,
    target: &AdaptTarget,
    prior_draft: &str,
    state: &Arc<Mutex<AdaptJobState>>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let source_text = unit.source_text();
    if source_text.trim().is_empty() {
        return Err("Unit has no raw text.".to_string());
    }

    // Resume: a unit that already passed or hit its floor is done. Say so and
    // spend nothing — this is what makes re-running `adapt run all` after an
    // interruption cost only the unfinished remainder.
    if matches!(mode, AdaptMode::Run) && unit.is_complete() {
        AdaptJobState::push_log(
            state,
            format!(
                "[{}] already {} at v{} — skipped, no LLM calls.",
                unit.name,
                unit.status.label(),
                unit.version
            ),
        );
        return Ok(());
    }

    unit.last_error = None;

    let needs_draft = matches!(mode, AdaptMode::Draft)
        || (matches!(mode, AdaptMode::Run) && unit.draft.trim().is_empty());

    if needs_draft {
        AdaptJobState::push_log(
            state,
            format!("[{}] drafting (unit {})...", unit.name, unit_index + 1),
        );
        let user = build_draft_prompt(
            prompts,
            source_language,
            target_language,
            target,
            unit,
            &source_text,
            prior_draft,
            domain_block,
        )?;
        let response = complete_with_retry(
            llm,
            logger,
            model,
            fallback_model,
            spec,
            &user,
            &format!("adapt draft: {}", unit.name),
            cancel,
        )?;
        apply_response(unit, &response, true);
        score_unit(unit, bridge, target_language, &source_text, domain_lemmas, target, state);
    }

    if matches!(mode, AdaptMode::Draft) {
        return Ok(());
    }

    if unit.draft.trim().is_empty() {
        return Err("No draft to squeeze. Run a draft pass first.".to_string());
    }

    let max_version = 1 + target.max_squeeze_passes;
    let mut passes = 0u32;

    loop {
        if cancel.load(Ordering::SeqCst) || AdaptJobState::cancelled(state) {
            return Ok(());
        }
        if unit.status == AdaptStatus::Passing || unit.status == AdaptStatus::Floor {
            break;
        }
        if unit.version >= max_version {
            AdaptJobState::push_log(
                state,
                format!(
                    "[{}] anti-churn ceiling reached at v{}.",
                    unit.name, unit.version
                ),
            );
            unit.status = AdaptStatus::Floor;
            break;
        }

        let report = unit
            .report
            .clone()
            .ok_or("No DRC report available; score the draft first.")?;
        let before = report.i_score.i_level;

        AdaptJobState::push_log(
            state,
            format!("[{}] squeezing v{} -> v{}...", unit.name, unit.version, unit.version + 1),
        );

        let user = build_squeeze_prompt(
            prompts,
            source_language,
            target_language,
            target,
            unit,
            &source_text,
            &report,
            domain_block,
        )?;
        let response = complete_with_retry(
            llm,
            logger,
            model,
            fallback_model,
            spec,
            &user,
            &format!("adapt squeeze: {}", unit.name),
            cancel,
        )?;

        let floor_declared = response.contains(FLOOR_SENTINEL);
        apply_response(unit, &response, false);
        score_unit(unit, bridge, target_language, &source_text, domain_lemmas, target, state);
        passes += 1;

        if floor_declared {
            AdaptJobState::push_log(state, format!("[{}] model declared FLOOR REACHED.", unit.name));
            if unit.status != AdaptStatus::Passing {
                unit.status = AdaptStatus::Floor;
            }
            break;
        }

        if let Some(after) = unit.report.as_ref().map(|r| r.i_score.i_level) {
            if unit.status != AdaptStatus::Passing && (before - after) < target.min_gain {
                AdaptJobState::push_log(
                    state,
                    format!(
                        "[{}] pass changed iLevel by {:.2} (< {:.2}); floor reached.",
                        unit.name,
                        before - after,
                        target.min_gain
                    ),
                );
                unit.status = AdaptStatus::Floor;
                break;
            }
        }

        // `Squeeze` mode is explicitly one pass.
        if matches!(mode, AdaptMode::Squeeze) || passes >= target.max_squeeze_passes {
            break;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Prompt assembly
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn build_draft_prompt(
    prompts: &PromptManager,
    source_language: &str,
    target_language: &str,
    target: &AdaptTarget,
    unit: &RawUnit,
    source_text: &str,
    prior_draft: &str,
    domain_block: Option<&str>,
) -> Result<String, String> {
    let template = prompts
        .get_prompt(PROMPT_DRAFT, source_language, target_language)
        .map_err(|e| format!("Failed to load '{}': {}", PROMPT_DRAFT, e))?;
    let source_words = escore::count_words(source_text);
    let template = render_part_vars(&template, unit);
    let mut user = render_vars(&template, target_language, target, source_words, unit.chapter_name());

    user.push_str("\n\n--- SOURCE (original) ---\n\n");
    user.push_str(source_text);

    if !prior_draft.trim().is_empty() {
        let header = if unit.part > 1 {
            "\n\n--- EARLIER IN THIS CHAPTER (already written, for continuity) ---\n\
             This comes immediately BEFORE the source you are adapting and is part of\n\
             the SAME chapter. It is for continuity ONLY. Do NOT repeat its content,\n\
             do NOT restate the chapter title, and do NOT re-gloss payload words\n\
             already introduced there — treat them as known to the reader.\n\
             Match its names, register, and tone exactly.\n\n"
        } else {
            "\n\n--- PREVIOUS CHAPTER (already written, for continuity) ---\n\
             This comes immediately BEFORE the source you are adapting. It is for\n\
             continuity ONLY. Do NOT repeat its content, and do NOT re-gloss payload\n\
             words already introduced there — treat them as known to the reader.\n\
             Match its names, register, and tone.\n\n"
        };
        user.push_str(header);
        user.push_str(&truncate_words(prior_draft, PRIOR_CONTEXT_WORD_CAP));
    }

    if let Some(block) = domain_block {
        user.push_str(block);
    }
    Ok(user)
}

#[allow(clippy::too_many_arguments)]
fn build_squeeze_prompt(
    prompts: &PromptManager,
    source_language: &str,
    target_language: &str,
    target: &AdaptTarget,
    unit: &RawUnit,
    source_text: &str,
    report: &crate::domain::raw_source::DrcReport,
    domain_block: Option<&str>,
) -> Result<String, String> {
    let template = prompts
        .get_prompt(PROMPT_SQUEEZE, source_language, target_language)
        .map_err(|e| format!("Failed to load '{}': {}", PROMPT_SQUEEZE, e))?;
    let source_words = escore::count_words(source_text);
    let mut user = render_vars(
        &render_part_vars(&template, unit),
        target_language,
        target,
        source_words,
        unit.chapter_name(),
    );

    user.push_str(&format!("\n\n--- CURRENT DRAFT (v{}) ---\n\n", unit.version));
    user.push_str(&unit.draft);
    user.push_str(&format!("\n\n--- DRC REPORT (v{}) ---\n\n", unit.version));
    user.push_str(&escore::render_report(report, &unit.name));

    if let Some(block) = domain_block {
        user.push_str(block);
    }
    Ok(user)
}

/// Substitute the part-aware placeholders.
///
/// A long chapter is adapted in parts, but the promoted source text must have
/// exactly one heading per chapter — so only part 1 is allowed to write the
/// chapter title line.
fn render_part_vars(template: &str, unit: &RawUnit) -> String {
    let title_rule = if unit.expects_title() {
        "CHAPTER TITLE LINE — required.\n\
         The FIRST non-empty line of your output must be the chapter title in\n\
         {{TARGET_LANGUAGE_NAME}}, as plain text, with the chapter number and name\n\
         separated by a colon (for example: \"Capítulo 1: El puerto\").\n\
         Then leave one blank line and start the prose.\n\
         Do not use Markdown headings, bold, or any other markup on that line."
            .to_string()
    } else {
        "CHAPTER TITLE LINE — forbidden.\n\
         This is a CONTINUATION of a chapter that is already underway. Do NOT write\n\
         a title, a heading, a part number, or any kind of separator. Begin directly\n\
         with prose that continues from the previous text, and end mid-chapter\n\
         without any closing flourish."
            .to_string()
    };
    let part_note = if unit.part_count > 1 {
        format!(
            "This is part {} of {} of the chapter \"{}\".",
            unit.part, unit.part_count, unit.chapter_name()
        )
    } else {
        String::new()
    };
    template
        .replace("{{CHAPTER_TITLE_RULE}}", &title_rule)
        .replace("{{PART_NOTE}}", &part_note)
}

/// Substitute the `{{KEY}}` placeholders shared by all three templates.
fn render_vars(
    template: &str,
    target_language: &str,
    target: &AdaptTarget,
    source_words: u32,
    chapter_name: &str,
) -> String {
    template
        .replace("{{TARGET_LANGUAGE_NAME}}", language_name(target_language))
        .replace("{{TARGET_LANGUAGE}}", target_language)
        .replace("{{I_LEVEL_MAX}}", &format!("{:.1}", target.i_level_max))
        .replace(
            "{{COVERAGE_PCT}}",
            &format!("{:.0}", target.coverage * 100.0),
        )
        .replace(
            "{{TAIL_PCT}}",
            &format!("{:.0}", (1.0 - target.coverage) * 100.0),
        )
        .replace("{{MIN_PERCENT}}", &format!("{:.0}", target.min_percent))
        .replace("{{MAX_PERCENT}}", &format!("{:.0}", target.max_percent))
        .replace("{{SOURCE_WORDS}}", &source_words.to_string())
        .replace("{{CHAPTER_NAME}}", chapter_name)
}

/// Human-readable name for the language codes this pipeline supports.
pub fn language_name(code: &str) -> &str {
    match code.to_ascii_lowercase().as_str() {
        "es" => "Spanish",
        "en" => "English",
        "fr" => "French",
        "de" => "German",
        "it" => "Italian",
        "pt" => "Portuguese",
        other => {
            // Fall back to the raw code rather than guessing.
            let _ = other;
            code
        }
    }
}

fn truncate_words(text: &str, max_words: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= max_words {
        return text.to_string();
    }
    let tail = &words[words.len() - max_words..];
    format!("[...]\n{}", tail.join(" "))
}

// ---------------------------------------------------------------------------
// LLM plumbing
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn complete_with_retry(
    llm: &LlmService,
    logger: &LlmLogger,
    model: &str,
    fallback_model: Option<&str>,
    system: &str,
    user: &str,
    context: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<String, String> {
    let mut models = vec![model.to_string()];
    if let Some(fb) = fallback_model {
        if fb != model {
            models.push(fb.to_string());
        }
    }

    let mut errors: Vec<String> = Vec::new();
    for m in &models {
        for attempt in 0..3u32 {
            if cancel.load(Ordering::SeqCst) {
                return Err("Cancelled.".to_string());
            }
            match llm.complete(m, system, user) {
                Ok(resp) => {
                    logger.log_interaction(context, system, user, &resp);
                    return Ok(resp);
                }
                Err(e) => {
                    logger.log_interaction(context, system, user, &format!("ERROR: {}", e));
                    let fatal = is_fatal(&e);
                    errors.push(format!(
                        "  [model '{}', attempt {}/3]{} {}",
                        m,
                        attempt + 1,
                        if fatal { " FATAL —" } else { "" },
                        e
                    ));
                    // A rejected request (bad key, unknown model, malformed
                    // body) fails identically every time. Retrying it just
                    // burns wall time before the inevitable fallback.
                    if fatal {
                        break;
                    }
                    thread::sleep(Duration::from_secs(1u64 << attempt));
                }
            }
        }
    }
    Err(format!(
        "All LLM attempts failed.\n{}",
        errors.join("\n")
    ))
}

/// True for API errors that are deterministic — retrying cannot help.
fn is_fatal(err: &str) -> bool {
    ["HTTP 400", "HTTP 401", "HTTP 403", "HTTP 404"]
        .iter()
        .any(|code| err.contains(code))
}

/// Strip anything the model was told not to emit, then install the result.
fn apply_response(unit: &mut RawUnit, response: &str, is_draft: bool) {
    let cleaned = clean_response(response);
    if !unit.draft.is_empty() {
        unit.history.push(std::mem::take(&mut unit.draft));
    }
    unit.draft = cleaned;
    unit.version = if is_draft { 1 } else { unit.version + 1 };
    // `unit.name` stays as imported: it is the selector shown in the UI and,
    // for a split chapter, encodes which part this is. The adapted chapter
    // title is read from the draft at promotion time instead.
    unit.status = AdaptStatus::Drafted;
}

/// Remove code fences, stray `%%META%%` lines, and the floor sentinel.
pub fn clean_response(response: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            continue;
        }
        if trimmed.starts_with("%%") {
            continue;
        }
        if trimmed.starts_with(FLOOR_SENTINEL) {
            continue;
        }
        kept.push(line);
    }
    kept.join("\n").trim().to_string()
}

fn score_unit(
    unit: &mut RawUnit,
    bridge: &BridgeService,
    target_language: &str,
    source_text: &str,
    domain_lemmas: &[DomainLemma],
    target: &AdaptTarget,
    state: &Arc<Mutex<AdaptJobState>>,
) {
    match escore::score(
        bridge,
        target_language,
        &unit.draft,
        source_text,
        domain_lemmas,
        target,
    ) {
        Ok(report) => {
            AdaptJobState::push_log(
                state,
                format!(
                    "[{}] v{}: {} — iLevel {:.1} (limit {:.1}), UL{}, {} words ({:.1}% of source)",
                    unit.name,
                    unit.version,
                    if report.overall_pass { "PASS" } else { "FAIL" },
                    report.i_score.i_level,
                    report.i_level_max,
                    report.ul_floor(),
                    report.submission_words,
                    report.percent_of_source,
                ),
            );
            if report.overall_pass {
                unit.status = AdaptStatus::Passing;
            }
            unit.report = Some(report);
        }
        Err(e) => {
            unit.report = None;
            unit.last_error = Some(format!("Scoring failed: {}", e));
            AdaptJobState::push_log(state, format!("[{}] scoring failed: {}", unit.name, e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_fences_meta_and_floor_sentinel() {
        let raw = "```\n%%META chapter: One%%\nTítulo: Uno\n\nHola.\nFLOOR REACHED — nothing left.\n```";
        assert_eq!(clean_response(raw), "Título: Uno\n\nHola.");
    }

    #[test]
    fn draft_pass_resets_version_squeeze_increments() {
        let mut unit = RawUnit::new("One".into(), vec![]);
        apply_response(&mut unit, "Title: One\n\nBody.", true);
        assert_eq!(unit.version, 1);
        assert!(unit.history.is_empty());
        apply_response(&mut unit, "Title: One\n\nBetter body.", false);
        assert_eq!(unit.version, 2);
        assert_eq!(unit.history.len(), 1);
        // The title line becomes the chapter name for %%META chapter:%%.
        assert_eq!(unit.name, "One");
    }

    #[test]
    fn truncates_prior_context_from_the_end() {
        let text = (1..=10).map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
        assert_eq!(truncate_words(&text, 3), "[...]\n8 9 10");
        assert_eq!(truncate_words(&text, 50), text);
    }
}
