// src/services/illustration/extract.rs
//
// Structured bible extraction.
//
// The critical difference from the previous prose-based extraction: this emits
// *addressable fields*. Age becomes a number and a frozen phrase, wardrobe
// becomes named variants, facial identity becomes an explicit blend. Only then
// can the renderer inject them verbatim and the linter assert their presence.

use serde::Deserialize;
use std::collections::BTreeMap;

use super::bible::{age_phrase_for, animal_age_phrase, dedupe_face_blends, slug};
use super::llm::IllustrationLlm;
use super::render::usable;
use super::types::{
    Bible, Character, Ensemble, Location, MinorCharacter, Wardrobe, BIBLE_SCHEMA_VERSION,
};

pub const SYSTEM_PROMPT: &str = r#"You are a character designer building a bible for illustrating a story.
You will be given the full text of a work. Extract the visual facts needed to draw
its characters consistently across many illustrations.

Respond with ONLY valid JSON in exactly this shape. No markdown fences, no commentary.

{
  "characters": [
    {
      "name": "Cosette",
      "aliases": ["the Lark", "Euphrasie"],
      "role": "protagonist",
      "species": "human",
      "prominence": 0.9,
      "age": 8,
      "age_source": "text",
      "age_noun": "girl",
      "face_blend": ["<well-known person A>", "<well-known person B>"],
      "blend_note": "the bone structure of the first, the eyes of the second",
      "blend_fallback": "large grey eyes, thin face, high forehead, pointed chin",
      "hair": "ash-blonde, unevenly cut to the shoulder",
      "eyes": "grey, unusually large",
      "skin": "pale, faintly hollow-cheeked",
      "build": "slight, small for her age",
      "invariants": ["unusually large eyes", "thin frame"],
      "evidence": ["\"her large eyes sunken in a sort of shadow\""],
      "wardrobe": [
        { "id": "montfermeil", "text": "ragged brown smock, torn apron, bare feet in wooden sabots", "default": true },
        { "id": "convent", "text": "plain black boarder's gown, white collar, hair tied back", "default": false }
      ]
    },
    {
      "name": "the bear",
      "aliases": [],
      "role": "antagonist",
      "species": "brown bear",
      "prominence": 0.8,
      "age_stage": "adult",
      "age_noun": "bear",
      "face_blend": [],
      "blend_fallback": "",
      "hair": "thick shaggy dark brown fur, paler across the muzzle",
      "eyes": "small, dark, deep-set",
      "skin": "",
      "build": "massive and heavy-shouldered, with a pronounced hump",
      "invariants": ["pale muzzle", "heavy shoulder hump"],
      "evidence": [],
      "wardrobe": []
    }
  ],
  "minor_characters": [
    { "name": "the innkeeper", "clause": "a heavy man in a stained apron" }
  ],
  "locations": [
    { "id": "thenardier_inn", "name": "the Thenardier inn", "text": "a low, smoke-blackened tavern room with a great hearth and rough plank tables" }
  ],
  "ensembles": [
    { "id": "village_poor", "text": "working poor in patched brown and grey wool, clogs and shawls", "era": "1820s France" }
  ]
}

FIELD RULES

- name / aliases: every way the text refers to this character. Aliases matter: if the
  narrative conceals that two names are one person, still list them under one entry.
- role: protagonist | deuteragonist | antagonist | supporting | animal_companion | minor
- species: "human" for people. For anything else give the creature, as specifically as
  the text supports: "brown bear", "badger", "wren", "red fox", "beetle". This field
  decides whether the character is drawn with human or animal anatomy, so it must be
  right even for talking animals in a fairy tale.
- prominence: 0.0-1.0, how central the character is. Used to rank who gets a full
  description in a crowded scene.
- age / age_noun / age_source (HUMANS ONLY): age is an integer, the character's age at
  first appearance; estimate if not stated. age_source is "text" if the work states or
  strongly implies the age, else "inferred". age_noun is the noun for the age phrase —
  "girl", "boy", "young woman", "man", "old woman". The phrase "a 12-year-old boy" is
  built from age + age_noun and injected into EVERY prompt this character appears in.
- age_stage (NON-HUMANS ONLY): one of newborn | young | adolescent | adult | old.
  Omit "age" entirely for non-humans — "a 10-year-old bear" is meaningless in a
  picture; "an adult bear" is what an illustrator can draw. Set age_noun to the plain
  creature noun ("bear", "chick", "fox").
