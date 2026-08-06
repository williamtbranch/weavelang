// src/services/illustration/types.rs
//
// Schema types for the illustration consistency system.
// See documentation/Illustration_Consistency_Plan.md §4.
//
// Design principle: the LLM emits structured *facts* (who is present, what they
// are doing, how the shot is framed). Identity text (age, face, hair, invariants)
// is injected verbatim by the deterministic renderer from the frozen bible, so it
// cannot drift between illustrations.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BIBLE_SCHEMA_VERSION: u32 = 2;
pub const SCENES_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Character bible
// ---------------------------------------------------------------------------

/// A named outfit for a character. Wardrobe varies legitimately with the story
/// (disguise, mud, mourning, court dress); identity fields never do.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Wardrobe {
    /// The rendered clothing description, injected verbatim.
    #[serde(default)]
    pub text: String,
    /// Exactly one variant per character should be marked default.
    #[serde(default)]
    pub default: bool,
}

/// A full bible entry. Every field here is frozen at build time and injected
/// verbatim at render time.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Character {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub role: String,
    /// "human" (or empty) for people; otherwise the creature kind — "bear",
    /// "wren", "fox". Non-human characters are never given a real-person face
    /// blend, because blending two actors onto a bear produces a bear with a
    /// man's head.
    #[serde(default)]
    pub species: String,
    /// 0..1, mention count x spread. Used to rank focal cards.
    #[serde(default)]
    pub prominence: f32,

    /// Protects every field of this character from regeneration.
    #[serde(default)]
    pub locked: bool,

    // --- age -------------------------------------------------------------
    #[serde(default)]
    pub canonical_age: Option<u32>,
    /// The load-bearing field: injected verbatim into every prompt in which
    /// this character appears. This is what makes "spell out the age"
    /// automatic and assertable.
    #[serde(default)]
    pub age_phrase: String,
    /// "text" if drawn from an explicit quote, "inferred" otherwise.
    #[serde(default)]
    pub age_source: String,
    #[serde(default)]
    pub age_locked: bool,

    // --- face ------------------------------------------------------------
    /// Two real people blended for facial identity. Globally unique across the
    /// bible so no two characters share a blend member.
    #[serde(default)]
    pub face_blend: Vec<String>,
    #[serde(default)]
    pub blend_note: String,
    /// Physiognomy description used when `face_blend_mode = "fallback"`, or when
    /// the image model refuses named real people.
    #[serde(default)]
    pub blend_fallback: String,
    #[serde(default)]
    pub face_locked: bool,

    // --- appearance ------------------------------------------------------
    #[serde(default)]
    pub hair: String,
    #[serde(default)]
    pub eyes: String,
    #[serde(default)]
    pub skin: String,
    #[serde(default)]
    pub build: String,
    #[serde(default)]
    pub appearance_locked: bool,

    /// Features that must appear in every prompt (scar, missing finger).
    /// Asserted by the linter.
    #[serde(default)]
    pub invariants: Vec<String>,
    /// Source quotes backing the above. Kept for auditability in report.md.
    #[serde(default)]
    pub evidence: Vec<String>,

    /// Named outfits. Must be the last field: TOML requires values before tables.
    #[serde(default)]
    pub wardrobe: BTreeMap<String, Wardrobe>,
}

impl Character {
    /// Human characters get real-person face blends and human anatomy nouns
    /// ("hair", "skin"); everything else gets an animal anchor instead.
    pub fn is_human(&self) -> bool {
        let s = self.species.trim();
        s.is_empty() || s.eq_ignore_ascii_case("human") || s.eq_ignore_ascii_case("person")
    }

    /// The wardrobe variant marked `default`, else the first declared, else None.
    pub fn default_wardrobe(&self) -> Option<(&String, &Wardrobe)> {
        self.wardrobe
            .iter()
            .find(|(_, w)| w.default)
            .or_else(|| self.wardrobe.iter().next())
    }

    /// Resolve a wardrobe id to its text, falling back to the default variant.
    /// Returns the id actually used, so the linter can report substitutions.
    pub fn resolve_wardrobe(&self, requested: &str) -> Option<(String, String)> {
        let want = requested.trim();
        if !want.is_empty() && want != "auto" {
            if let Some(w) = self.wardrobe.get(want) {
                return Some((want.to_string(), w.text.clone()));
            }
        }
        self.default_wardrobe()
            .map(|(id, w)| (id.clone(), w.text.clone()))
    }

