// src/services/illustration/scene_plan.rs
//
// Per-segment scene planning.
//
// The LLM's job here is deliberately narrow: decide who is on stage, what they
// are doing, and how the shot is framed. It never describes what a character
// looks like — that is the renderer's job, from frozen bible text. This split
// is what makes identity drift structurally impossible rather than merely
// discouraged.

use serde::Deserialize;

use super::llm::IllustrationLlm;
use super::types::{Bible, CastMember, Scene, SceneKind, TableauKind};

pub const SYSTEM_PROMPT: &str = r#"You are an illustration director. Given a passage from a story, plan ONE image.

You will be shown a CAST list. Do NOT describe what any character looks like — their
appearance is filled in automatically from a fixed reference. Describe only who is
present, what is happening, and how the shot is framed.

Respond with ONLY valid JSON in exactly this shape. No markdown fences, no commentary.

{
  "kind": "cast",
  "tableau_kind": null,
  "cast": [
    { "id": "cosette", "focal": true, "wardrobe": "auto", "condition": "" }
  ],
  "ensembles": [],
  "location": "thenardier_inn",
  "subject": "",
  "action": "drags a wooden bucket twice her size through deep snow",
  "camera": "low three-quarter shot",
  "time_of_day": "night",
  "mood": "cold and desolate"
}

FIELDS

- kind: "cast" if any listed character is physically present in THIS passage,
  otherwise "tableau".
- cast: ONLY characters physically present in this passage. Use the exact "id" from the
  CAST list. Never include someone merely mentioned, remembered, or discussed. Mark at
  most two as "focal": true — the ones the image is really about.
- wardrobe: the id of a declared wardrobe variant, or "auto" to use the character's
  default. Only override when the passage says they are dressed differently.
- condition: a scene-specific modification to their state — "mud-streaked and
  exhausted", "soaked through", "arm in a sling", "in a hooded disguise". Empty string
  if nothing has changed. Do NOT restate their normal appearance here.
- location: the id from the LOCATIONS list if one fits, otherwise a short plain
  description of the setting.
- action: what is physically happening, in a single clause. No character descriptions.
- camera: the framing — e.g. "low wide shot", "close three-quarter view",
  "high angle looking down". Vary this between images.
- time_of_day: "dawn", "midday", "dusk", "night", "overcast" etc.
- mood: two or three words of emotional register.

TABLEAU PASSAGES

Some passages have no characters in them at all — descriptions of a place, a building,
a crowd, an object, or an authorial digression. These still need an image. Set
"kind": "tableau", leave "cast" empty, and set "tableau_kind" to one of:

  "place"     a landscape or exterior
  "interior"  a building or room
  "crowd"     people are present but none of them are named characters
  "object"    a single significant thing
  "abstract"  an essayistic or digressive passage with no scene

For a tableau you MUST fill "subject" with something PHYSICALLY PRESENT in the passage
text — a road, a wall, a doorway, a tool, a body of water, a crowd of figures. You may
NOT invent a symbol, an allegory, or a metaphor. Even a purely argumentative passage
mentions concrete things; choose one of those.

If PREVIOUS SUBJECT is given, your "subject" must be clearly different from it.

For "crowd" tableaux, list ids from the ENSEMBLES list rather than describing the
group's clothing yourself.

HARD RULES

- Never name a character who is not physically present in the passage. The surrounding
  context is provided only so you understand the scene; characters mentioned there but
  absent from the passage must NOT appear.
- Never describe hair, eyes, face, age, or build. Those are supplied automatically.
- One single scene. No panels, no collages, no text or lettering in the image.
- Output ONLY the JSON object."#;

