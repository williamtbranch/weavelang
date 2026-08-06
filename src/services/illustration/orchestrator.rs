// src/services/illustration/orchestrator.rs
//
// Stage sequencing for `av generate prompts`.
//
// Fully unattended: no confirmation gates. It prints a plan, runs every stage,
// and reports. Manual edits are supported by lock flags in the bible plus the
// fact that stages 5-7 are deterministic — editing the bible and re-running
// re-renders every prompt for zero API calls.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use super::bible;
use super::extract;
use super::lint::{self, LintReport};
use super::llm::{map_concurrent, IllustrationLlm};
use super::output;
use super::render::{self, RenderConfig};
use super::scene_plan::{self, SegmentRequest};
use super::segment::{self, SegmentBy};
use super::thumbnail::{self, ThumbnailConfig};
use super::types::{Bible, RenderedPrompt, Scene, SceneKind, ScenePlan, SCENES_SCHEMA_VERSION};

pub struct JobConfig {
    pub tts_dir: PathBuf,
    pub illustrations_dir: PathBuf,
    pub bible_dir: PathBuf,
    pub cache_root: Option<PathBuf>,
    pub book_name: String,
    pub chapter_name: String,
    /// Overrides UL0 discovery when lesson-realign alignment is active.
    pub input_file: Option<PathBuf>,

    pub sentences_per_illustration: usize,
    pub minimum_count: usize,
    pub max_illustrations: usize,
    pub segment_by: SegmentBy,
    pub context_radius: usize,

    pub prompt_model: String,
    pub fallback_models: Vec<String>,
    pub prompt_concurrency: usize,
    pub max_bible_characters: usize,

    pub render: RenderConfig,
    pub max_prompt_chars: usize,

    /// YouTube key art. Two extra prompts appended after the scene prompts.
    pub thumbnail: ThumbnailConfig,

    /// Rebuild the bible and re-plan every scene even when cached artifacts
    /// are still valid.
    pub force: bool,
}

pub type Logger<'a> = &'a (dyn Fn(String) + Sync);