    /// True when any lock applies to the whole entry.
    pub fn is_fully_locked(&self) -> bool {
        self.locked
    }
}

/// A character kept out of the full bible by the prominence filter. Rendered as
/// a single clause so it does not bloat prompts.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MinorCharacter {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub clause: String,
}

/// An anonymous crowd. Not "no cast" but *unnamed* cast, and it still needs
/// consistency: the soldiers in every battle scene should match.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Ensemble {
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub era: String,
    #[serde(default)]
    pub locked: bool,
}

/// A recurring setting. Drifts exactly like characters do, so it gets the same
/// freeze-and-inject treatment.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Location {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub locked: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bible {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default, rename = "character")]
    pub characters: Vec<Character>,
    #[serde(default, rename = "minor")]
    pub minors: Vec<MinorCharacter>,
    #[serde(default, rename = "ensemble")]
    pub ensembles: Vec<Ensemble>,
    #[serde(default, rename = "location")]
    pub locations: Vec<Location>,
}

impl Bible {
    pub fn get(&self, id: &str) -> Option<&Character> {
        self.characters.iter().find(|c| c.id == id)
    }

    pub fn get_minor(&self, id: &str) -> Option<&MinorCharacter> {
        self.minors.iter().find(|m| m.id == id)
    }

    pub fn get_ensemble(&self, id: &str) -> Option<&Ensemble> {
        self.ensembles.iter().find(|e| e.id == id)
    }

    pub fn get_location(&self, id: &str) -> Option<&Location> {
        self.locations.iter().find(|l| l.id == id)
    }