- face_blend: exactly TWO widely recognisable real people whose blended features give
  this character a stable face. Every person named must be UNIQUE across the whole
  bible — never reuse a person for two characters. **HUMANS ONLY. For any non-human
  character this MUST be an empty array**, because blending two real actors onto an
  animal produces an animal with a human head.
- blend_fallback: the same face described purely physically, naming no one. Used when
  the image model refuses named people. Humans only; empty for non-humans.
- hair / eyes / skin / build: BARE descriptors only. For humans write "ash-blonde,
  unevenly cut", NOT "ash-blonde hair" — the words "hair", "eyes", "skin" are added
  automatically. For non-humans put the coat in "hair" INCLUDING its own noun
  ("thick shaggy brown fur", "sleek black and white plumage", "hard chitinous shell")
  and leave "skin" empty.
- NEVER write "not applicable", "N/A", "none" or similar in any field. If a field does
  not apply, use an empty string or omit it.
- invariants: 1-3 permanent distinguishing features that must appear in every single
  illustration (a scar, a missing finger, unusual eyes, a white blaze on the muzzle).
  Keep them short and literal. Do NOT put clothing here — clothing changes. Do NOT
  restate the species; that is added automatically.
- evidence: direct quotes from the text supporting the appearance. Empty if inferred.
- wardrobe: named outfits. Give a variant for each distinct way the character is
  dressed across the work (everyday, travelling, formal, disguise, mourning). Exactly
  one must have "default": true. Describe clothing ONLY — no body, face, or age.
  **Leave this empty for animals unless the text explicitly dresses them.** A talking
  animal in a folk tale is still drawn as an ordinary animal.

WHAT TO INCLUDE

- "characters": at most the most important characters, ranked by prominence. These get
  full descriptions in prompts. Animals that speak or act are characters.
- "minor_characters": recurring but peripheral figures. One short clause each.
- "locations": recurring settings. These drift between images exactly like characters
  do, so freeze them too.
- "ensembles": anonymous groups that recur (soldiers, monks, a mob). Describe the group,
  never individuals.

GENERAL RULES

- Where the text describes appearance explicitly, use those details exactly.
- Where it does not, invent details consistent with the setting, era, and culture — and
  set age_source to "inferred". Invent ONCE and be specific; these become permanent.
- Do not include one-off bystanders or abstract entities.
- Output ONLY the JSON object."#;

// ---------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct RawBible {
    #[serde(default)]
    characters: Vec<RawCharacter>,
    #[serde(default)]
    minor_characters: Vec<RawMinor>,
    #[serde(default)]
    locations: Vec<RawLocation>,
    #[serde(default)]
    ensembles: Vec<RawEnsemble>,
}

#[derive(Debug, Deserialize, Default)]
struct RawCharacter {
    #[serde(default)]
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    role: String,
    #[serde(default)]
    species: String,
    #[serde(default)]
    prominence: f32,
    #[serde(default)]
    age: Option<u32>,
    #[serde(default)]
    age_source: String,
    #[serde(default)]
    age_noun: String,
    #[serde(default)]
    age_stage: String,
    #[serde(default)]
    face_blend: Vec<String>,
    #[serde(default)]
    blend_note: String,
    #[serde(default)]
    blend_fallback: String,
    #[serde(default)]
    hair: String,
    #[serde(default)]
    eyes: String,
    #[serde(default)]
    skin: String,
    #[serde(default)]
    build: String,
    #[serde(default)]
    invariants: Vec<String>,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(default)]
    wardrobe: Vec<RawWardrobe>,
}

#[derive(Debug, Deserialize, Default)]
struct RawWardrobe {
    #[serde(default)]
    id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    default: bool,
}