// ---------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct RawScene {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    tableau_kind: Option<String>,
    #[serde(default)]
    cast: Vec<RawCast>,
    #[serde(default)]
    ensembles: Vec<String>,
    #[serde(default)]
    location: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    camera: String,
    #[serde(default)]
    time_of_day: String,
    #[serde(default)]
    mood: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawCast {
    #[serde(default)]
    id: String,
    #[serde(default)]
    focal: bool,
    #[serde(default)]
    wardrobe: String,
    #[serde(default)]
    condition: String,
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

pub struct SegmentRequest {
    pub index: usize,
    pub paragraph_start: usize,
    pub paragraph_end: usize,
    pub text: String,
    pub context: String,
    pub previous_subject: String,
}

pub fn plan_segment(
    llm: &IllustrationLlm,
    bible: &Bible,
    req: &SegmentRequest,
) -> Result<Scene, String> {
    let user = build_user_prompt(bible, req);
    let raw: RawScene = llm.complete_json(SYSTEM_PROMPT, &user)?;
    Ok(to_scene(raw, bible, req))
}

/// Same wire format, a different director brief. The thumbnail stage plans one
/// image for the whole work rather than for a passage, but it must produce the
/// identical `Scene` shape so it renders through the same identity pipeline.
pub fn plan_with_system(
    llm: &IllustrationLlm,
    bible: &Bible,
    req: &SegmentRequest,
    system: &str,
    user: &str,
) -> Result<Scene, String> {
    let raw: RawScene = llm.complete_json(system, user)?;
    Ok(to_scene(raw, bible, req))
}

pub fn build_user_prompt(bible: &Bible, req: &SegmentRequest) -> String {
    let mut s = String::new();

    s.push_str(&cast_block(bible));
    if !bible.locations.is_empty() {
        s.push_str("\nLOCATIONS (use these ids where they fit):\n");
        for l in &bible.locations {
            let label = if l.name.is_empty() { &l.text } else { &l.name };
            s.push_str(&format!("- {}: {}\n", l.id, truncate(label, 90)));
        }
    }
    if !bible.ensembles.is_empty() {
        s.push_str("\nENSEMBLES (use these ids for crowds):\n");
        for e in &bible.ensembles {
            s.push_str(&format!("- {}: {}\n", e.id, truncate(&e.text, 90)));
        }
    }
    if !req.previous_subject.trim().is_empty() {
        s.push_str(&format!(
            "\nPREVIOUS SUBJECT (your tableau subject must differ from this):\n{}\n",
            req.previous_subject.trim()
        ));
    }

    s.push_str("\n=== PASSAGE TO ILLUSTRATE ===\n");
    s.push_str(req.text.trim());

    if !req.context.trim().is_empty() {
        s.push_str(
            "\n\n=== SURROUNDING CONTEXT (for understanding only — do NOT illustrate it, \
             and do NOT include characters who appear only here) ===\n",
        );
        s.push_str(req.context.trim());
    }
    s
}

pub fn cast_block(bible: &Bible) -> String {
    if bible.is_empty() {
        return "CAST: (none identified — treat every passage as a tableau)\n".to_string();
    }
    let mut s = String::from("CAST (use these exact ids):\n");
    for c in &bible.characters {
        let aliases = if c.aliases.is_empty() {
            String::new()
        } else {
            format!(" [also: {}]", c.aliases.join(", "))
        };
        let variants: Vec<&str> = c.wardrobe.keys().map(|k| k.as_str()).collect();
        let wardrobe = if variants.is_empty() {
            String::new()
        } else {
            format!(" | wardrobe: {}", variants.join(", "))
        };
        s.push_str(&format!("- {}: {}{}{}\n", c.id, c.name, aliases, wardrobe));
    }
    for m in &bible.minors {
        s.push_str(&format!("- {}: {} (minor)\n", m.id, m.name));
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    format!("{}…", t.chars().take(max).collect::<String>())
}

fn to_scene(raw: RawScene, bible: &Bible, req: &SegmentRequest) -> Scene {
    // Keep only cast the bible actually knows. A hallucinated id would render
    // as nothing, so dropping it here keeps the linter's on-stage set honest.
    let cast: Vec<CastMember> = raw
        .cast
        .into_iter()
        .filter(|c| {
            let id = c.id.trim();
            !id.is_empty() && (bible.get(id).is_some() || bible.get_minor(id).is_some())
        })
        .map(|c| CastMember {
            id: c.id.trim().to_string(),
            focal: c.focal,
            wardrobe: c.wardrobe.trim().to_string(),
            condition: c.condition.trim().to_string(),
        })
        .collect();

    let declared_tableau = raw.kind.trim().eq_ignore_ascii_case("tableau");
    let kind = if cast.is_empty() || declared_tableau {
        SceneKind::Tableau
    } else {
        SceneKind::Cast
    };

    // An empty cast means tableau whether the model said so or not; default the
    // subtype rather than losing the scene.
    let tableau_kind = if kind == SceneKind::Tableau {
        Some(parse_tableau_kind(raw.tableau_kind.as_deref().unwrap_or("")))
    } else {
        None
    };

    // If nothing was marked focal, promote the first two so the renderer has
    // something to build full cards from.
    let mut cast = cast;
    if kind == SceneKind::Cast && !cast.iter().any(|c| c.focal) {
        for c in cast.iter_mut().take(2) {
            c.focal = true;
        }
    }

    Scene {
        index: req.index,
        paragraph_start: req.paragraph_start,
        paragraph_end: req.paragraph_end,
        kind,
        tableau_kind,
        cast,
        ensembles: raw
            .ensembles
            .into_iter()
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .collect(),
        location: raw.location.trim().to_string(),
        subject: raw.subject.trim().to_string(),
        action: raw.action.trim().to_string(),
        camera: raw.camera.trim().to_string(),
        time_of_day: raw.time_of_day.trim().to_string(),
        mood: raw.mood.trim().to_string(),
    }
}

fn parse_tableau_kind(s: &str) -> TableauKind {
    match s.trim().to_ascii_lowercase().as_str() {
        "interior" => TableauKind::Interior,
        "crowd" => TableauKind::Crowd,
        "object" => TableauKind::Object,
        "abstract" => TableauKind::Abstract,
        _ => TableauKind::Place,
    }
}

/// Deterministic fallback used when every model attempt fails for a segment, so
/// a single bad call never leaves a gap in the video's visual coverage.
pub fn fallback_scene(bible: &Bible, req: &SegmentRequest) -> Scene {
    let snippet: String = req
        .text
        .split_whitespace()
        .take(30)
        .collect::<Vec<_>>()
        .join(" ");
    let _ = bible;
    Scene {
        index: req.index,
        paragraph_start: req.paragraph_start,
        paragraph_end: req.paragraph_end,
        kind: SceneKind::Tableau,
        tableau_kind: Some(TableauKind::Place),
        cast: Vec::new(),
        ensembles: Vec::new(),
        location: String::new(),
        subject: snippet,
        action: String::new(),
        camera: "wide establishing shot".to_string(),
        time_of_day: String::new(),
        mood: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::illustration::types::{Character, Ensemble, Location, MinorCharacter};

    fn bible() -> Bible {
        Bible {
            schema_version: 1,
            characters: vec![
                Character {
                    id: "cosette".into(),
                    name: "Cosette".into(),
                    aliases: vec!["the Lark".into()],
                    ..Default::default()
                },
                Character { id: "valjean".into(), name: "Valjean".into(), ..Default::default() },
            ],
            minors: vec![MinorCharacter {
                id: "innkeeper".into(),
                name: "the innkeeper".into(),
                clause: "a heavy man in a stained apron".into(),
            }],
            ensembles: vec![Ensemble {
                id: "village_poor".into(),
                text: "working poor in patched wool".into(),
                ..Default::default()
            }],
            locations: vec![Location {
                id: "inn".into(),
                name: "the inn".into(),
                text: "a smoke-blackened tavern room".into(),
                ..Default::default()
            }],
        }
    }

    fn req() -> SegmentRequest {
        SegmentRequest {
            index: 1,
            paragraph_start: 0,
            paragraph_end: 10,
            text: "She dragged the bucket through the snow.".into(),
            context: "Valjean waited at the inn.".into(),
            previous_subject: String::new(),
        }
    }

    #[test]
    fn user_prompt_lists_ids_aliases_and_wardrobe_options() {
        let p = build_user_prompt(&bible(), &req());
        assert!(p.contains("- cosette: Cosette [also: the Lark]"));
        assert!(p.contains("- innkeeper: the innkeeper (minor)"));
        assert!(p.contains("- village_poor:"));
        assert!(p.contains("- inn:"));
        assert!(p.contains("PASSAGE TO ILLUSTRATE"));
        assert!(p.contains("SURROUNDING CONTEXT"));
    }

    #[test]
    fn user_prompt_includes_the_previous_subject_when_present() {
        let mut r = req();
        r.previous_subject = "a chalk mark on a wall".into();
        let p = build_user_prompt(&bible(), &r);
        assert!(p.contains("PREVIOUS SUBJECT"));
        assert!(p.contains("a chalk mark on a wall"));
    }

    #[test]
    fn unknown_cast_ids_are_dropped() {
        let raw = RawScene {
            kind: "cast".into(),
            cast: vec![
                RawCast { id: "cosette".into(), focal: true, ..Default::default() },
                RawCast { id: "napoleon".into(), focal: true, ..Default::default() },
            ],
            ..Default::default()
        };
        let scene = to_scene(raw, &bible(), &req());
        assert_eq!(scene.cast.len(), 1);
        assert_eq!(scene.cast[0].id, "cosette");
    }

    #[test]
    fn empty_cast_is_forced_to_tableau() {
        let raw = RawScene { kind: "cast".into(), cast: vec![], ..Default::default() };
        let scene = to_scene(raw, &bible(), &req());
        assert_eq!(scene.kind, SceneKind::Tableau);
        assert!(scene.tableau_kind.is_some());
    }

    #[test]
    fn tableau_subtype_is_parsed_and_defaults_to_place() {
        let mk = |k: Option<&str>| {
            to_scene(
                RawScene {
                    kind: "tableau".into(),
                    tableau_kind: k.map(|s| s.to_string()),
                    ..Default::default()
                },
                &bible(),
                &req(),
            )
            .tableau_kind
            .unwrap()
        };
        assert_eq!(mk(Some("crowd")), TableauKind::Crowd);
        assert_eq!(mk(Some("ABSTRACT")), TableauKind::Abstract);
        assert_eq!(mk(Some("nonsense")), TableauKind::Place);
        assert_eq!(mk(None), TableauKind::Place);
    }

    #[test]
    fn first_two_cast_are_promoted_when_none_are_marked_focal() {
        let raw = RawScene {
            kind: "cast".into(),
            cast: vec![
                RawCast { id: "cosette".into(), focal: false, ..Default::default() },
                RawCast { id: "valjean".into(), focal: false, ..Default::default() },
            ],
            ..Default::default()
        };
        let scene = to_scene(raw, &bible(), &req());
        assert!(scene.cast.iter().all(|c| c.focal));
    }

    #[test]
    fn minor_characters_are_accepted_as_cast() {
        let raw = RawScene {
            kind: "cast".into(),
            cast: vec![RawCast { id: "innkeeper".into(), focal: true, ..Default::default() }],
            ..Default::default()
        };
        let scene = to_scene(raw, &bible(), &req());
        assert_eq!(scene.cast.len(), 1);
    }

    #[test]
    fn fallback_scene_is_a_tableau_with_a_concrete_snippet() {
        let s = fallback_scene(&bible(), &req());
        assert_eq!(s.kind, SceneKind::Tableau);
        assert!(!s.subject.is_empty());
        assert_eq!(s.index, 1);
    }

    #[test]
    fn empty_bible_still_produces_a_usable_prompt() {
        let p = build_user_prompt(&Bible::default(), &req());
        assert!(p.contains("none identified"));
    }
}
