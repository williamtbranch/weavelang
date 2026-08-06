// src/services/illustration/lint.rs
//
// Deterministic validation and auto-repair of rendered prompts.
//
// This is what converts "usually consistent" into "provably consistent", and is
// the concrete replacement for the manual cleanup pass. No LLM, no network —
// it runs over the whole book in milliseconds.

use once_cell::sync::Lazy;
use regex::Regex;

use super::render::{render_scene, RenderConfig};
use super::types::{Bible, RenderedPrompt, Scene, SceneKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
            Severity::Info => "info",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub scene_index: usize,
    pub rule: &'static str,
    pub message: String,
    /// True when auto-repair resolved it. Unrepaired errors are what the
    /// orchestrator escalates to a bounded re-ask.
    pub repaired: bool,
}

#[derive(Debug, Default)]
pub struct LintReport {
    pub findings: Vec<Finding>,
}

impl LintReport {
    pub fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error && !f.repaired)
            .count()
    }
    pub fn repaired(&self) -> usize {
        self.findings.iter().filter(|f| f.repaired).count()
    }
    pub fn warnings(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Warn).count()
    }
    /// Scenes with errors auto-repair could not fix; these need a re-ask.
    pub fn unrepairable_scenes(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error && !f.repaired)
            .map(|f| f.scene_index)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Phrases that must never survive into an image prompt.
static BANNED: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    let pats: Vec<(&str, &str)> = vec![
        (r"(?i)\bspeech bubbles?\b", "speech bubble"),
        (r"(?i)\bpanels?\b", "panel"),
        (r"(?i)\bcollage\b", "collage"),
        (r"(?i)\btwo scenes\b", "two scenes"),
        (r"(?i)\bsplit[- ]screen\b", "split screen"),
        (r"(?i)\bdiptych\b", "diptych"),
        (r"(?i)\bcaptions?\b", "caption"),
        (r"(?i)\bsubtitles?\b", "subtitle"),
        (r"(?i)\blettering\b", "lettering"),
        (r"(?i)\bwatermark\b", "watermark"),
    ];
    pats.into_iter()
        .filter_map(|(p, label)| Regex::new(p).ok().map(|r| (r, label)))
        .collect()
});

