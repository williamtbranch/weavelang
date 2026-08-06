// src/services/illustration/render.rs
//
// Deterministic prompt rendering. This is the stage that eliminates drift:
// identity text is copied verbatim out of the frozen bible, so the same
// character is described with byte-identical words in every prompt they appear
// in. The LLM contributed only the action, framing, and mood.
//
// Pure function of (bible, scenes, config) — no I/O, no network. Which is why
// hand-editing the bible and re-rendering costs nothing.

use std::collections::HashMap;

use super::types::{
    Bible, CastMember, Character, RenderedPrompt, ResolvedState, Scene, SceneKind, TableauKind,
};

#[derive(Debug, Clone, PartialEq)]
pub enum FaceBlendMode {
    /// Name the two real people. Strongest identity anchor.
    Blend,
    /// Use the derived physiognomy description instead, for image models that
    /// refuse named real people.
    Fallback,
    Off,
}

impl FaceBlendMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "fallback" => FaceBlendMode::Fallback,
            "off" | "none" | "" => FaceBlendMode::Off,
            _ => FaceBlendMode::Blend,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub style_prefix: String,
    pub face_blend_mode: FaceBlendMode,
    pub max_focal_cards: usize,
    pub camera_repeat_limit: usize,
    /// Composition modifiers appended to `style_prefix` for tableau scenes. The
    /// art style itself is never replaced, so the book stays visually unified.
    pub tableau_style: HashMap<String, String>,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            style_prefix: "fairy tale watercolor, storybook illustration, warm lighting"
                .to_string(),
            face_blend_mode: FaceBlendMode::Blend,
            max_focal_cards: 2,
            camera_repeat_limit: 3,
            tableau_style: default_tableau_style(),
        }
    }
}

pub fn default_tableau_style() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("place".into(), "wide establishing view, no figures in the foreground".into());
    m.insert(
        "interior".into(),
        "architectural interior study, soft depth, no figures in the foreground".into(),
    );
    m.insert("crowd".into(), "distant figures, no individual faces in focus".into());
    m.insert("object".into(), "still life, shallow depth of field, single subject".into());
    m.insert("abstract".into(), "atmospheric and evocative, a single concrete subject".into());
    m
}

/// Alternate camera framings cycled through when the planner keeps returning the
/// same one. Over-constraining identity makes every image look alike; varying
/// composition is the cheap corrective.
const CAMERA_ROTATION: &[&str] = &[
    "wide establishing shot",
    "medium shot at eye level",
    "close three-quarter view",
    "low angle looking up",
    "high angle looking down",
    "over-the-shoulder view",
];

// ---------------------------------------------------------------------------
// State resolution (Phase 1: bible only; Phase 2 folds the continuity ledger)
// ---------------------------------------------------------------------------

pub fn resolve_state(character: &Character, member: &CastMember) -> ResolvedState {
    let (wardrobe_id, wardrobe_text) = character
        .resolve_wardrobe(&member.wardrobe)
        .unwrap_or_default();
    ResolvedState {
        age: character.canonical_age,
        age_phrase: character.age_phrase.clone(),
        wardrobe_id,
        wardrobe_text,
        condition: normalise_auto(&member.condition),
    }
}

fn normalise_auto(s: &str) -> String {
    let t = s.trim();
    if t.eq_ignore_ascii_case("auto") || t.eq_ignore_ascii_case("none") {
        String::new()
    } else {
        t.to_string()
    }
}

// ---------------------------------------------------------------------------
// Card rendering
// ---------------------------------------------------------------------------

/// Drop values that carry no visual information. Models answer irrelevant
/// fields with a literal "not applicable" rather than omitting them, and the
/// renderer would otherwise inject "not applicable (fur) skin" into every
/// prompt the character appears in.
pub fn usable(value: &str) -> Option<String> {
    let v = value.trim().trim_matches(',').trim();
    if v.is_empty() {
        return None;
    }
    let lower = v.to_ascii_lowercase();
    const NULLS: &[&str] = &["n/a", "na", "none", "null", "unknown", "-", "—", "nil"];
    if NULLS.contains(&lower.as_str())
        || lower.starts_with("not applicable")
        || lower.starts_with("n/a")
    {
        return None;
    }
    Some(v.to_string())
}