pub fn run(cfg: &JobConfig, log: Logger, cancel: &AtomicBool) -> Result<String, String> {
    require_story_title(cfg)?;

    // --- S0: load and plan -------------------------------------------------
    let source = match &cfg.input_file {
        Some(p) => p.clone(),
        None => segment::find_ul0(&cfg.tts_dir, &cfg.book_name, &cfg.chapter_name)?,
    };
    log(format!("Source text: {}", source.display()));

    let text = std::fs::read_to_string(&source)
        .map_err(|e| format!("Failed to read {}: {}", source.display(), e))?;
    let paragraphs = segment::split_paragraphs(&text);
    if paragraphs.is_empty() {
        return Err(format!("No paragraphs found in {}", source.display()));
    }

    let count = segment::illustration_count(
        paragraphs.len(),
        cfg.sentences_per_illustration,
        cfg.minimum_count,
        cfg.max_illustrations,
    );
    let segments = segment::segment(&paragraphs, count, cfg.segment_by);

    let bible_path = cfg.bible_dir.join("characters.toml");
    let mut existing = bible::load(&bible_path)?;
    let migrated = !log_notes(bible::backfill_species(&mut existing), log).is_empty();
    let need_bible = cfg.force || existing.is_empty();

    // A scene plan that still matches the text is reused verbatim. That is what
    // makes a hand-edit to characters.toml cost nothing: stages 5-8 are
    // deterministic, so re-running just re-renders.
    let scenes_path = cfg.illustrations_dir.join("_scenes.json");
    let reusable = if cfg.force {
        None
    } else {
        reusable_plan(&scenes_path, &segments, &cfg.chapter_name)
    };

    log(format!(
        "{} — {} sentences\n  bible     : {}\n  scene plan: {}\n  render+lint: {} prompts",
        cfg.chapter_name,
        paragraphs.len(),
        if need_bible { "rebuild" } else { "reuse (edit characters.toml to change)" },
        match &reusable {
            Some(_) => "reuse (pass 'force' to re-plan)".to_string(),
            None => format!("{} segments", segments.len()),
        },
        segments.len(),
    ));

    let llm = IllustrationLlm::new(
        cfg.cache_root.clone(),
        &cfg.prompt_model,
        cfg.fallback_models.clone(),
    );

    // --- S2: bible ---------------------------------------------------------
    let bible = if need_bible {
        log("Building character bible...".to_string());
        let fresh = extract::extract(&llm, &text, cfg.max_bible_characters)?;
        let mut merged = bible::merge(&existing, &fresh);
        log_notes(bible::backfill_species(&mut merged), log);
        for w in bible::dedupe_face_blends(&mut merged) {
            log(format!("  [warn] {}", w));
        }
        bible::save(&bible_path, &merged)?;
        log(format!(
            "  {} characters, {} minor, {} locations, {} ensembles -> {}",
            merged.characters.len(),
            merged.minors.len(),
            merged.locations.len(),
            merged.ensembles.len(),
            bible_path.display()
        ));
        merged
    } else {
        // Persist the migration, so the correction is visible in the file the
        // user edits rather than being silently re-applied on every run.
        if migrated {
            bible::save(&bible_path, &existing)?;
        }
        log(format!(
            "Reusing bible: {} characters from {}",
            existing.characters.len(),
            bible_path.display()
        ));
        existing
    };

    if is_cancelled(cancel) {
        return Err("Cancelled.".to_string());
    }

    // --- S4: scene planning ------------------------------------------------
    let reused_thumbnail = reusable.as_ref().and_then(|p| p.thumbnail.clone());
    let scenes = match reusable {
        Some(plan) => {
            log(format!("Reusing {} planned scenes (no API calls).", plan.scenes.len()));
            plan.scenes
        }
        None => plan_scenes(&llm, &bible, &paragraphs, &segments, cfg, log, cancel)?,
    };

    // --- S6/S7: render and lint (deterministic, no API calls) --------------
    let mut prompts = render::render_all(&bible, &scenes, &cfg.render);
    let report = lint::lint_and_repair(
        &bible,
        &scenes,
        &mut prompts,
        &cfg.render,
        cfg.max_prompt_chars,
    );
    log(format!(
        "Lint: {} prompts, {} errors, {} auto-repaired, {} warnings",
        prompts.len(),
        report.errors(),
        report.repaired(),
        report.warnings()
    ));
    for f in report.findings.iter().filter(|f| !f.repaired) {
        log(format!("  [{}] scene {}: {}", f.severity.as_str(), f.scene_index, f.message));
    }

    // --- S9: key art -------------------------------------------------------
    // Appended after linting: the thumbnails deliberately carry title text,
    // which every other prompt is linted for the absence of.
    let key_art = plan_key_art(&llm, &bible, &paragraphs, &scenes, reused_thumbnail, cfg, log);
    if let Some(scene) = &key_art {
        let pair = thumbnail::render_pair(&bible, scene, &cfg.render, &cfg.thumbnail);
        log(format!(
            "Thumbnails: {} and {} ({})",
            pair[0].file, pair[1].file, cfg.thumbnail.size
        ));
        prompts.extend(pair);
    }

    // --- S8: write artifacts ----------------------------------------------
    let plan = ScenePlan {
        schema_version: SCENES_SCHEMA_VERSION,
        chapter: cfg.chapter_name.clone(),
        scenes,
        thumbnail: key_art,
    };
    write_artifacts(cfg, &bible, &plan, &prompts, &report)?;

    Ok(summary(cfg, &prompts, &report))
}

