// src/services/illustration/bible.rs
//
// Load, save, and merge the character bible.
//
// The merge rules are the manual-edit durability guarantee: regeneration must
// never silently discard a hand edit. This replaces the previous behaviour where
// re-running character extraction overwrote the file wholesale.

use std::fs;
use std::path::Path;

use super::render::usable;
use super::types::{Bible, Character, Ensemble, Location, BIBLE_SCHEMA_VERSION};

const BIBLE_HEADER: &str = "\
# Character bible — drives illustration consistency.
#
# This file is generated, but edits are preserved. Set:
#   locked            = true   protect the ENTIRE character from regeneration
#   age_locked        = true   protect canonical_age + age_phrase
#   face_locked       = true   protect face_blend / blend_note / blend_fallback
#   appearance_locked = true   protect hair / eyes / skin / build
#
# age_phrase is injected verbatim into every prompt this character appears in.
# invariants are asserted by the linter and must survive into every prompt.
#
# Editing this file and re-running 'av generate prompts' costs ZERO API calls —
# the scene plan is cached and only the deterministic render + lint re-run.
";

/// Read a bible from disk. A missing file is not an error — it yields an empty
/// bible, which is what a first run sees.
pub fn load(path: &Path) -> Result<Bible, String> {
    if !path.exists() {
        return Ok(Bible { schema_version: BIBLE_SCHEMA_VERSION, ..Default::default() });
    }
    let text = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let mut bible: Bible = toml::from_str(&text)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
    if bible.schema_version == 0 {
        bible.schema_version = BIBLE_SCHEMA_VERSION;
    }
    Ok(bible)
}

pub fn save(path: &Path, bible: &Bible) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let body = toml::to_string_pretty(bible)
        .map_err(|e| format!("Failed to serialise bible: {}", e))?;
    fs::write(path, format!("{}\n{}", BIBLE_HEADER, body))
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(())
}

/// Merge freshly generated data into an existing bible, honouring lock flags.
///
/// - A character with `locked = true` is preserved byte-for-byte.
/// - Field-group locks preserve just those fields.
/// - Characters present in `existing` but absent from `fresh` are kept, so a
///   hand-added character is never dropped.
pub fn merge(existing: &Bible, fresh: &Bible) -> Bible {
    let mut out = Bible {
        schema_version: BIBLE_SCHEMA_VERSION,
        characters: Vec::new(),
        minors: fresh.minors.clone(),
        ensembles: merge_ensembles(&existing.ensembles, &fresh.ensembles),
        locations: merge_locations(&existing.locations, &fresh.locations),
    };

    for new_ch in &fresh.characters {
        match existing.get(&new_ch.id) {
            None => out.characters.push(new_ch.clone()),
            Some(old) if old.is_fully_locked() => out.characters.push(old.clone()),
            Some(old) => out.characters.push(merge_character(old, new_ch)),
        }
    }

    // Preserve hand-added characters the generator did not rediscover.
    for old in &existing.characters {
        if !out.characters.iter().any(|c| c.id == old.id) {
            out.characters.push(old.clone());
        }
    }

    // Preserve hand-added minors.
    for old in &existing.minors {
        if !out.minors.iter().any(|m| m.id == old.id) {
            out.minors.push(old.clone());
        }
    }

    out
}

fn merge_character(old: &Character, new: &Character) -> Character {
    let mut c = new.clone();

    // Locks always win, and always carry forward.
    c.locked = old.locked;
    c.age_locked = old.age_locked;
    c.face_locked = old.face_locked;
    c.appearance_locked = old.appearance_locked;

    if old.age_locked {
        c.canonical_age = old.canonical_age;
        c.age_phrase = old.age_phrase.clone();
        c.age_source = old.age_source.clone();
    }
    if old.face_locked {
        c.face_blend = old.face_blend.clone();
        c.blend_note = old.blend_note.clone();
        c.blend_fallback = old.blend_fallback.clone();
    }
    if old.appearance_locked {
        c.species = old.species.clone();
        c.hair = old.hair.clone();
        c.eyes = old.eyes.clone();
        c.skin = old.skin.clone();
        c.build = old.build.clone();
        c.invariants = old.invariants.clone();
    }

    // Hand-added wardrobe variants survive regeneration; on an id collision the
    // existing text wins, since it was more likely deliberately edited.
    for (id, w) in &old.wardrobe {
        c.wardrobe.insert(id.clone(), w.clone());
    }

    c
}

