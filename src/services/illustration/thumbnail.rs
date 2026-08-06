// src/services/illustration/thumbnail.rs
//
// YouTube thumbnail key art.
//
// Two extra prompts per work, appended after the scene prompts:
//
//   1. `_thumbnail.jpg`        — representative key art carrying the title
//   2. `_thumbnail_diglot.jpg` — the same image plus a "diglot" badge
//
// The second is generated with the first as a reference image, so it is the
// same picture with a badge added rather than a second interpretation of the
// same brief.
//
// Both are excluded from `_illustration_map.json`: the video timeline is
// derived from that file, and a title card has no place in it.

use super::llm::IllustrationLlm;
use super::render::{render_scene, RenderConfig};
use super::scene_plan::{self, SegmentRequest};
use super::types::{Bible, RenderedPrompt, Scene, KIND_THUMBNAIL};

pub const DEFAULT_FILE: &str = "_thumbnail.jpg";
pub const DEFAULT_DIGLOT_FILE: &str = "_thumbnail_diglot.jpg";
pub const DEFAULT_SIZE: &str = "1280x720";
pub const DEFAULT_LABEL: &str = "diglot";

#[derive(Debug, Clone)]
pub struct ThumbnailConfig {
    pub enabled: bool,
    /// The printable title of the work. Empty is a hard error upstream — a
    /// thumbnail without a title is not usable.
    pub story_title: String,
    /// Empty in whole-book mode; the chapter's printable name otherwise.
    pub chapter_title: String,
    pub file: String,
    pub diglot_file: String,
    /// `WIDTHxHEIGHT`. YouTube wants 1280x720 or larger at 16:9.
    pub size: String,
    pub diglot_label: String,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            story_title: String::new(),
            chapter_title: String::new(),
            file: DEFAULT_FILE.to_string(),
            diglot_file: DEFAULT_DIGLOT_FILE.to_string(),
            size: DEFAULT_SIZE.to_string(),
            diglot_label: DEFAULT_LABEL.to_string(),
        }
    }
}

pub const SYSTEM_PROMPT: &str = r#"You are an illustration director choosing the key art for a story — the single
image that will represent the whole work as a video thumbnail.

You will be shown a CAST list and a digest of the story. Do NOT describe what any
character looks like — their appearance is filled in automatically from a fixed
reference. Choose who is on stage, what they are doing, and how the shot is framed.

Respond with ONLY valid JSON in exactly this shape. No markdown fences, no commentary.

{
  "kind": "cast",
  "tableau_kind": null,
  "cast": [
    { "id": "el_oso", "focal": true, "wardrobe": "auto", "condition": "" }
  ],
  "ensembles": [],
  "location": "el_bosque",
  "subject": "",
  "action": "squares up to a tiny bird perched on a branch at eye level",
  "camera": "medium shot at eye level, subject on the right, uncluttered sky on the left",
  "time_of_day": "golden hour",
  "mood": "bold and comic"
}

WHAT MAKES GOOD KEY ART

- Choose the moment that a reader would name if asked what the story is about.
  Prefer the central confrontation, meeting, or turning point over the opening
  scene.
- At most TWO characters, and mark them "focal": true. A thumbnail is viewed at
  the size of a postage stamp; a crowd reads as mush.
- The framing must leave one uncluttered region for a title. Say so in "camera" —
  for example "subjects low and to the right, open sky across the top third".
- Prefer a strong readable silhouette, a clear focal subject, and high contrast.
- If the work genuinely has no characters, set "kind": "tableau", leave "cast"
  empty, choose a "tableau_kind", and put the single most emblematic physical
  thing in the story into "subject".

HARD RULES

- Use only ids from the CAST list. Never invent one.
- Never describe hair, eyes, face, age, species or build. Those are supplied
  automatically.
- Do NOT mention any title, words, text or type in your answer. The title is
  added afterwards and you must not plan for it beyond leaving space.
- One single scene. No panels, no collages, no borders.
- Output ONLY the JSON object."#;