/// Anchors a non-human character to animal anatomy. Without this the image
/// model happily grafts a human head onto a bear.
fn species_anchor(character: &Character) -> Option<String> {
    if character.is_human() {
        return None;
    }
    let species = character.species.trim().to_lowercase();
    Some(if character.wardrobe.is_empty() {
        format!(
            "a real {}, natural animal anatomy, not anthropomorphic, \
             no human facial features and no human clothing",
            species
        )
    } else {
        format!(
            "a real {} with an animal head and animal features, never a human face",
            species
        )
    })
}

/// The full identity card. Every element here is verbatim bible text.
pub fn focal_card(
    character: &Character,
    state: &ResolvedState,
    mode: &FaceBlendMode,
) -> String {
    let human = character.is_human();
    let mut parts: Vec<String> = Vec::new();

    if let Some(age) = usable(&state.age_phrase) {
        parts.push(age);
    }
    if let Some(anchor) = species_anchor(character) {
        parts.push(anchor);
    }
    if let Some(build) = usable(&character.build) {
        parts.push(build);
    }
    // "hair" and "skin" are human nouns; an animal's coat description already
    // carries its own ('thick shaggy brown fur').
    let hair = usable(&character.hair)
        .map(|h| if human { format!("{} hair", h) } else { h });
    let eyes = usable(&character.eyes).map(|e| format!("{} eyes", e));
    match (hair, eyes) {
        (Some(h), Some(e)) => parts.push(format!("with {} and {}", h, e)),
        (Some(h), None) => parts.push(format!("with {}", h)),
        (None, Some(e)) => parts.push(format!("with {}", e)),
        (None, None) => {}
    }
    if let Some(skin) = usable(&character.skin) {
        parts.push(if human { format!("{} skin", skin) } else { skin });
    }
    if let Some(face) = face_clause(character, mode) {
        parts.push(face);
    }
    for inv in &character.invariants {
        if let Some(i) = usable(inv) {
            parts.push(i);
        }
    }

    let mut card = if parts.is_empty() {
        character.name.clone()
    } else {
        format!("{}, {}", character.name, parts.join(", "))
    };

    if let Some(w) = usable(&state.wardrobe_text) {
        card.push_str(&format!(" — wearing {}", w));
    }
    if let Some(c) = usable(&state.condition) {
        card.push_str(&format!(", {}", c));
    }
    card
}

/// One clause for background cast, so several present characters do not swamp
/// the scene description.
pub fn compact_clause(character: &Character, state: &ResolvedState) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(age) = usable(&state.age_phrase) {
        parts.push(age);
    }
    if let Some(anchor) = species_anchor(character) {
        parts.push(anchor);
    }
    if let Some(w) = usable(&state.wardrobe_text) {
        parts.push(format!("in {}", w));
    }
    if let Some(c) = usable(&state.condition) {
        parts.push(c);
    }
    if parts.is_empty() {
        character.name.clone()
    } else {
        format!("{}, {}", character.name, parts.join(", "))
    }
}