fn merge_ensembles(existing: &[Ensemble], fresh: &[Ensemble]) -> Vec<Ensemble> {
    let mut out: Vec<Ensemble> = fresh
        .iter()
        .map(|f| match existing.iter().find(|e| e.id == f.id) {
            Some(old) if old.locked => old.clone(),
            _ => f.clone(),
        })
        .collect();
    for old in existing {
        if !out.iter().any(|e| e.id == old.id) {
            out.push(old.clone());
        }
    }
    out
}

fn merge_locations(existing: &[Location], fresh: &[Location]) -> Vec<Location> {
    let mut out: Vec<Location> = fresh
        .iter()
        .map(|f| match existing.iter().find(|l| l.id == f.id) {
            Some(old) if old.locked => old.clone(),
            _ => f.clone(),
        })
        .collect();
    for old in existing {
        if !out.iter().any(|l| l.id == old.id) {
            out.push(old.clone());
        }
    }
    out
}

/// Normalise a display name into a stable snake_case id.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    let mut last_underscore = true; // suppress a leading underscore
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                s.push(lower);
            }
            last_underscore = false;
        } else if !last_underscore {
            s.push('_');
            last_underscore = true;
        }
    }
    while s.ends_with('_') {
        s.pop();
    }
    if s.is_empty() {
        "unnamed".to_string()
    } else {
        s
    }
}

/// Build the frozen `age_phrase`. Deterministic, so the phrase is byte-stable
/// across runs and the linter can assert it verbatim.
pub fn age_phrase_for(age: Option<u32>, noun_hint: &str) -> String {
    let noun = if noun_hint.trim().is_empty() { "person" } else { noun_hint.trim() };
    match age {
        Some(a) => format!("{} {}-year-old {}", indefinite_article(a), a, noun),
        None => format!("an adult {}", noun),
    }
}

/// "an" before numbers whose spoken form starts with a vowel sound:
/// 8 (eight), 11 (eleven), 18 (eighteen), and the eighties.
fn indefinite_article(n: u32) -> &'static str {
    if n == 8 || n == 11 || n == 18 || (80..=89).contains(&n) {
        "an"
    } else {
        "a"
    }
}

/// "an adult bear", "a newborn chick" — a life stage an illustrator can draw,
/// rather than "a 10-year-old bear", which tells them nothing.
pub fn animal_age_phrase(stage: &str, noun_hint: &str, species: &str) -> String {
    let noun = usable(noun_hint).unwrap_or_else(|| species.trim().to_string());
    if noun.is_empty() {
        return String::new();
    }
    let stage = match stage.trim().to_ascii_lowercase().as_str() {
        "newborn" | "infant" | "baby" | "hatchling" | "nestling" => "newborn",
        "young" | "juvenile" | "child" | "cub" | "chick" | "kit" | "kitten" | "puppy"
        | "lamb" | "foal" | "calf" | "piglet" | "fawn" => "young",
        "adolescent" | "teen" | "teenage" | "subadult" => "adolescent",
        "old" | "elderly" | "aged" | "ancient" => "old",
        _ => "adult",
    };
    let phrase = format!("{} {}", stage, noun);
    let article = match phrase.chars().next() {
        Some(c) if "aeiou".contains(c.to_ascii_lowercase()) => "an",
        _ => "a",
    };
    format!("{} {}", article, phrase)
}

// ---------------------------------------------------------------------------
// Schema migration — species backfill
// ---------------------------------------------------------------------------

/// Backfill `species` on records written before the field existed, and undo the
/// damage its absence causes.
///
/// An old bible classifies every character as human, so a bear is handed a blend
/// of two real actors and the image model renders a bear with a man's head. That
/// is exactly the defect `species` was added to prevent, and it silently returns
/// the moment prompts are regenerated from a stale file. Re-extracting would fix
/// it too, but costs an API call and re-decides fields the user may have edited
/// by hand; this pass is deterministic, free, and respects every lock.
///
/// Returns one note per change, for the run log.
pub fn backfill_species(bible: &mut Bible) -> Vec<String> {
    let mut notes = Vec::new();

    for c in &mut bible.characters {
        if c.locked || !c.species.trim().is_empty() {
            continue;
        }
        let species = match infer_species(c) {
            Some(s) => s,
            None => continue,
        };
        notes.push(format!(
            "'{}' is a {}, not a person — species backfilled",
            c.name, species
        ));
        c.species = species.clone();

        let had_blend = !c.face_blend.is_empty()
            || !c.blend_note.is_empty()
            || !c.blend_fallback.is_empty();
        if had_blend && c.face_locked {
            notes.push(format!(
                "  [warn] '{}' keeps its real-person face blend because face_locked = true; \
                 clear it by hand or the render will graft a human head onto a {}",
                c.name, species
            ));
        } else if had_blend {
            c.face_blend.clear();
            c.blend_note.clear();
            c.blend_fallback.clear();
            notes.push(format!("  dropped the real-person face blend from '{}'", c.name));
        }

        // A number of years describes nothing an illustrator can draw for an
        // animal, and the old extractor produced things like "a 0-year-old chicks".
        if !c.age_locked && c.canonical_age.is_some() {
            let phrase = animal_age_phrase(&species, &species, &species);
            if !phrase.is_empty() {
                notes.push(format!("  '{}' age '{}' -> '{}'", c.name, c.age_phrase, phrase));
                c.age_phrase = phrase;
            }
            c.canonical_age = None;
        }
    }

    bible.schema_version = BIBLE_SCHEMA_VERSION;
    notes
}