    /// Every name and alias in the bible, for the linter's contamination check.
    pub fn all_names(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for c in &self.characters {
            out.push((c.id.clone(), c.name.clone()));
            for a in &c.aliases {
                out.push((c.id.clone(), a.clone()));
            }
        }
        for m in &self.minors {
            out.push((m.id.clone(), m.name.clone()));
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.characters.is_empty() && self.minors.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Scene plan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SceneKind {
    Cast,
    Tableau,
}

impl Default for SceneKind {
    fn default() -> Self {
        SceneKind::Cast
    }
}

/// Tableau segments cannot be skipped — the video requires continuous visual
/// coverage — so the only question is what to show. Sub-typing routes to a
/// different render template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TableauKind {
    /// Landscape / exterior.
    Place,
    /// Architecture / interior.
    Interior,
    /// People present but not bible cast.
    Crowd,
    /// Still life.
    Object,
    /// Essayistic or digressive passages. Must anchor to a concrete referent.
    Abstract,
}

impl Default for TableauKind {
    fn default() -> Self {
        TableauKind::Place
    }
}

impl TableauKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TableauKind::Place => "place",
            TableauKind::Interior => "interior",
            TableauKind::Crowd => "crowd",
            TableauKind::Object => "object",
            TableauKind::Abstract => "abstract",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CastMember {
    pub id: String,
    #[serde(default)]
    pub focal: bool,
    /// "auto" means resolve from the bible default (or, in Phase 2, the ledger).
    #[serde(default)]
    pub wardrobe: String,
    /// Scene-specific modification: muddy, injured, disguised, soaked.
    #[serde(default)]
    pub condition: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scene {
    pub index: usize,
    pub paragraph_start: usize,
    pub paragraph_end: usize,
    #[serde(default)]
    pub kind: SceneKind,
    #[serde(default)]
    pub tableau_kind: Option<TableauKind>,
    #[serde(default)]
    pub cast: Vec<CastMember>,
    #[serde(default)]
    pub ensembles: Vec<String>,
    #[serde(default)]
    pub location: String,
    /// For tableaux: the concrete referent physically present in the passage.
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub camera: String,
    #[serde(default)]
    pub time_of_day: String,
    #[serde(default)]
    pub mood: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScenePlan {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub chapter: String,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    /// The key-art scene backing the YouTube thumbnails. Stored here so that
    /// re-running costs no API call, exactly like the scene plan itself.
    #[serde(default)]
    pub thumbnail: Option<Scene>,
}

// ---------------------------------------------------------------------------
// Resolved state (output of the fold; Phase 1 resolves from the bible alone)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedState {
    pub age: Option<u32>,
    pub age_phrase: String,
    pub wardrobe_id: String,
    pub wardrobe_text: String,
    pub condition: String,
}

// ---------------------------------------------------------------------------
// Rendered output
// ---------------------------------------------------------------------------

/// Marks the two YouTube-thumbnail prompts. They carry title text, are written
/// to fixed filenames, and are deliberately excluded from the illustration map
/// so they never enter the video timeline.
pub const KIND_THUMBNAIL: &str = "thumbnail";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RenderedPrompt {
    pub index: usize,
    pub text: String,
    pub style: String,
    pub paragraph_start: usize,
    pub paragraph_end: usize,
    /// Character ids present, for reference-image selection at image-gen time.
    #[serde(default)]
    pub cast: Vec<String>,
    /// Parallel to `cast`: the wardrobe variant used, so the right reference
    /// portrait can be attached.
    #[serde(default)]
    pub wardrobe: Vec<String>,

    // --- optional overrides -------------------------------------------------
    // Skipped when empty so ordinary scene prompts serialise byte-identically
    // to the pre-thumbnail schema.
    /// "thumbnail" for the two key-art prompts; empty for ordinary scenes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// Output filename relative to the illustrations directory. Empty means the
    /// consumer's default of `{index:03}.png`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    /// `WIDTHxHEIGHT` to resize to after generation. Empty leaves the image as
    /// the model produced it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resize: String,
    /// Already-generated images (relative to the illustrations directory) to
    /// feed back in as references. This is how the diglot thumbnail is made to
    /// match the plain one rather than merely resemble it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ref_files: Vec<String>,
}

impl RenderedPrompt {
    pub fn is_thumbnail(&self) -> bool {
        self.kind.eq_ignore_ascii_case(KIND_THUMBNAIL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch() -> Character {
        let mut wardrobe = BTreeMap::new();
        wardrobe.insert(
            "plain".to_string(),
            Wardrobe { text: "a grey smock".into(), default: true },
        );
        wardrobe.insert(
            "court".to_string(),
            Wardrobe { text: "a green silk gown".into(), default: false },
        );
        Character {
            id: "cosette".into(),
            name: "Cosette".into(),
            wardrobe,
            ..Default::default()
        }
    }

    #[test]
    fn default_wardrobe_prefers_the_marked_variant() {
        let c = ch();
        let (id, w) = c.default_wardrobe().unwrap();
        assert_eq!(id, "plain");
        assert_eq!(w.text, "a grey smock");
    }

    #[test]
    fn resolve_wardrobe_uses_requested_variant() {
        let c = ch();
        let (id, text) = c.resolve_wardrobe("court").unwrap();
        assert_eq!(id, "court");
        assert_eq!(text, "a green silk gown");
    }

    #[test]
    fn resolve_wardrobe_falls_back_on_auto_and_on_unknown_ids() {
        let c = ch();
        assert_eq!(c.resolve_wardrobe("auto").unwrap().0, "plain");
        assert_eq!(c.resolve_wardrobe("").unwrap().0, "plain");
        assert_eq!(c.resolve_wardrobe("nonexistent").unwrap().0, "plain");
    }

    #[test]
    fn resolve_wardrobe_returns_none_when_no_variants_exist() {
        let c = Character { id: "x".into(), ..Default::default() };
        assert!(c.resolve_wardrobe("auto").is_none());
    }

    #[test]
    fn all_names_includes_aliases() {
        let bible = Bible {
            schema_version: 1,
            characters: vec![Character {
                id: "valjean".into(),
                name: "Jean Valjean".into(),
                aliases: vec!["Monsieur Madeleine".into(), "the convict".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let names: Vec<String> = bible.all_names().into_iter().map(|(_, n)| n).collect();
        assert!(names.contains(&"Jean Valjean".to_string()));
        assert!(names.contains(&"Monsieur Madeleine".to_string()));
        assert!(names.contains(&"the convict".to_string()));
    }
}