fn face_clause(character: &Character, mode: &FaceBlendMode) -> Option<String> {
    // Naming real people for a non-human character is exactly the failure this
    // system exists to prevent: "Nick Offerman" on a bear yields a bear with a
    // man's head (and, apparently, a cigar).
    if !character.is_human() {
        return None;
    }
    match mode {
        FaceBlendMode::Off => None,
        FaceBlendMode::Fallback => usable(&character.blend_fallback),
        FaceBlendMode::Blend => {
            let members: Vec<String> = character
                .face_blend
                .iter()
                .filter_map(|m| usable(m))
                .collect();
            match members.len() {
                0 => usable(&character.blend_fallback),
                1 => Some(format!("a face resembling {}", members[0])),
                _ => {
                    let base = format!(
                        "a face blending {} and {}",
                        members[0], members[1]
                    );
                    match usable(&character.blend_note) {
                        Some(note) => Some(format!("{} ({})", base, note)),
                        None => Some(base),
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scene rendering
// ---------------------------------------------------------------------------

/// Render every scene, applying camera rotation across the sequence.
pub fn render_all(bible: &Bible, scenes: &[Scene], cfg: &RenderConfig) -> Vec<RenderedPrompt> {
    let mut out = Vec::with_capacity(scenes.len());
    let mut last_camera = String::new();
    let mut repeat_run = 0usize;
    let mut rotation_cursor = 0usize;

    for scene in scenes {
        let camera = if scene.camera.trim().is_empty() {
            String::new()
        } else if scene.camera.trim().eq_ignore_ascii_case(last_camera.trim()) {
            repeat_run += 1;
            if cfg.camera_repeat_limit > 0 && repeat_run >= cfg.camera_repeat_limit {
                let alt = CAMERA_ROTATION[rotation_cursor % CAMERA_ROTATION.len()].to_string();
                rotation_cursor += 1;
                repeat_run = 0;
                alt
            } else {
                scene.camera.trim().to_string()
            }
        } else {
            repeat_run = 0;
            scene.camera.trim().to_string()
        };
        last_camera = scene.camera.trim().to_string();

        out.push(render_scene(bible, scene, cfg, &camera));
    }
    out
}

pub fn render_scene(
    bible: &Bible,
    scene: &Scene,
    cfg: &RenderConfig,
    camera: &str,
) -> RenderedPrompt {
    match scene.kind {
        SceneKind::Cast => render_cast_scene(bible, scene, cfg, camera),
        SceneKind::Tableau => render_tableau_scene(bible, scene, cfg, camera),
    }
}

fn render_cast_scene(
    bible: &Bible,
    scene: &Scene,
    cfg: &RenderConfig,
    camera: &str,
) -> RenderedPrompt {
    // Rank by declared focal flag, then bible prominence.
    let mut members: Vec<(&CastMember, &Character)> = scene
        .cast
        .iter()
        .filter_map(|m| bible.get(&m.id).map(|c| (m, c)))
        .collect();
    members.sort_by(|a, b| {
        b.0.focal
            .cmp(&a.0.focal)
            .then_with(|| {
                b.1.prominence
                    .partial_cmp(&a.1.prominence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut focal_cards = Vec::new();
    let mut background = Vec::new();
    let mut cast_ids = Vec::new();
    let mut wardrobes = Vec::new();

    for (i, (member, character)) in members.iter().enumerate() {
        let state = resolve_state(character, member);
        cast_ids.push(character.id.clone());
        wardrobes.push(state.wardrobe_id.clone());
        if i < cfg.max_focal_cards {
            focal_cards.push(focal_card(character, &state, &cfg.face_blend_mode));
        } else {
            background.push(compact_clause(character, &state));
        }
    }

    // Minor cast referenced by the planner but not in the full bible.
    for member in &scene.cast {
        if bible.get(&member.id).is_none() {
            if let Some(minor) = bible.get_minor(&member.id) {
                let clause = if minor.clause.trim().is_empty() {
                    minor.name.clone()
                } else {
                    format!("{}, {}", minor.name, minor.clause.trim())
                };
                background.push(clause);
                cast_ids.push(minor.id.clone());
                wardrobes.push(String::new());
            }
        }
    }

    let mut sentences: Vec<String> = vec![cfg.style_prefix.trim().trim_end_matches('.').to_string()];
    if !focal_cards.is_empty() {
        sentences.push(focal_cards.join(". "));
    }
    if !background.is_empty() {
        sentences.push(format!("Also present: {}", background.join("; ")));
    }
    if let Some(loc) = location_text(bible, &scene.location) {
        sentences.push(loc);
    }
    if !scene.action.trim().is_empty() {
        sentences.push(scene.action.trim().trim_end_matches('.').to_string());
    }
    if let Some(shot) = shot_clause(camera, &scene.time_of_day) {
        sentences.push(shot);
    }
    if !scene.mood.trim().is_empty() {
        sentences.push(scene.mood.trim().trim_end_matches('.').to_string());
    }

    RenderedPrompt {
        index: scene.index,
        text: join_sentences(&sentences),
        style: cfg.style_prefix.clone(),
        paragraph_start: scene.paragraph_start,
        paragraph_end: scene.paragraph_end,
        cast: cast_ids,
        wardrobe: wardrobes,
        ..Default::default()
    }
}

fn render_tableau_scene(
    bible: &Bible,
    scene: &Scene,
    cfg: &RenderConfig,
    camera: &str,
) -> RenderedPrompt {
    let kind = scene.tableau_kind.unwrap_or(TableauKind::Place);
    let modifier = cfg
        .tableau_style
        .get(kind.as_str())
        .cloned()
        .unwrap_or_default();

    let head = if modifier.trim().is_empty() {
        cfg.style_prefix.trim().trim_end_matches('.').to_string()
    } else {
        format!(
            "{}, {}",
            cfg.style_prefix.trim().trim_end_matches('.'),
            modifier.trim().trim_end_matches('.')
        )
    };

    let mut sentences: Vec<String> = vec![head];

    if let Some(loc) = location_text(bible, &scene.location) {
        sentences.push(loc);
    }

    // Ensembles are injected verbatim, exactly like character cards, so crowds
    // stay consistent between images of the same event.
    let ensemble_clauses: Vec<String> = scene
        .ensembles
        .iter()
        .filter_map(|id| bible.get_ensemble(id))
        .filter(|e| !e.text.trim().is_empty())
        .map(|e| e.text.trim().trim_end_matches('.').to_string())
        .collect();
    if !ensemble_clauses.is_empty() {
        sentences.push(ensemble_clauses.join("; "));
    }

    if !scene.subject.trim().is_empty() {
        sentences.push(scene.subject.trim().trim_end_matches('.').to_string());
    }
    if !scene.action.trim().is_empty() {
        sentences.push(scene.action.trim().trim_end_matches('.').to_string());
    }
    if let Some(shot) = shot_clause(camera, &scene.time_of_day) {
        sentences.push(shot);
    }
    if !scene.mood.trim().is_empty() {
        sentences.push(scene.mood.trim().trim_end_matches('.').to_string());
    }

    RenderedPrompt {
        index: scene.index,
        text: join_sentences(&sentences),
        style: cfg.style_prefix.clone(),
        paragraph_start: scene.paragraph_start,
        paragraph_end: scene.paragraph_end,
        cast: Vec::new(),
        wardrobe: Vec::new(),
        ..Default::default()
    }
}

fn location_text(bible: &Bible, location: &str) -> Option<String> {
    let raw = location.trim();
    if raw.is_empty() {
        return None;
    }
    // Prefer the frozen bible entry; fall back to whatever the planner wrote.
    if let Some(loc) = bible.get_location(raw) {
        if !loc.text.trim().is_empty() {
            return Some(loc.text.trim().trim_end_matches('.').to_string());
        }
        if !loc.name.trim().is_empty() {
            return Some(loc.name.trim().to_string());
        }
    }
    Some(raw.trim_end_matches('.').to_string())
}

fn shot_clause(camera: &str, time_of_day: &str) -> Option<String> {
    let cam = camera.trim().trim_end_matches('.');
    let tod = time_of_day.trim().trim_end_matches('.');
    match (cam.is_empty(), tod.is_empty()) {
        (true, true) => None,
        (false, true) => Some(cam.to_string()),
        (true, false) => Some(format!("{} light", tod)),
        (false, false) => Some(format!("{}, {} light", cam, tod)),
    }
}

fn join_sentences(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| format!("{}.", p.trim_end_matches('.')))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::illustration::types::Wardrobe;
    use std::collections::BTreeMap;

    fn cosette() -> Character {
        let mut wardrobe = BTreeMap::new();
        wardrobe.insert(
            "montfermeil".to_string(),
            Wardrobe { text: "a ragged brown smock and wooden sabots".into(), default: true },
        );
        wardrobe.insert(
            "convent".to_string(),
            Wardrobe { text: "a plain black boarder's gown".into(), default: false },
        );
        Character {
            id: "cosette".into(),
            name: "Cosette".into(),
            prominence: 0.9,
            canonical_age: Some(8),
            age_phrase: "an 8-year-old girl".into(),
            face_blend: vec!["Person One".into(), "Person Two".into()],
            blend_note: "the bone structure of the first".into(),
            blend_fallback: "large grey eyes, thin face".into(),
            hair: "ash-blonde, unevenly cut".into(),
            eyes: "grey, unusually large".into(),
            skin: "pale".into(),
            build: "slight and small for her age".into(),
            invariants: vec!["unusually large eyes".into()],
            wardrobe,
            ..Default::default()
        }
    }

    fn bible() -> Bible {
        Bible { schema_version: 1, characters: vec![cosette()], ..Default::default() }
    }

    /// A talking animal from a folk tale: no wardrobe, no human face.
    fn bear() -> Character {
        Character {
            id: "el_oso".into(),
            name: "El oso".into(),
            species: "brown bear".into(),
            prominence: 0.8,
            age_phrase: "an adult bear".into(),
            hair: "thick shaggy brown fur".into(),
            eyes: "small, dark and beady".into(),
            build: "large, heavy and powerful".into(),
            invariants: vec!["pale muzzle".into()],
            ..Default::default()
        }
    }

    fn member(id: &str) -> CastMember {
        CastMember {
            id: id.into(),
            focal: true,
            wardrobe: "auto".into(),
            condition: "auto".into(),
        }
    }

    fn cast_scene() -> Scene {
        Scene {
            index: 1,
            paragraph_start: 0,
            paragraph_end: 10,
            kind: SceneKind::Cast,
            cast: vec![CastMember {
                id: "cosette".into(),
                focal: true,
                wardrobe: "auto".into(),
                condition: "auto".into(),
            }],
            location: "a snow-covered forest road".into(),
            action: "drags a bucket of water twice her size".into(),
            camera: "low three-quarter shot".into(),
            time_of_day: "night".into(),
            mood: "cold and desolate".into(),
            ..Default::default()
        }
    }

    #[test]
    fn focal_card_contains_age_phrase_and_invariants_verbatim() {
        let c = cosette();
        let state = resolve_state(&c, &cast_scene().cast[0]);
        let card = focal_card(&c, &state, &FaceBlendMode::Blend);
        assert!(card.contains("an 8-year-old girl"));
        assert!(card.contains("unusually large eyes"));
        assert!(card.contains("a face blending Person One and Person Two"));
        assert!(card.contains("a ragged brown smock"));
    }

    #[test]
    fn animals_never_get_a_real_person_face_blend() {
        // Regression: "a face blending Nick Offerman and John C. Reilly" on a
        // bear rendered a bear with a man's head.
        let mut c = bear();
        c.face_blend = vec!["Nick Offerman".into(), "John C. Reilly".into()];
        c.blend_fallback = "a broad human face".into();
        let state = resolve_state(&c, &member("el_oso"));

        for mode in [FaceBlendMode::Blend, FaceBlendMode::Fallback, FaceBlendMode::Off] {
            let card = focal_card(&c, &state, &mode);
            assert!(!card.contains("Nick Offerman"), "{:?}: {}", mode, card);
            assert!(!card.contains("face blending"), "{:?}: {}", mode, card);
            assert!(!card.contains("human face"), "{:?}: {}", mode, card);
        }
    }

    #[test]
    fn unclothed_animals_are_anchored_to_animal_anatomy() {
        let c = bear();
        let state = resolve_state(&c, &member("el_oso"));
        let card = focal_card(&c, &state, &FaceBlendMode::Blend);
        assert!(card.contains("a real brown bear"));
        assert!(card.contains("not anthropomorphic"));
        assert!(card.contains("no human facial features"));
        assert!(card.contains("an adult bear"));
    }

    #[test]
    fn a_clothed_animal_keeps_its_wardrobe_but_still_has_an_animal_head() {
        let mut c = bear();
        c.wardrobe.insert(
            "waistcoat".into(),
            Wardrobe { text: "a green velvet waistcoat".into(), default: true },
        );
        let state = resolve_state(&c, &member("el_oso"));
        let card = focal_card(&c, &state, &FaceBlendMode::Blend);
        assert!(card.contains("a green velvet waistcoat"));
        assert!(card.contains("an animal head"));
        assert!(!card.contains("not anthropomorphic"));
    }

    #[test]
    fn animal_coat_is_not_suffixed_with_the_human_noun() {
        let c = bear();
        let state = resolve_state(&c, &member("el_oso"));
        let card = focal_card(&c, &state, &FaceBlendMode::Blend);
        assert!(card.contains("with thick shaggy brown fur and"));
        assert!(!card.contains("fur hair"));
        // Humans keep the noun.
        let h = cosette();
        let hs = resolve_state(&h, &cast_scene().cast[0]);
        assert!(focal_card(&h, &hs, &FaceBlendMode::Blend).contains("ash-blonde, unevenly cut hair"));
    }

    #[test]
    fn placeholder_field_values_are_never_rendered() {
        // Regression: "not applicable (fur) skin" was being injected verbatim.
        let mut c = bear();
        c.skin = "not applicable (fur)".into();
        c.build = "N/A".into();
        c.invariants = vec!["none".into(), "pale muzzle".into()];
        let state = resolve_state(&c, &member("el_oso"));
        let card = focal_card(&c, &state, &FaceBlendMode::Blend);
        assert!(!card.to_lowercase().contains("not applicable"), "{}", card);
        assert!(!card.contains("N/A"), "{}", card);
        assert!(!card.contains(", none,"), "{}", card);
        assert!(card.contains("pale muzzle"));
    }

    #[test]
    fn compact_clause_anchors_background_animals_too() {
        let c = bear();
        let state = resolve_state(&c, &member("el_oso"));
        let clause = compact_clause(&c, &state);
        assert!(clause.contains("an adult bear"));
        assert!(clause.contains("not anthropomorphic"));
    }

    #[test]
    fn fallback_mode_swaps_the_named_people_for_physiognomy() {
        let c = cosette();
        let state = resolve_state(&c, &cast_scene().cast[0]);
        let card = focal_card(&c, &state, &FaceBlendMode::Fallback);
        assert!(!card.contains("Person One"));
        assert!(card.contains("large grey eyes, thin face"));
    }

    #[test]
    fn off_mode_emits_no_face_clause() {
        let c = cosette();
        let state = resolve_state(&c, &cast_scene().cast[0]);
        let card = focal_card(&c, &state, &FaceBlendMode::Off);
        assert!(!card.contains("Person One"));
        assert!(!card.contains("large grey eyes, thin face"));
        assert!(card.contains("an 8-year-old girl"));
    }

    #[test]
    fn scene_condition_is_appended_to_the_wardrobe() {
        let c = cosette();
        let member = CastMember {
            id: "cosette".into(),
            focal: true,
            wardrobe: "convent".into(),
            condition: "soaked through and shivering".into(),
        };
        let state = resolve_state(&c, &member);
        let card = focal_card(&c, &state, &FaceBlendMode::Blend);
        assert!(card.contains("a plain black boarder's gown"));
        assert!(card.contains("soaked through and shivering"));
    }

    #[test]
    fn rendering_is_byte_stable_across_repeated_calls() {
        let b = bible();
        let scenes = vec![cast_scene()];
        let a = render_all(&b, &scenes, &RenderConfig::default());
        let c = render_all(&b, &scenes, &RenderConfig::default());
        assert_eq!(a[0].text, c[0].text);
    }

    #[test]
    fn same_character_renders_identically_in_different_scenes() {
        let b = bible();
        let mut s2 = cast_scene();
        s2.index = 2;
        s2.action = "sits alone by the hearth".into();
        s2.camera = "medium shot".into();
        let out = render_all(&b, &[cast_scene(), s2], &RenderConfig::default());

        let card = focal_card(
            &cosette(),
            &resolve_state(&cosette(), &cast_scene().cast[0]),
            &FaceBlendMode::Blend,
        );
        assert!(out[0].text.contains(&card));
        assert!(out[1].text.contains(&card), "identity text must not drift between scenes");
    }

    #[test]
    fn background_cast_beyond_the_focal_cap_is_compacted() {
        let mut b = bible();
        let mut marius = cosette();
        marius.id = "marius".into();
        marius.name = "Marius".into();
        marius.prominence = 0.1;
        b.characters.push(marius);

        let mut scene = cast_scene();
        scene.cast.push(CastMember {
            id: "marius".into(),
            focal: false,
            wardrobe: "auto".into(),
            condition: String::new(),
        });

        let cfg = RenderConfig { max_focal_cards: 1, ..RenderConfig::default() };
        let out = render_all(&b, &[scene], &cfg);
        assert!(out[0].text.contains("Also present: Marius"));
        // The compact clause still states the age.
        assert!(out[0].text.contains("Marius, an 8-year-old girl"));
    }

    #[test]
    fn camera_rotates_after_consecutive_repeats() {
        let b = bible();
        let scenes: Vec<Scene> = (1..=4)
            .map(|i| Scene { index: i, ..cast_scene() })
            .collect();
        let cfg = RenderConfig { camera_repeat_limit: 3, ..RenderConfig::default() };
        let out = render_all(&b, &scenes, &cfg);
        assert!(out[0].text.contains("low three-quarter shot"));
        assert!(
            !out[3].text.contains("low three-quarter shot"),
            "a fourth identical camera value should be rotated away"
        );
    }

    #[test]
    fn tableau_appends_a_composition_modifier_without_replacing_the_style() {
        let b = Bible {
            schema_version: 1,
            ensembles: vec![crate::services::illustration::types::Ensemble {
                id: "french_infantry".into(),
                text: "French line infantry in blue coats with white crossbelts".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let scene = Scene {
            index: 7,
            kind: SceneKind::Tableau,
            tableau_kind: Some(TableauKind::Crowd),
            ensembles: vec!["french_infantry".into()],
            subject: "a sunken road choked with fallen cavalry horses".into(),
            location: "the field of Waterloo".into(),
            camera: "high wide establishing view".into(),
            time_of_day: "dawn".into(),
            mood: "desolate".into(),
            ..Default::default()
        };
        let cfg = RenderConfig::default();
        let out = render_all(&b, &[scene], &cfg);
        let text = &out[0].text;
        assert!(text.starts_with(&cfg.style_prefix.trim().trim_end_matches('.').to_string()));
        assert!(text.contains("distant figures, no individual faces in focus"));
        assert!(text.contains("French line infantry in blue coats"));
        assert!(text.contains("a sunken road choked with fallen cavalry horses"));
        assert!(out[0].cast.is_empty());
    }

    #[test]
    fn unknown_wardrobe_id_falls_back_to_the_default_variant() {
        let c = cosette();
        let member = CastMember {
            id: "cosette".into(),
            focal: true,
            wardrobe: "nonexistent".into(),
            condition: String::new(),
        };
        let state = resolve_state(&c, &member);
        assert_eq!(state.wardrobe_id, "montfermeil");
    }
}