/// `None` means "leave it alone" — either a person, or too little evidence to
/// overrule the stored record.
fn infer_species(c: &Character) -> Option<String> {
    let noun = singular(&head_noun(&c.age_phrase));
    if HUMAN_NOUNS.contains(&noun.as_str()) {
        return None;
    }
    if CREATURE_NOUNS.contains(&noun.as_str()) {
        return Some(noun);
    }
    if !has_animal_marker(c) {
        return None;
    }
    // An unrecognised noun described as having fur or feathers is still not a
    // person; naming it loosely beats leaving a celebrity face on it.
    Some(if noun.is_empty() { "animal".to_string() } else { noun })
}

/// The last word of the age phrase: "a 10-year-old bear" -> "bear".
fn head_noun(age_phrase: &str) -> String {
    age_phrase
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_lowercase()
}

/// Only strips a plural when the singular is a creature we recognise, so
/// "mice" and "geese" are left alone rather than guessed at.
fn singular(noun: &str) -> String {
    if noun.len() > 3 && noun.ends_with('s') {
        let stem = &noun[..noun.len() - 1];
        if CREATURE_NOUNS.contains(&stem) {
            return stem.to_string();
        }
    }
    noun.to_string()
}

/// Whole-word match only. Substring matching would read "fin" out of "fingers".
fn has_animal_marker(c: &Character) -> bool {
    let mut haystack = format!("{} {} {} {}", c.hair, c.eyes, c.skin, c.build);
    for inv in &c.invariants {
        haystack.push(' ');
        haystack.push_str(inv);
    }
    haystack
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .any(|word| ANIMAL_MARKERS.contains(&word))
}

/// Nouns that name a person. A wren needs an animal anchor; a king does not,
/// even though neither of them is the word "human".
const HUMAN_NOUNS: &[&str] = &[
    "person", "human", "man", "woman", "boy", "girl", "child", "children", "lad", "lass",
    "youth", "teenager", "adult", "baby", "infant", "toddler", "lady", "gentleman",
    "king", "queen", "prince", "princess", "knight", "soldier", "farmer", "miller",
    "tailor", "hunter", "huntsman", "woodcutter", "shepherd", "fisherman", "merchant",
    "servant", "maid", "cook", "priest", "monk", "nun", "doctor", "sailor", "captain",
    "witch", "wizard", "sorcerer", "giant", "dwarf", "elf", "fairy", "angel", "ghost",
    "mother", "father", "son", "daughter", "sister", "brother", "wife", "husband",
    "grandmother", "grandfather", "widow", "beggar", "thief", "robber", "student",
];