/// Re-render `_prompts.toml` from the existing scene plan and the (possibly
/// hand-edited) bible. No LLM calls at all — this is the path that makes manual
/// bible edits free to apply.
pub fn rerender(cfg: &JobConfig, log: Logger) -> Result<String, String> {
    require_story_title(cfg)?;
    let bible_path = cfg.bible_dir.join("characters.toml");
    let mut bible = bible::load(&bible_path)?;
    if bible.is_empty() {
        return Err(format!("No bible at {}. Run 'av generate prompts' first.", bible_path.display()));
    }
    if !log_notes(bible::backfill_species(&mut bible), log).is_empty() {
        bible::save(&bible_path, &bible)?;
    }
    let scenes_path = cfg.illustrations_dir.join("_scenes.json");
    let plan = output::read_scenes(&scenes_path)?;

    let mut prompts = render::render_all(&bible, &plan.scenes, &cfg.render);
    let report = lint::lint_and_repair(
        &bible,
        &plan.scenes,
        &mut prompts,
        &cfg.render,
        cfg.max_prompt_chars,
    );
    if cfg.thumbnail.enabled {
        if let Some(scene) = &plan.thumbnail {
            prompts.extend(thumbnail::render_pair(&bible, scene, &cfg.render, &cfg.thumbnail));
        }
    }
    log(format!(
        "Re-rendered {} prompts from the existing scene plan (no API calls). {} errors, {} repaired.",
        prompts.len(),
        report.errors(),
        report.repaired()
    ));
    write_artifacts(cfg, &bible, &plan, &prompts, &report)?;
    Ok(summary(cfg, &prompts, &report))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Echo migration notes into the run log and hand them back, so the caller can
/// tell whether anything changed without counting twice.
fn log_notes(notes: Vec<String>, log: Logger) -> Vec<String> {
    for n in &notes {
        log(format!("  {}", n));
    }
    notes
}

/// A thumbnail without a title is not usable, and the title cannot be guessed:
/// `book_name` is a filename slug. Refuse early rather than produce artwork
/// that has to be regenerated.
fn require_story_title(cfg: &JobConfig) -> Result<(), String> {
    if cfg.thumbnail.enabled && cfg.thumbnail.story_title.trim().is_empty() {
        return Err(
            "No story title set. The YouTube thumbnail prints it, and it cannot be \
             derived from the project filename.\n  \u{2192} Preferences \u{2192} Story Metadata..., \
             or: set title <the printable title of the work>"
                .to_string(),
        );
    }
    Ok(())
}

/// The key-art scene for the thumbnails. Reused from the stored plan when one
/// exists, so a re-run costs nothing; falls back to the most prominent
/// character rather than skipping, because a thumbnail is not optional.
fn plan_key_art(
    llm: &IllustrationLlm,
    bible: &Bible,
    paragraphs: &[String],
    scenes: &[Scene],
    reused: Option<Scene>,
    cfg: &JobConfig,
    log: Logger,
) -> Option<Scene> {
    if !cfg.thumbnail.enabled {
        return None;
    }
    let index = scenes.iter().map(|s| s.index).max().unwrap_or(0) + 1;

    if let Some(mut scene) = reused {
        log("Reusing planned key art (no API calls).".to_string());
        scene.index = index;
        return Some(scene);
    }

    log("Planning key art for the thumbnail...".to_string());
    match thumbnail::plan(llm, bible, &cfg.thumbnail, paragraphs, index) {
        Ok(scene) => Some(scene),
        Err(e) => {
            log(format!(
                "  [warn] key art planning failed ({}); falling back to the most \
                 prominent character",
                e
            ));
            Some(thumbnail::fallback_scene(bible, index, paragraphs.len()))
        }
    }
}

/// Rebuild only the character bible. Backs `av generate characters`.
pub fn build_bible(cfg: &JobConfig, log: Logger) -> Result<String, String> {
    let source = match &cfg.input_file {
        Some(p) => p.clone(),
        None => segment::find_ul0(&cfg.tts_dir, &cfg.book_name, &cfg.chapter_name)?,
    };
    log(format!("Source text: {}", source.display()));
    let text = std::fs::read_to_string(&source)
        .map_err(|e| format!("Failed to read {}: {}", source.display(), e))?;

    let llm = IllustrationLlm::new(
        cfg.cache_root.clone(),
        &cfg.prompt_model,
        cfg.fallback_models.clone(),
    );
    let bible_path = cfg.bible_dir.join("characters.toml");
    let mut existing = bible::load(&bible_path)?;
    log_notes(bible::backfill_species(&mut existing), log);

    log("Building character bible...".to_string());
    let fresh = extract::extract(&llm, &text, cfg.max_bible_characters)?;
    let mut merged = bible::merge(&existing, &fresh);
    log_notes(bible::backfill_species(&mut merged), log);
    for w in bible::dedupe_face_blends(&mut merged) {
        log(format!("  [warn] {}", w));
    }
    bible::save(&bible_path, &merged)?;

    Ok(format!(
        "Wrote {} characters ({} minor, {} locations, {} ensembles) to {}. \
         Locked entries were preserved. Edit and re-run 'av generate prompts' \
         to apply changes without new API calls.",
        merged.characters.len(),
        merged.minors.len(),
        merged.locations.len(),
        merged.ensembles.len(),
        bible_path.display()
    ))
}

fn plan_scenes(
    llm: &IllustrationLlm,
    bible: &Bible,
    paragraphs: &[String],
    segments: &[segment::Segment],
    cfg: &JobConfig,
    log: Logger,
    cancel: &AtomicBool,
) -> Result<Vec<Scene>, String> {
    let requests: Vec<SegmentRequest> = segments
        .iter()
        .enumerate()
        .map(|(i, seg)| SegmentRequest {
            index: i + 1,
            // 1-based inclusive, matching the existing _prompts.toml contract.
            paragraph_start: seg.start + 1,
            paragraph_end: seg.end,
            text: seg.text.clone(),
            context: segment::build_context(paragraphs, seg, cfg.context_radius),
            previous_subject: String::new(),
        })
        .collect();

    log(format!("Planning {} scenes...", requests.len()));
    let planned = map_concurrent(
        requests,
        cfg.prompt_concurrency,
        Some(cancel),
        |_, req| match scene_plan::plan_segment(llm, bible, req) {
            Ok(scene) => Some(scene),
            Err(e) => {
                log(format!("  [warn] segment {} failed ({}); using fallback", req.index, e));
                Some(scene_plan::fallback_scene(bible, req))
            }
        },
        |done, total| {
            if done % 10 == 0 || done == total {
                log(format!("  planned {}/{}", done, total));
            }
        },
    );

    if is_cancelled(cancel) {
        return Err("Cancelled.".to_string());
    }

    let mut scenes: Vec<Scene> = planned.into_iter().flatten().collect();
    if scenes.is_empty() {
        return Err("Scene planning produced no scenes.".to_string());
    }
    scenes.sort_by_key(|s| s.index);

    // Consecutive tableaux with identical subjects produce near-identical
    // images. Planning runs concurrently so it cannot see its predecessor;
    // this bounded sequential pass fixes the few cases that collide.
    let repeats = repair_repeated_tableaux(llm, bible, &mut scenes, paragraphs, cfg, log);
    if repeats > 0 {
        log(format!("  re-planned {} repeated tableau subject(s)", repeats));
    }
    Ok(scenes)
}

/// An existing plan is reusable only when it still describes the same segments.
/// Any edit to the source text shifts the ranges and forces a re-plan.
fn reusable_plan(
    path: &std::path::Path,
    segments: &[segment::Segment],
    chapter: &str,
) -> Option<ScenePlan> {
    let plan = output::read_scenes(path).ok()?;
    if plan.schema_version != SCENES_SCHEMA_VERSION
        || plan.chapter != chapter
        || plan.scenes.len() != segments.len()
    {
        return None;
    }
    let matches = plan.scenes.iter().zip(segments).all(|(scene, seg)| {
        scene.paragraph_start == seg.start + 1 && scene.paragraph_end == seg.end
    });
    matches.then_some(plan)
}

fn write_artifacts(
    cfg: &JobConfig,
    bible: &Bible,
    plan: &ScenePlan,
    prompts: &[RenderedPrompt],
    report: &LintReport,
) -> Result<(), String> {
    output::write_prompts(&cfg.illustrations_dir.join("_prompts.toml"), prompts)?;
    output::write_illustration_map(
        &cfg.illustrations_dir.join("_illustration_map.json"),
        prompts,
    )?;
    output::write_scenes(&cfg.illustrations_dir.join("_scenes.json"), plan)?;
    output::write_report(&cfg.bible_dir.join("report.md"), bible, plan, report)?;
    Ok(())
}

fn summary(cfg: &JobConfig, prompts: &[RenderedPrompt], report: &LintReport) -> String {
    format!(
        "Wrote {} prompts to {} ({} lint errors, {} auto-repaired). Report: {}",
        prompts.len(),
        cfg.illustrations_dir.join("_prompts.toml").display(),
        report.errors(),
        report.repaired(),
        cfg.bible_dir.join("report.md").display()
    )
}

fn is_cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(std::sync::atomic::Ordering::SeqCst)
}