#[derive(Debug, Deserialize, Default)]
struct RawMinor {
    #[serde(default)]
    name: String,
    #[serde(default)]
    clause: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawLocation {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawEnsemble {
    #[serde(default)]
    id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    era: String,
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Build a bible from the full work text.
pub fn extract(
    llm: &IllustrationLlm,
    full_text: &str,
    max_characters: usize,
) -> Result<Bible, String> {
    let user = format!(
        "Extract the illustration bible for the following work. \
         Include at most {} entries in \"characters\".\n\n---\n\n{}",
        max_characters, full_text
    );
    let raw: RawBible = llm.complete_json(SYSTEM_PROMPT, &user)?;
    Ok(to_bible(raw, max_characters))
}

fn to_bible(raw: RawBible, max_characters: usize) -> Bible {
    let mut characters: Vec<Character> = raw
        .characters
        .into_iter()
        .filter(|c| !c.name.trim().is_empty())
        .map(to_character)
        .collect();

    // Stable ordering by prominence so report output and focal ranking agree.
    characters.sort_by(|a, b| {
        b.prominence
            .partial_cmp(&a.prominence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    // Anything past the cap becomes a one-clause minor entry rather than being
    // dropped, so the planner can still refer to it.
    let mut minors: Vec<MinorCharacter> = Vec::new();
    if max_characters > 0 && characters.len() > max_characters {
        for c in characters.split_off(max_characters) {
            minors.push(MinorCharacter {
                id: c.id.clone(),
                name: c.name.clone(),
                clause: short_clause(&c),
            });
        }
    }

    for m in raw.minor_characters {
        if m.name.trim().is_empty() {
            continue;
        }
        let id = slug(&m.name);
        if characters.iter().any(|c| c.id == id) || minors.iter().any(|x| x.id == id) {
            continue;
        }
        minors.push(MinorCharacter {
            id,
            name: m.name.trim().to_string(),
            clause: m.clause.trim().to_string(),
        });
    }

    let locations = raw
        .locations
        .into_iter()
        .filter(|l| !l.text.trim().is_empty() || !l.name.trim().is_empty())
        .map(|l| Location {
            id: if l.id.trim().is_empty() { slug(&l.name) } else { slug(&l.id) },
            name: l.name.trim().to_string(),
            text: l.text.trim().to_string(),
            locked: false,
        })
        .collect();

    let ensembles = raw
        .ensembles
        .into_iter()
        .filter(|e| !e.text.trim().is_empty())
        .map(|e| Ensemble {
            id: if e.id.trim().is_empty() { slug(&e.text) } else { slug(&e.id) },
            text: e.text.trim().to_string(),
            era: e.era.trim().to_string(),
            locked: false,
        })
        .collect();

    let mut bible = Bible {
        schema_version: BIBLE_SCHEMA_VERSION,
        characters,
        minors,
        ensembles,
        locations,
    };
    dedupe_face_blends(&mut bible);
    bible
}

fn to_character(raw: RawCharacter) -> Character {
    let name = raw.name.trim().to_string();
    let species = normalise_species(&raw.species);
    let human = species.is_empty();

    let mut wardrobe: BTreeMap<String, Wardrobe> = BTreeMap::new();
    for w in raw.wardrobe {
        if w.text.trim().is_empty() {
            continue;
        }
        let id = if w.id.trim().is_empty() { slug(&w.text) } else { slug(&w.id) };
        wardrobe.insert(id, Wardrobe { text: w.text.trim().to_string(), default: w.default });
    }
    // Exactly one default, always.
    if !wardrobe.is_empty() && !wardrobe.values().any(|w| w.default) {
        if let Some(first) = wardrobe.keys().next().cloned() {
            if let Some(w) = wardrobe.get_mut(&first) {
                w.default = true;
            }
        }
    }
    let mut seen_default = false;
    for w in wardrobe.values_mut() {
        if w.default {
            if seen_default {
                w.default = false;
            }
            seen_default = true;
        }
    }

    let age_source = if raw.age_source.trim().is_empty() {
        if raw.evidence.is_empty() { "inferred" } else { "text" }
    } else {
        raw.age_source.trim()
    }
    .to_string();

    Character {
        id: slug(&name),
        name: name.clone(),
        aliases: raw
            .aliases
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty() && !a.eq_ignore_ascii_case(&name))
            .collect(),
        role: raw.role.trim().to_string(),
        species: species.clone(),
        prominence: raw.prominence.clamp(0.0, 1.0),
        locked: false,
        // A number of years says nothing an illustrator can draw for an animal.
        canonical_age: if human { raw.age } else { None },
        age_phrase: if human {
            age_phrase_for(raw.age, raw.age_noun.trim())
        } else {
            animal_age_phrase(&raw.age_stage, raw.age_noun.trim(), &species)
        },
        age_source,
        age_locked: false,
        // Real-person blends are for human faces only.
        face_blend: if human {
            raw.face_blend
                .into_iter()
                .filter_map(|m| usable(&m))
                .take(2)
                .collect()
        } else {
            Vec::new()
        },
        blend_note: if human { clean(&raw.blend_note) } else { String::new() },
        blend_fallback: if human { clean(&raw.blend_fallback) } else { String::new() },
        face_locked: false,
        hair: clean(&strip_redundant_noun(&raw.hair, "hair")),
        eyes: clean(&strip_redundant_noun(&raw.eyes, "eyes")),
        skin: clean(&strip_redundant_noun(&raw.skin, "skin")),
        build: clean(&raw.build),
        appearance_locked: false,
        invariants: raw
            .invariants
            .into_iter()
            .filter_map(|i| usable(&i))
            .take(3)
            .collect(),
        evidence: raw
            .evidence
            .into_iter()
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .collect(),
        wardrobe,
    }
}

/// Empty means human. Anything else is the creature kind, lowercased.
fn normalise_species(raw: &str) -> String {
    match usable(raw) {
        None => String::new(),
        Some(s) => {
            let lower = s.to_lowercase();
            if lower == "human" || lower == "person" || lower == "man" || lower == "woman" {
                String::new()
            } else {
                lower
            }
        }
    }
}

fn clean(value: &str) -> String {
    usable(value).unwrap_or_default()
}

/// The renderer supplies "hair"/"eyes"/"skin", so a model that includes them
/// anyway would produce "ash-blonde hair hair".
fn strip_redundant_noun(value: &str, noun: &str) -> String {
    let v = value.trim().trim_end_matches('.').trim();
    let lower = v.to_lowercase();
    let suffix = format!(" {}", noun);
    if lower.ends_with(&suffix) {
        return v[..v.len() - suffix.len()].trim().to_string();
    }
    v.to_string()
}

fn short_clause(c: &Character) -> String {
    let mut parts = Vec::new();
    if !c.age_phrase.is_empty() {
        parts.push(c.age_phrase.clone());
    }
    if !c.hair.is_empty() {
        parts.push(if c.is_human() {
            format!("{} hair", c.hair)
        } else {
            c.hair.clone()
        });
    }
    if let Some((_, w)) = c.default_wardrobe() {
        if !w.text.is_empty() {
            parts.push(format!("in {}", w.text));
        }
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_character(name: &str, age: u32, prominence: f32) -> RawCharacter {
        RawCharacter {
            name: name.into(),
            age: Some(age),
            age_noun: "girl".into(),
            prominence,
            hair: "ash-blonde hair".into(),
            eyes: "grey".into(),
            face_blend: vec!["Person One".into(), "Person Two".into(), "Person Three".into()],
            invariants: vec!["large eyes".into(), "a".into(), "b".into(), "c".into()],
            wardrobe: vec![
                RawWardrobe { id: "Plain Dress".into(), text: "a grey smock".into(), default: false },
                RawWardrobe { id: "court".into(), text: "a silk gown".into(), default: false },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn builds_a_frozen_age_phrase_from_age_and_noun() {
        let c = to_character(raw_character("Cosette", 8, 0.9));
        assert_eq!(c.age_phrase, "an 8-year-old girl");
        assert_eq!(c.canonical_age, Some(8));
    }

    #[test]
    fn age_phrase_article_follows_the_number() {
        assert_eq!(age_phrase_for(Some(8), "girl"), "an 8-year-old girl");
        assert_eq!(age_phrase_for(Some(11), "boy"), "an 11-year-old boy");
        assert_eq!(age_phrase_for(Some(18), "woman"), "an 18-year-old woman");
        assert_eq!(age_phrase_for(Some(12), "boy"), "a 12-year-old boy");
        assert_eq!(age_phrase_for(Some(55), "man"), "a 55-year-old man");
    }

    #[test]
    fn strips_redundant_nouns_the_renderer_supplies() {
        let c = to_character(raw_character("Cosette", 8, 0.9));
        assert_eq!(c.hair, "ash-blonde", "renderer adds the word 'hair'");
        assert_eq!(c.eyes, "grey");
    }

    fn raw_bear() -> RawCharacter {
        RawCharacter {
            name: "El oso".into(),
            species: "Brown Bear".into(),
            age: Some(10),
            age_stage: "adult".into(),
            age_noun: "bear".into(),
            prominence: 0.8,
            face_blend: vec!["Nick Offerman".into(), "John C. Reilly".into()],
            blend_note: "the gruff strength of the first".into(),
            blend_fallback: "a broad, bearded human face".into(),
            hair: "thick, shaggy brown fur".into(),
            eyes: "small, dark, beady".into(),
            skin: "not applicable (fur)".into(),
            build: "large, heavy, powerful".into(),
            ..Default::default()
        }
    }

    #[test]
    fn non_humans_lose_every_real_person_reference() {
        let c = to_character(raw_bear());
        assert!(!c.is_human());
        assert_eq!(c.species, "brown bear");
        assert!(c.face_blend.is_empty(), "a bear must not wear an actor's face");
        assert!(c.blend_note.is_empty());
        assert!(c.blend_fallback.is_empty());
    }

    #[test]
    fn non_humans_get_a_life_stage_not_a_number_of_years() {
        let c = to_character(raw_bear());
        assert_eq!(c.age_phrase, "an adult bear");
        assert_eq!(c.canonical_age, None);
        assert_eq!(animal_age_phrase("nestling", "chick", "wren"), "a newborn chick");
        assert_eq!(animal_age_phrase("old", "fox", "fox"), "an old fox");
        assert_eq!(animal_age_phrase("", "", "badger"), "an adult badger");
    }

    #[test]
    fn placeholder_values_are_stripped_at_extraction() {
        let c = to_character(raw_bear());
        assert_eq!(c.skin, "", "'not applicable (fur)' must never reach the bible");
        assert_eq!(c.hair, "thick, shaggy brown fur", "an animal coat keeps its own noun");
    }

    #[test]
    fn humans_are_detected_from_an_explicit_or_missing_species() {
        assert_eq!(normalise_species("human"), "");
        assert_eq!(normalise_species("Person"), "");
        assert_eq!(normalise_species(""), "");
        assert_eq!(normalise_species("not applicable"), "");
        assert_eq!(normalise_species("Red Fox"), "red fox");
        assert!(to_character(raw_character("Cosette", 8, 0.9)).is_human());
    }

    #[test]
    fn caps_face_blends_at_two_and_invariants_at_three() {
        let c = to_character(raw_character("Cosette", 8, 0.9));
        assert_eq!(c.face_blend.len(), 2);
        assert_eq!(c.invariants.len(), 3);
    }

    #[test]
    fn guarantees_exactly_one_default_wardrobe() {
        let c = to_character(raw_character("Cosette", 8, 0.9));
        assert_eq!(c.wardrobe.values().filter(|w| w.default).count(), 1);
        assert!(c.wardrobe.contains_key("plain_dress"), "ids are slugified");
    }

    #[test]
    fn characters_beyond_the_cap_become_minor_entries() {
        let raw = RawBible {
            characters: vec![
                raw_character("A", 10, 0.9),
                raw_character("B", 20, 0.5),
                raw_character("C", 30, 0.1),
            ],
            ..Default::default()
        };
        let bible = to_bible(raw, 2);
        assert_eq!(bible.characters.len(), 2);
        assert_eq!(bible.characters[0].name, "A", "sorted by prominence");
        assert_eq!(bible.minors.len(), 1);
        assert_eq!(bible.minors[0].name, "C");
        assert!(!bible.minors[0].clause.is_empty());
    }

    #[test]
    fn face_blend_members_are_unique_across_the_bible() {
        let raw = RawBible {
            characters: vec![raw_character("A", 10, 0.9), raw_character("B", 20, 0.5)],
            ..Default::default()
        };
        let bible = to_bible(raw, 10);
        let a: Vec<String> = bible.characters[0].face_blend.iter().map(|s| s.to_lowercase()).collect();
        let b: Vec<String> = bible.characters[1].face_blend.iter().map(|s| s.to_lowercase()).collect();
        assert!(a.iter().all(|m| !b.contains(m)), "no shared blend members: {:?} / {:?}", a, b);
    }

    #[test]
    fn self_referential_aliases_are_dropped() {
        let mut raw = raw_character("Cosette", 8, 0.9);
        raw.aliases = vec!["Cosette".into(), "the Lark".into(), "  ".into()];
        let c = to_character(raw);
        assert_eq!(c.aliases, vec!["the Lark".to_string()]);
    }
}