/// Story-book creatures, used to recognise a non-human character from an
/// `age_phrase` written before `species` existed.
const CREATURE_NOUNS: &[&str] = &[
    "animal", "beast", "creature", "bird", "fish", "insect",
    "bear", "wolf", "fox", "badger", "boar", "deer", "stag", "doe", "fawn", "elk",
    "moose", "hare", "rabbit", "mouse", "rat", "squirrel", "hedgehog", "mole", "otter",
    "beaver", "weasel", "stoat", "marten", "lynx", "lion", "lioness", "tiger",
    "leopard", "panther", "elephant", "rhinoceros", "hippopotamus", "camel", "llama",
    "monkey", "ape", "gorilla", "bat", "seal", "whale", "dolphin", "shark",
    "cat", "kitten", "dog", "puppy", "hound", "horse", "pony", "mare", "stallion",
    "foal", "colt", "donkey", "mule", "ox", "bull", "cow", "calf", "goat", "kid",
    "sheep", "lamb", "ram", "ewe", "pig", "sow", "piglet", "swine",
    "wren", "sparrow", "robin", "finch", "lark", "nightingale", "swallow", "starling",
    "crow", "raven", "magpie", "jay", "owl", "eagle", "hawk", "falcon", "kite",
    "vulture", "stork", "crane", "heron", "duck", "drake", "goose", "swan", "hen",
    "rooster", "cock", "chicken", "chick", "turkey", "peacock", "dove", "pigeon",
    "parrot", "cuckoo", "woodpecker", "kingfisher",
    "frog", "toad", "snake", "serpent", "adder", "viper", "lizard", "tortoise",
    "turtle", "crocodile", "salmon", "trout", "pike", "carp", "eel", "crab",
    "lobster", "snail", "slug", "worm", "bee", "wasp", "hornet", "ant", "beetle",
    "flea", "fly", "gnat", "louse", "spider", "butterfly", "moth", "cricket",
    "grasshopper", "dragonfly", "caterpillar",
    "dragon", "griffin", "unicorn", "phoenix", "basilisk", "cub", "kit", "cockerel",
];

/// Body parts no person has. One of these in an appearance field is enough to
/// overrule a missing `species` when the noun is unfamiliar.
const ANIMAL_MARKERS: &[&str] = &[
    "fur", "furry", "furred", "feather", "feathers", "feathered", "unfeathered",
    "plumage", "beak", "beaks", "snout", "snouts", "muzzle", "paw", "paws", "pawed",
    "claw", "claws", "clawed", "talon", "talons", "hoof", "hooves", "mane", "antlers",
    "whiskers", "scaled", "scales", "scaly", "pelt", "fang", "fangs", "tusk", "tusks",
    "carapace", "gills", "fin", "fins", "flippers", "haunches", "hindquarters",
    "forelegs", "hindlegs", "bristles", "quills", "wingspan", "tail", "wings",
];