fn repair_repeated_tableaux(
    llm: &IllustrationLlm,
    bible: &Bible,
    scenes: &mut [Scene],
    paragraphs: &[String],
    cfg: &JobConfig,
    log: Logger,
) -> usize {
    let mut fixed = 0usize;
    for i in 1..scenes.len() {
        if scenes[i].kind != SceneKind::Tableau || scenes[i - 1].kind != SceneKind::Tableau {
            continue;
        }
        let prev = scenes[i - 1].subject.trim().to_string();
        if prev.is_empty() || !scenes[i].subject.trim().eq_ignore_ascii_case(&prev) {
            continue;
        }

        let start = scenes[i].paragraph_start.saturating_sub(1);
        let end = scenes[i].paragraph_end.min(paragraphs.len());
        if start >= end {
            continue;
        }
        let seg = segment::Segment {
            start,
            end,
            text: paragraphs[start..end].join("\n\n"),
        };
        let req = SegmentRequest {
            index: scenes[i].index,
            paragraph_start: scenes[i].paragraph_start,
            paragraph_end: scenes[i].paragraph_end,
            text: seg.text.clone(),
            context: segment::build_context(paragraphs, &seg, cfg.context_radius),
            previous_subject: prev,
        };
        match scene_plan::plan_segment(llm, bible, &req) {
            Ok(scene) => {
                scenes[i] = scene;
                fixed += 1;
            }
            Err(e) => log(format!("  [warn] re-plan of scene {} failed: {}", req.index, e)),
        }
    }
    fixed
}