pub fn lint_and_repair(
    bible: &Bible,
    scenes: &[Scene],
    prompts: &mut [RenderedPrompt],
    cfg: &RenderConfig,
    max_prompt_chars: usize,
) -> LintReport {
    let mut report = LintReport::default();
    let mut previous_tableau_subject = String::new();

    for prompt in prompts.iter_mut() {
        let Some(scene) = scenes.iter().find(|s| s.index == prompt.index) else {
            continue;
        };

        match scene.kind {
            SceneKind::Cast => {
                check_identity(bible, scene, prompt, cfg, &mut report);
            }
            SceneKind::Tableau => {
                check_tableau(bible, scene, prompt, &mut previous_tableau_subject, &mut report);
            }
        }

        check_offstage_names(bible, scene, prompt, &mut report);
        check_banned_tokens(prompt, cfg, &mut report);
        check_style_prefix(prompt, cfg, &mut report);
        check_length(bible, scene, prompt, cfg, max_prompt_chars, &mut report);
    }

    report
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// Age phrases, invariants, and wardrobe validity for every focal character.
/// Repair is a re-injection of the card, which cannot fail — the card is
/// deterministic bible text.
fn check_identity(
    bible: &Bible,
    scene: &Scene,
    prompt: &mut RenderedPrompt,
    cfg: &RenderConfig,
    report: &mut LintReport,
) {
    // Cloned so the repair helpers can take `prompt` mutably while iterating.
    let focal_ids: Vec<String> = prompt.cast.iter().take(cfg.max_focal_cards).cloned().collect();

    for id in &focal_ids {
        let Some(character) = bible.get(id) else { continue };
        let member = scene.cast.iter().find(|m| &m.id == id);

        // --- age phrase ---
        if !character.age_phrase.trim().is_empty()
            && !prompt.text.contains(character.age_phrase.trim())
        {
            let state = member
                .map(|m| super::render::resolve_state(character, m))
                .unwrap_or_default();
            let card = super::render::focal_card(character, &state, &cfg.face_blend_mode);
            reinject(prompt, &card);
            report.findings.push(Finding {
                severity: Severity::Error,
                scene_index: scene.index,
                rule: "age_phrase_present",
                message: format!("'{}' missing age phrase; card re-injected", character.name),
                repaired: true,
            });
        }

        // --- invariants ---
        for inv in &character.invariants {
            let inv = inv.trim();
            if inv.is_empty() {
                continue;
            }
            if !prompt.text.to_lowercase().contains(&inv.to_lowercase()) {
                let state = member
                    .map(|m| super::render::resolve_state(character, m))
                    .unwrap_or_default();
                let card = super::render::focal_card(character, &state, &cfg.face_blend_mode);
                reinject(prompt, &card);
                report.findings.push(Finding {
                    severity: Severity::Error,
                    scene_index: scene.index,
                    rule: "invariant_present",
                    message: format!(
                        "'{}' missing invariant '{}'; card re-injected",
                        character.name, inv
                    ),
                    repaired: true,
                });
                break;
            }
        }

        // --- wardrobe validity ---
        if let Some(m) = member {
            let want = m.wardrobe.trim();
            if !want.is_empty()
                && want != "auto"
                && !character.wardrobe.contains_key(want)
            {
                report.findings.push(Finding {
                    severity: Severity::Error,
                    scene_index: scene.index,
                    rule: "wardrobe_declared",
                    message: format!(
                        "'{}' requested undeclared wardrobe '{}'; used default",
                        character.name, want
                    ),
                    repaired: true,
                });
            }
        }
    }
}

/// The dominant tableau failure is a bible character dragged in from the
/// surrounding context window. Repair is strip-only — never re-inject.
fn check_tableau(
    bible: &Bible,
    scene: &Scene,
    prompt: &mut RenderedPrompt,
    previous_subject: &mut String,
    report: &mut LintReport,
) {
    if scene.subject.trim().is_empty() {
        report.findings.push(Finding {
            severity: Severity::Error,
            scene_index: scene.index,
            rule: "tableau_subject_present",
            message: "tableau has no concrete subject".to_string(),
            repaired: false,
        });
    } else if !prompt.text.contains(scene.subject.trim().trim_end_matches('.')) {
        report.findings.push(Finding {
            severity: Severity::Error,
            scene_index: scene.index,
            rule: "tableau_subject_present",
            message: "tableau subject did not survive into the prompt".to_string(),
            repaired: false,
        });
    }

    if !scene.subject.trim().is_empty() {
        if scene.subject.trim().eq_ignore_ascii_case(previous_subject.trim())
            && !previous_subject.is_empty()
        {
            report.findings.push(Finding {
                severity: Severity::Warn,
                scene_index: scene.index,
                rule: "tableau_subject_varies",
                message: format!(
                    "repeats the previous tableau subject '{}'",
                    scene.subject.trim()
                ),
                repaired: false,
            });
        }
        *previous_subject = scene.subject.trim().to_string();
    }

    for id in &scene.ensembles {
        if bible.get_ensemble(id).is_none() {
            report.findings.push(Finding {
                severity: Severity::Error,
                scene_index: scene.index,
                rule: "ensemble_declared",
                message: format!("undeclared ensemble '{}'; dropped", id),
                repaired: true,
            });
        }
    }
}

/// No character may be named who is not in the scene's resolved cast. The
/// context window mentions neighbours constantly, so this fires often.
fn check_offstage_names(
    bible: &Bible,
    scene: &Scene,
    prompt: &mut RenderedPrompt,
    report: &mut LintReport,
) {
    let onstage: Vec<&String> = scene.cast.iter().map(|m| &m.id).collect();

    for (id, name) in bible.all_names() {
        if onstage.iter().any(|o| **o == id) {
            continue;
        }
        let trimmed = name.trim();
        // Skip short or generic aliases; they produce false positives.
        if trimmed.len() < 4 || !trimmed.chars().next().is_some_and(|c| c.is_uppercase()) {
            continue;
        }
        let Ok(re) = Regex::new(&format!(r"\b{}\b", regex::escape(trimmed))) else {
            continue;
        };
        if re.is_match(&prompt.text) {
            prompt.text = re.replace_all(&prompt.text, "a figure").to_string();
            report.findings.push(Finding {
                severity: Severity::Error,
                scene_index: scene.index,
                rule: "no_offstage_cast",
                message: format!("'{}' is not in this scene; name removed", trimmed),
                repaired: true,
            });
        }
    }
}

/// Banned phrases are removed by dropping the sentence containing them. The
/// style prefix sentence is protected so the repair cannot destroy the prompt.
fn check_banned_tokens(prompt: &mut RenderedPrompt, cfg: &RenderConfig, report: &mut LintReport) {
    let mut hits: Vec<&'static str> = Vec::new();
    for (re, label) in BANNED.iter() {
        if re.is_match(&prompt.text) {
            hits.push(label);
        }
    }
    if hits.is_empty() {
        return;
    }

    let style_head = cfg.style_prefix.trim().trim_end_matches('.').to_lowercase();
    let kept: Vec<String> = split_sentences(&prompt.text)
        .into_iter()
        .filter(|s| {
            let is_style = s.to_lowercase().starts_with(&style_head);
            is_style || !BANNED.iter().any(|(re, _)| re.is_match(s))
        })
        .collect();

    prompt.text = kept.join(" ");
    report.findings.push(Finding {
        severity: Severity::Error,
        scene_index: prompt.index,
        rule: "no_banned_tokens",
        message: format!("removed banned wording: {}", hits.join(", ")),
        repaired: true,
    });
}

fn check_style_prefix(prompt: &mut RenderedPrompt, cfg: &RenderConfig, report: &mut LintReport) {
    let head = cfg.style_prefix.trim().trim_end_matches('.');
    if head.is_empty() {
        return;
    }
    let count = prompt.text.matches(head).count();
    if count == 1 {
        return;
    }
    if count == 0 {
        prompt.text = format!("{}. {}", head, prompt.text);
        report.findings.push(Finding {
            severity: Severity::Error,
            scene_index: prompt.index,
            rule: "style_prefix_once",
            message: "style prefix missing; prepended".to_string(),
            repaired: true,
        });
        return;
    }
    // Keep the first occurrence, drop the rest.
    if let Some(first_end) = prompt.text.find(head).map(|i| i + head.len()) {
        let (keep, tail) = prompt.text.split_at(first_end);
        let cleaned = tail.replace(head, "");
        prompt.text = format!("{}{}", keep, cleaned);
    }
    report.findings.push(Finding {
        severity: Severity::Error,
        scene_index: prompt.index,
        rule: "style_prefix_once",
        message: format!("style prefix appeared {} times; de-duplicated", count),
        repaired: true,
    });
}

/// Over-length prompts are shortened by demoting focal cards to compact
/// clauses, which is exactly the trade the plan calls for.
fn check_length(
    bible: &Bible,
    scene: &Scene,
    prompt: &mut RenderedPrompt,
    cfg: &RenderConfig,
    max_chars: usize,
    report: &mut LintReport,
) {
    if max_chars == 0 || prompt.text.chars().count() <= max_chars {
        return;
    }
    let original = prompt.text.chars().count();
    let mut cards = cfg.max_focal_cards;
    while cards > 0 {
        cards -= 1;
        let demoted = RenderConfig { max_focal_cards: cards, ..cfg.clone() };
        let candidate = render_scene(bible, scene, &demoted, &scene.camera);
        if candidate.text.chars().count() <= max_chars || cards == 0 {
            prompt.text = candidate.text;
            prompt.cast = candidate.cast;
            prompt.wardrobe = candidate.wardrobe;
            break;
        }
    }
    let repaired = prompt.text.chars().count() <= max_chars;
    report.findings.push(Finding {
        severity: Severity::Warn,
        scene_index: scene.index,
        rule: "max_prompt_chars",
        message: format!(
            "{} chars exceeded limit {}; demoted focal cards to {}",
            original, max_chars, cards
        ),
        repaired,
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn reinject(prompt: &mut RenderedPrompt, card: &str) {
    let card = card.trim().trim_end_matches('.');
    if card.is_empty() || prompt.text.contains(card) {
        return;
    }
    prompt.text = format!("{} {}.", prompt.text.trim(), card);
}

/// Split on sentence boundaries. Rendered prompts use ". " separators, so this
/// is exact for our own output and good enough for repaired text.
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        if c == '.' {
            match chars.peek() {
                Some(' ') | None => {
                    out.push(current.trim().to_string());
                    current.clear();
                }
                _ => {}
            }
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::illustration::render::render_all;
    use crate::services::illustration::types::{
        CastMember, Character, Ensemble, TableauKind, Wardrobe,
    };
    use std::collections::BTreeMap;

    fn cosette() -> Character {
        let mut wardrobe = BTreeMap::new();
        wardrobe.insert(
            "plain".to_string(),
            Wardrobe { text: "a ragged brown smock".into(), default: true },
        );
        Character {
            id: "cosette".into(),
            name: "Cosette".into(),
            prominence: 0.9,
            canonical_age: Some(8),
            age_phrase: "an 8-year-old girl".into(),
            hair: "ash-blonde".into(),
            invariants: vec!["unusually large eyes".into()],
            wardrobe,
            ..Default::default()
        }
    }

    fn valjean() -> Character {
        Character {
            id: "valjean".into(),
            name: "Valjean".into(),
            prominence: 0.95,
            age_phrase: "a 55-year-old man".into(),
            ..Default::default()
        }
    }

    fn bible() -> Bible {
        Bible {
            schema_version: 1,
            characters: vec![cosette(), valjean()],
            ..Default::default()
        }
    }

    fn scene() -> Scene {
        Scene {
            index: 1,
            paragraph_start: 0,
            paragraph_end: 10,
            kind: SceneKind::Cast,
            cast: vec![CastMember {
                id: "cosette".into(),
                focal: true,
                wardrobe: "auto".into(),
                condition: String::new(),
            }],
            action: "drags a heavy bucket through the snow".into(),
            camera: "low shot".into(),
            mood: "cold".into(),
            ..Default::default()
        }
    }

    fn run(bible: &Bible, scenes: &[Scene]) -> (Vec<RenderedPrompt>, LintReport) {
        let cfg = RenderConfig::default();
        let mut prompts = render_all(bible, scenes, &cfg);
        let report = lint_and_repair(bible, scenes, &mut prompts, &cfg, 1200);
        (prompts, report)
    }

    #[test]
    fn clean_render_produces_no_errors() {
        let (_, report) = run(&bible(), &[scene()]);
        assert_eq!(report.errors(), 0, "{:?}", report.findings);
    }

    #[test]
    fn missing_age_phrase_is_reinjected() {
        let b = bible();
        let s = scene();
        let cfg = RenderConfig::default();
        let mut prompts = render_all(&b, &[s.clone()], &cfg);
        prompts[0].text = "A watercolour of a girl in the snow.".to_string();
        let report = lint_and_repair(&b, &[s], &mut prompts, &cfg, 1200);
        assert!(prompts[0].text.contains("an 8-year-old girl"));
        assert!(report.findings.iter().any(|f| f.rule == "age_phrase_present" && f.repaired));
    }

    #[test]
    fn missing_invariant_is_reinjected() {
        let b = bible();
        let s = scene();
        let cfg = RenderConfig::default();
        let mut prompts = render_all(&b, &[s.clone()], &cfg);
        prompts[0].text = "Cosette, an 8-year-old girl, in the snow.".to_string();
        let report = lint_and_repair(&b, &[s], &mut prompts, &cfg, 1200);
        assert!(prompts[0].text.to_lowercase().contains("unusually large eyes"));
        assert!(report.findings.iter().any(|f| f.rule == "invariant_present"));
    }

    #[test]
    fn offstage_character_names_are_stripped() {
        let b = bible();
        let mut s = scene();
        s.action = "Valjean watches Cosette from the doorway".into();
        let (prompts, report) = run(&b, &[s]);
        assert!(!prompts[0].text.contains("Valjean"));
        assert!(prompts[0].text.contains("a figure"));
        assert!(report.findings.iter().any(|f| f.rule == "no_offstage_cast" && f.repaired));
    }

    #[test]
    fn banned_tokens_are_removed_without_destroying_the_style_prefix() {
        let b = bible();
        let mut s = scene();
        s.mood = "shown in two panels with a caption".into();
        let (prompts, report) = run(&b, &[s]);
        assert!(!prompts[0].text.to_lowercase().contains("panel"));
        assert!(!prompts[0].text.to_lowercase().contains("caption"));
        assert!(prompts[0].text.contains("fairy tale watercolor"));
        assert!(report.findings.iter().any(|f| f.rule == "no_banned_tokens"));
    }

    #[test]
    fn duplicated_style_prefix_is_deduplicated() {
        let b = bible();
        let s = scene();
        let cfg = RenderConfig::default();
        let mut prompts = render_all(&b, &[s.clone()], &cfg);
        let head = cfg.style_prefix.trim().to_string();
        prompts[0].text = format!("{} {}", prompts[0].text, head);
        let report = lint_and_repair(&b, &[s], &mut prompts, &cfg, 1200);
        let head_stripped = head.trim_end_matches('.');
        assert_eq!(prompts[0].text.matches(head_stripped).count(), 1);
        assert!(report.findings.iter().any(|f| f.rule == "style_prefix_once"));
    }

    #[test]
    fn over_length_prompts_demote_focal_cards() {
        let b = bible();
        let mut s = scene();
        s.cast.push(CastMember {
            id: "valjean".into(),
            focal: true,
            wardrobe: "auto".into(),
            condition: String::new(),
        });
        let cfg = RenderConfig::default();
        let mut prompts = render_all(&b, &[s.clone()], &cfg);
        let report = lint_and_repair(&b, &[s], &mut prompts, &cfg, 80);
        assert!(report.findings.iter().any(|f| f.rule == "max_prompt_chars"));
    }

    #[test]
    fn tableau_naming_a_bible_character_is_an_error() {
        let b = bible();
        let s = Scene {
            index: 2,
            kind: SceneKind::Tableau,
            tableau_kind: Some(TableauKind::Place),
            subject: "a sunken road choked with fallen horses".into(),
            mood: "where Valjean once walked".into(),
            ..Default::default()
        };
        let (prompts, report) = run(&b, &[s]);
        assert!(!prompts[0].text.contains("Valjean"));
        assert!(report.findings.iter().any(|f| f.rule == "no_offstage_cast"));
    }

    #[test]
    fn tableau_without_a_concrete_subject_is_unrepairable() {
        let b = bible();
        let s = Scene {
            index: 3,
            kind: SceneKind::Tableau,
            tableau_kind: Some(TableauKind::Abstract),
            subject: String::new(),
            mood: "meditative".into(),
            ..Default::default()
        };
        let (_, report) = run(&b, &[s]);
        assert_eq!(report.errors(), 1);
        assert_eq!(report.unrepairable_scenes(), vec![3]);
    }

    #[test]
    fn repeated_tableau_subjects_warn() {
        let b = bible();
        let mk = |i: usize| Scene {
            index: i,
            kind: SceneKind::Tableau,
            tableau_kind: Some(TableauKind::Abstract),
            subject: "a chalk mark on a wall".into(),
            ..Default::default()
        };
        let (_, report) = run(&b, &[mk(1), mk(2)]);
        assert!(report.findings.iter().any(|f| f.rule == "tableau_subject_varies"));
    }

    #[test]
    fn undeclared_ensembles_are_reported() {
        let mut b = bible();
        b.ensembles.push(Ensemble {
            id: "known".into(),
            text: "soldiers in blue coats".into(),
            ..Default::default()
        });
        let s = Scene {
            index: 4,
            kind: SceneKind::Tableau,
            tableau_kind: Some(TableauKind::Crowd),
            ensembles: vec!["known".into(), "unknown".into()],
            subject: "a barricade of paving stones".into(),
            ..Default::default()
        };
        let (_, report) = run(&b, &[s]);
        assert!(report.findings.iter().any(|f| f.rule == "ensemble_declared"));
    }

    #[test]
    fn split_sentences_handles_our_own_output_shape() {
        let s = split_sentences("One thing. Two things. Three.");
        assert_eq!(s, vec!["One thing.", "Two things.", "Three."]);
    }
}