/// Plan the key-art scene. One LLM call per work.
pub fn plan(
    llm: &IllustrationLlm,
    bible: &Bible,
    cfg: &ThumbnailConfig,
    paragraphs: &[String],
    index: usize,
) -> Result<Scene, String> {
    let req = SegmentRequest {
        index,
        // Key art spans the whole work; the range is recorded for provenance
        // only, since thumbnails never enter the illustration map.
        paragraph_start: 1,
        paragraph_end: paragraphs.len(),
        text: String::new(),
        context: String::new(),
        previous_subject: String::new(),
    };
    let user = build_user_prompt(bible, cfg, paragraphs);
    scene_plan::plan_with_system(llm, bible, &req, SYSTEM_PROMPT, &user)
}

/// A fallback used when the key-art call fails. Falls back to the most prominent
/// character rather than to nothing, because a thumbnail is not optional.
pub fn fallback_scene(bible: &Bible, index: usize, paragraphs: usize) -> Scene {
    use super::types::{CastMember, SceneKind, TableauKind};

    let mut ranked: Vec<&super::types::Character> = bible.characters.iter().collect();
    ranked.sort_by(|a, b| {
        b.prominence
            .partial_cmp(&a.prominence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let cast: Vec<CastMember> = ranked
        .iter()
        .take(1)
        .map(|c| CastMember {
            id: c.id.clone(),
            focal: true,
            wardrobe: String::new(),
            condition: String::new(),
        })
        .collect();

    let kind = if cast.is_empty() { SceneKind::Tableau } else { SceneKind::Cast };
    Scene {
        index,
        paragraph_start: 1,
        paragraph_end: paragraphs,
        kind,
        tableau_kind: (kind == SceneKind::Tableau).then_some(TableauKind::Place),
        cast,
        ensembles: Vec::new(),
        location: String::new(),
        subject: String::new(),
        action: String::new(),
        camera: "medium shot at eye level, subject low and to the right, \
                 open uncluttered space across the top third"
            .to_string(),
        time_of_day: String::new(),
        mood: String::new(),
    }
}

/// Render the planned scene into the pair of thumbnail prompts. Deterministic:
/// the identity text comes from the same renderer as every other prompt, so the
/// characters on the thumbnail match the ones inside the video.
pub fn render_pair(
    bible: &Bible,
    scene: &Scene,
    render_cfg: &RenderConfig,
    cfg: &ThumbnailConfig,
) -> Vec<RenderedPrompt> {
    let base = render_scene(bible, scene, render_cfg, &scene.camera);

    let mut plain = base.clone();
    plain.index = scene.index;
    plain.kind = KIND_THUMBNAIL.to_string();
    plain.file = non_empty(&cfg.file, DEFAULT_FILE);
    plain.resize = non_empty(&cfg.size, DEFAULT_SIZE);
    plain.text = format!(
        "{} {} {}",
        key_art_clause(),
        base.text.trim(),
        title_clause(&cfg.story_title, &cfg.chapter_title)
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");

    let mut diglot = plain.clone();
    diglot.index = scene.index + 1;
    diglot.file = non_empty(&cfg.diglot_file, DEFAULT_DIGLOT_FILE);
    // The plain thumbnail is fed back in as a reference so this is the same
    // picture with a badge added, not a second attempt at the same brief.
    diglot.ref_files = vec![plain.file.clone()];
    diglot.text = format!("{} {}", plain.text, diglot_clause(&cfg.diglot_label));

    vec![plain, diglot]
}

// ---------------------------------------------------------------------------
// Deterministic prompt fragments
// ---------------------------------------------------------------------------

fn key_art_clause() -> &'static str {
    "Poster-style key art for a video thumbnail, one single image, bold and \
     high in contrast so it stays readable at a small size."
}

/// The title clause. Every word the image must spell is quoted exactly, and the
/// image is told to render nothing else, because a model given a vague brief
/// invents plausible-looking gibberish text.
fn title_clause(story_title: &str, chapter_title: &str) -> String {
    let story = story_title.trim();
    let chapter = chapter_title.trim();

    let mut s = String::from(
        "Leave the upper third of the frame clear and uncluttered for a title. ",
    );
    if chapter.is_empty() || chapter.eq_ignore_ascii_case(story) {
        s.push_str(&format!(
            "Across that clear space render the exact words \"{}\" as large bold display \
             type, spelled letter for letter as written, in a clean serif face with a \
             strong outline or drop shadow so it separates from the artwork. ",
            story
        ));
    } else {
        s.push_str(&format!(
            "Across that clear space render the exact words \"{}\" as large bold display \
             type, and directly beneath it, at roughly half the size, the exact words \
             \"{}\". Spell both letter for letter as written, in a clean serif face with \
             a strong outline or drop shadow so they separate from the artwork. ",
            story, chapter
        ));
    }
    s.push_str(
        "No other words, letters, numbers or symbols appear anywhere in the image.",
    );
    s
}

fn diglot_clause(label: &str) -> String {
    let word = if label.trim().is_empty() { DEFAULT_LABEL } else { label.trim() };
    format!(
        "In the lower right corner, well clear of the title and not overlapping the \
         main subject, place a solid rounded rectangle badge in a contrasting flat \
         colour containing the single word \"{}\" in clean bold sans-serif type. \
         This badge is the only addition; everything else in the image is unchanged.",
        word
    )
}

fn non_empty(value: &str, fallback: &str) -> String {
    let v = value.trim();
    if v.is_empty() { fallback.to_string() } else { v.to_string() }
}

// ---------------------------------------------------------------------------
// Story digest
// ---------------------------------------------------------------------------

fn build_user_prompt(bible: &Bible, cfg: &ThumbnailConfig, paragraphs: &[String]) -> String {
    let mut s = String::new();
    s.push_str(&format!("WORK: {}\n", cfg.story_title.trim()));
    if !cfg.chapter_title.trim().is_empty() {
        s.push_str(&format!("CHAPTER: {}\n", cfg.chapter_title.trim()));
    }
    s.push('\n');
    s.push_str(&scene_plan::cast_block(bible));

    if !bible.locations.is_empty() {
        s.push_str("\nLOCATIONS (use these ids where they fit):\n");
        for l in &bible.locations {
            let label = if l.name.is_empty() { &l.text } else { &l.name };
            s.push_str(&format!("- {}: {}\n", l.id, first_chars(label, 90)));
        }
    }

    s.push_str("\n=== STORY DIGEST (beginning, middle and end) ===\n");
    s.push_str(&digest(paragraphs));
    s.push_str(
        "\n\nChoose the single image that best represents this whole work as a thumbnail.",
    );
    s
}

/// Beginning, middle and end rather than a prefix: the turning point of a story
/// is almost never in its first few thousand characters, and key art that shows
/// the opening scene is key art that misrepresents the book.
fn digest(paragraphs: &[String]) -> String {
    const HEAD: usize = 12;
    const MID: usize = 8;
    const TAIL: usize = 8;
    const PARA_CHARS: usize = 400;

    let n = paragraphs.len();
    if n <= HEAD + MID + TAIL {
        return paragraphs
            .iter()
            .map(|p| first_chars(p, PARA_CHARS))
            .collect::<Vec<_>>()
            .join("\n\n");
    }

    let mid_start = n / 2 - MID / 2;
    let mut out: Vec<String> = Vec::new();
    out.extend(paragraphs[..HEAD].iter().map(|p| first_chars(p, PARA_CHARS)));
    out.push("[...]".to_string());
    out.extend(
        paragraphs[mid_start..mid_start + MID]
            .iter()
            .map(|p| first_chars(p, PARA_CHARS)),
    );
    out.push("[...]".to_string());
    out.extend(paragraphs[n - TAIL..].iter().map(|p| first_chars(p, PARA_CHARS)));
    out.join("\n\n")
}

fn first_chars(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    format!("{}…", t.chars().take(max).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::illustration::types::{CastMember, Character, SceneKind};

    fn bible() -> Bible {
        Bible {
            schema_version: 1,
            characters: vec![Character {
                id: "el_oso".into(),
                name: "El oso".into(),
                species: "brown bear".into(),
                age_phrase: "an adult bear".into(),
                hair: "thick shaggy brown fur".into(),
                prominence: 0.9,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn scene() -> Scene {
        Scene {
            index: 9,
            paragraph_start: 1,
            paragraph_end: 40,
            kind: SceneKind::Cast,
            cast: vec![CastMember { id: "el_oso".into(), focal: true, ..Default::default() }],
            action: "squares up to a small bird".into(),
            camera: "medium shot at eye level".into(),
            ..Default::default()
        }
    }

    fn cfg() -> ThumbnailConfig {
        ThumbnailConfig {
            story_title: "The Willow Wren and the Bear".into(),
            ..Default::default()
        }
    }

    #[test]
    fn the_pair_shares_everything_except_the_badge() {
        let pair = render_pair(&bible(), &scene(), &RenderConfig::default(), &cfg());
        assert_eq!(pair.len(), 2);
        let (plain, diglot) = (&pair[0], &pair[1]);
        assert!(diglot.text.starts_with(&plain.text));
        assert!(diglot.text.contains("\"diglot\""));
        assert!(!plain.text.contains("diglot"));
    }

    #[test]
    fn the_diglot_thumbnail_references_the_plain_one() {
        let pair = render_pair(&bible(), &scene(), &RenderConfig::default(), &cfg());
        assert_eq!(pair[0].file, DEFAULT_FILE);
        assert_eq!(pair[1].file, DEFAULT_DIGLOT_FILE);
        assert_eq!(pair[1].ref_files, vec![DEFAULT_FILE.to_string()]);
        assert_ne!(pair[0].index, pair[1].index);
    }

    #[test]
    fn both_are_marked_as_thumbnails_and_sized_for_youtube() {
        let pair = render_pair(&bible(), &scene(), &RenderConfig::default(), &cfg());
        for p in &pair {
            assert!(p.is_thumbnail());
            assert_eq!(p.resize, DEFAULT_SIZE);
        }
    }

    #[test]
    fn the_title_is_quoted_verbatim_so_it_cannot_be_paraphrased() {
        let pair = render_pair(&bible(), &scene(), &RenderConfig::default(), &cfg());
        assert!(pair[0].text.contains("\"The Willow Wren and the Bear\""));
        assert!(pair[0].text.contains("No other words"));
    }

    #[test]
    fn a_chapter_title_is_rendered_beneath_the_work_title() {
        let mut c = cfg();
        c.story_title = "Grimm's Fairy Tales".into();
        c.chapter_title = "The Willow Wren and the Bear".into();
        let pair = render_pair(&bible(), &scene(), &RenderConfig::default(), &c);
        assert!(pair[0].text.contains("\"Grimm's Fairy Tales\""));
        assert!(pair[0].text.contains("\"The Willow Wren and the Bear\""));
    }

    #[test]
    fn a_chapter_title_equal_to_the_work_title_is_not_printed_twice() {
        let mut c = cfg();
        c.chapter_title = c.story_title.clone();
        let pair = render_pair(&bible(), &scene(), &RenderConfig::default(), &c);
        assert_eq!(pair[0].text.matches("The Willow Wren and the Bear").count(), 1);
    }

    #[test]
    fn character_identity_still_comes_from_the_bible() {
        let pair = render_pair(&bible(), &scene(), &RenderConfig::default(), &cfg());
        assert!(pair[0].text.contains("an adult bear"));
        assert!(pair[0].text.contains("thick shaggy brown fur"));
        // The species anchor is what keeps a man's head off the bear.
        assert!(pair[0].text.contains("natural animal anatomy"));
    }

    #[test]
    fn the_fallback_promotes_the_most_prominent_character() {
        let s = fallback_scene(&bible(), 9, 40);
        assert_eq!(s.kind, SceneKind::Cast);
        assert_eq!(s.cast[0].id, "el_oso");
        assert!(s.cast[0].focal);
    }

    #[test]
    fn an_empty_bible_falls_back_to_a_tableau() {
        let s = fallback_scene(&Bible::default(), 1, 10);
        assert_eq!(s.kind, SceneKind::Tableau);
        assert!(s.cast.is_empty());
    }

    #[test]
    fn the_digest_samples_the_end_of_a_long_work() {
        let paragraphs: Vec<String> = (0..200).map(|i| format!("paragraph {}", i)).collect();
        let d = digest(&paragraphs);
        assert!(d.contains("paragraph 0"));
        assert!(d.contains("paragraph 199"));
        assert!(d.contains("[...]"));
    }
}