/// Enforce global uniqueness of face-blend members. Two characters sharing a
/// blend member is a direct cause of "these two look like the same person".
pub fn dedupe_face_blends(bible: &mut Bible) -> Vec<String> {
    let mut used: Vec<String> = Vec::new();
    let mut warnings = Vec::new();

    // Highest prominence keeps the contested member.
    let mut order: Vec<usize> = (0..bible.characters.len()).collect();
    order.sort_by(|&a, &b| {
        bible.characters[b]
            .prominence
            .partial_cmp(&bible.characters[a].prominence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for idx in order {
        if bible.characters[idx].face_locked || bible.characters[idx].locked {
            for m in &bible.characters[idx].face_blend {
                used.push(m.to_lowercase());
            }
            continue;
        }
        let name = bible.characters[idx].name.clone();
        let mut kept = Vec::new();
        for member in bible.characters[idx].face_blend.clone() {
            let key = member.to_lowercase();
            if used.contains(&key) {
                warnings.push(format!(
                    "face blend member '{}' already used; dropped from '{}'",
                    member, name
                ));
            } else {
                used.push(key);
                kept.push(member);
            }
        }
        bible.characters[idx].face_blend = kept;
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::illustration::types::Wardrobe;
    use std::collections::BTreeMap;

    fn character(id: &str, age: u32, hair: &str) -> Character {
        let mut wardrobe = BTreeMap::new();
        wardrobe.insert(
            "plain".to_string(),
            Wardrobe { text: "a grey smock".into(), default: true },
        );
        Character {
            id: id.into(),
            name: id.into(),
            canonical_age: Some(age),
            age_phrase: format!("a {}-year-old girl", age),
            hair: hair.into(),
            wardrobe,
            ..Default::default()
        }
    }

    #[test]
    fn slug_normalises_names() {
        assert_eq!(slug("Jean Valjean"), "jean_valjean");
        assert_eq!(slug("  The King's Horse  "), "the_king_s_horse");
        assert_eq!(slug("Éponine"), "éponine");
        assert_eq!(slug("!!!"), "unnamed");
    }

    // --- species backfill -------------------------------------------------

    /// A character exactly as the pre-`species` extractor wrote it.
    fn legacy_animal(name: &str, age: u32, noun: &str) -> Character {
        Character {
            id: slug(name),
            name: name.into(),
            canonical_age: Some(age),
            age_phrase: format!("a {}-year-old {}", age, noun),
            face_blend: vec!["Nick Offerman".into(), "John C. Reilly".into()],
            blend_note: "the gruff strength of the first".into(),
            blend_fallback: "a broad, rounded face".into(),
            hair: "thick, shaggy brown fur".into(),
            skin: "not applicable (fur)".into(),
            build: "large, heavy, powerful".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_legacy_bear_loses_its_celebrity_face() {
        let mut b = Bible {
            characters: vec![legacy_animal("El oso", 10, "bear")],
            ..Default::default()
        };
        let notes = backfill_species(&mut b);
        let c = b.get("el_oso").unwrap();
        assert_eq!(c.species, "bear");
        assert!(c.face_blend.is_empty());
        assert!(c.blend_note.is_empty());
        assert!(c.blend_fallback.is_empty());
        assert!(!notes.is_empty());
    }

    #[test]
    fn a_legacy_animal_trades_its_birthday_for_a_life_stage() {
        let mut b = Bible {
            characters: vec![legacy_animal("El oso", 10, "bear")],
            ..Default::default()
        };
        backfill_species(&mut b);
        let c = b.get("el_oso").unwrap();
        assert_eq!(c.age_phrase, "an adult bear");
        assert_eq!(c.canonical_age, None);
    }

    #[test]
    fn a_plural_juvenile_noun_is_made_singular_and_staged() {
        let mut b = Bible {
            characters: vec![legacy_animal("Los hijos del rey", 0, "chicks")],
            ..Default::default()
        };
        backfill_species(&mut b);
        let c = b.get("los_hijos_del_rey").unwrap();
        assert_eq!(c.species, "chick");
        assert_eq!(c.age_phrase, "a young chick");
    }

    /// The bird in the willow-wren tale has no fur or feathers in its record;
    /// only the noun in its age phrase gives it away.
    #[test]
    fn the_noun_alone_is_enough_when_the_appearance_fields_are_silent() {
        let mut bird = legacy_animal("El rey de los pájaros", 5, "bird");
        bird.hair = String::new();
        bird.skin = String::new();
        bird.build = "very small, agile, with delicate legs".into();
        let mut b = Bible { characters: vec![bird], ..Default::default() };
        backfill_species(&mut b);
        assert_eq!(b.get("el_rey_de_los_pájaros").unwrap().species, "bird");
    }

    #[test]
    fn people_are_left_alone() {
        let mut human = character("a", 30, "auburn");
        human.face_blend = vec!["Emma Thompson".into()];
        human.age_phrase = "a 30-year-old woman".into();
        let mut b = Bible { characters: vec![human], ..Default::default() };
        let notes = backfill_species(&mut b);
        let c = b.get("a").unwrap();
        assert_eq!(c.species, "");
        assert_eq!(c.face_blend, vec!["Emma Thompson".to_string()]);
        assert_eq!(c.canonical_age, Some(30));
        assert!(notes.is_empty());
    }

    /// "delicate fingers" must not match the marker "fin".
    #[test]
    fn a_human_noun_is_never_overruled_by_appearance_text() {
        let mut human = character("a", 30, "auburn");
        human.age_phrase = "a 30-year-old king".into();
        human.build = "slender, with delicate fingers and a fine tailored coat".into();
        let mut b = Bible { characters: vec![human], ..Default::default() };
        backfill_species(&mut b);
        assert_eq!(b.get("a").unwrap().species, "");
    }

    #[test]
    fn an_unfamiliar_creature_is_caught_by_its_anatomy() {
        let mut odd = legacy_animal("Grimalkin", 4, "grimalkin");
        odd.hair = "matted grey fur".into();
        let mut b = Bible { characters: vec![odd], ..Default::default() };
        backfill_species(&mut b);
        assert_eq!(b.get("grimalkin").unwrap().species, "grimalkin");
        assert!(b.get("grimalkin").unwrap().face_blend.is_empty());
    }

    #[test]
    fn a_hand_set_species_is_not_second_guessed() {
        let mut c = legacy_animal("Bruno", 10, "bear");
        c.species = "spectacled bear".into();
        let mut b = Bible { characters: vec![c], ..Default::default() };
        let notes = backfill_species(&mut b);
        assert_eq!(b.get("bruno").unwrap().species, "spectacled bear");
        assert!(notes.is_empty());
    }

    #[test]
    fn a_face_lock_is_honoured_but_warned_about() {
        let mut c = legacy_animal("El oso", 10, "bear");
        c.face_locked = true;
        let mut b = Bible { characters: vec![c], ..Default::default() };
        let notes = backfill_species(&mut b);
        assert_eq!(b.get("el_oso").unwrap().species, "bear");
        assert_eq!(b.get("el_oso").unwrap().face_blend.len(), 2);
        assert!(notes.iter().any(|n| n.contains("face_locked")));
    }

    #[test]
    fn a_fully_locked_character_is_not_touched_at_all() {
        let mut c = legacy_animal("El oso", 10, "bear");
        c.locked = true;
        let mut b = Bible { characters: vec![c], ..Default::default() };
        backfill_species(&mut b);
        assert_eq!(b.get("el_oso").unwrap().species, "");
        assert_eq!(b.get("el_oso").unwrap().face_blend.len(), 2);
    }

    #[test]
    fn backfill_stamps_the_current_schema_version() {
        let mut b = Bible { schema_version: 1, ..Default::default() };
        backfill_species(&mut b);
        assert_eq!(b.schema_version, BIBLE_SCHEMA_VERSION);
    }

    #[test]
    fn merge_keeps_fresh_data_when_nothing_is_locked() {
        let old = Bible { characters: vec![character("a", 8, "brown")], ..Default::default() };
        let new = Bible { characters: vec![character("a", 9, "blonde")], ..Default::default() };
        let merged = merge(&old, &new);
        assert_eq!(merged.get("a").unwrap().hair, "blonde");
        assert_eq!(merged.get("a").unwrap().canonical_age, Some(9));
    }

    #[test]
    fn fully_locked_character_is_preserved_verbatim() {
        let mut locked = character("a", 8, "brown");
        locked.locked = true;
        let old = Bible { characters: vec![locked.clone()], ..Default::default() };
        let new = Bible { characters: vec![character("a", 30, "blonde")], ..Default::default() };
        let merged = merge(&old, &new);
        assert_eq!(merged.get("a").unwrap(), &locked);
    }

    #[test]
    fn field_locks_protect_only_their_group() {
        let mut old_ch = character("a", 8, "brown");
        old_ch.age_locked = true;
        let old = Bible { characters: vec![old_ch], ..Default::default() };
        let new = Bible { characters: vec![character("a", 30, "blonde")], ..Default::default() };
        let merged = merge(&old, &new);
        let c = merged.get("a").unwrap();
        assert_eq!(c.canonical_age, Some(8), "age was locked");
        assert_eq!(c.hair, "blonde", "hair was not locked");
        assert!(c.age_locked, "lock flag carries forward");
    }

    #[test]
    fn hand_added_characters_survive_regeneration() {
        let old = Bible {
            characters: vec![character("a", 8, "brown"), character("handmade", 40, "grey")],
            ..Default::default()
        };
        let new = Bible { characters: vec![character("a", 8, "brown")], ..Default::default() };
        let merged = merge(&old, &new);
        assert!(merged.get("handmade").is_some());
    }

    #[test]
    fn hand_added_wardrobe_variants_survive() {
        let mut old_ch = character("a", 8, "brown");
        old_ch
            .wardrobe
            .insert("disguise".into(), Wardrobe { text: "a boy's coat".into(), default: false });
        let old = Bible { characters: vec![old_ch], ..Default::default() };
        let new = Bible { characters: vec![character("a", 8, "brown")], ..Default::default() };
        let merged = merge(&old, &new);
        assert!(merged.get("a").unwrap().wardrobe.contains_key("disguise"));
    }

    #[test]
    fn face_blend_members_are_globally_unique() {
        let mut a = character("a", 8, "brown");
        a.prominence = 0.9;
        a.face_blend = vec!["Person One".into(), "Person Two".into()];
        let mut b = character("b", 30, "grey");
        b.prominence = 0.2;
        b.face_blend = vec!["person one".into(), "Person Three".into()];

        let mut bible = Bible { characters: vec![a, b], ..Default::default() };
        let warnings = dedupe_face_blends(&mut bible);

        assert_eq!(bible.get("a").unwrap().face_blend.len(), 2, "higher prominence keeps both");
        assert_eq!(bible.get("b").unwrap().face_blend, vec!["Person Three".to_string()]);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn round_trips_through_toml() {
        let bible = Bible {
            schema_version: 1,
            characters: vec![character("cosette", 8, "ash blonde")],
            ..Default::default()
        };
        let text = toml::to_string_pretty(&bible).unwrap();
        let back: Bible = toml::from_str(&text).unwrap();
        assert_eq!(back.characters.len(), 1);
        assert_eq!(back.get("cosette").unwrap().canonical_age, Some(8));
        assert!(back.get("cosette").unwrap().wardrobe.contains_key("plain"));
    }
}
