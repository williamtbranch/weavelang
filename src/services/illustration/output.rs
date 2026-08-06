// src/services/illustration/output.rs
//
// Artifact writers. `_prompts.toml` and `_illustration_map.json` keep the exact
// shape the existing Python consumers expect, so illustration_gen.py and
// create_video.py need no changes.

use serde::Serialize;
use std::fs;
use std::path::Path;

use super::lint::{LintReport, Severity};
use super::types::{Bible, RenderedPrompt, ScenePlan};

#[derive(Serialize)]
struct PromptsFile<'a> {
    #[serde(rename = "prompt")]
    prompts: &'a [RenderedPrompt],
}

#[derive(Serialize)]
struct IllustrationMap {
    illustrations: Vec<IllustrationMapEntry>,
}

#[derive(Serialize)]
struct IllustrationMapEntry {
    index: usize,
    file: String,
    start_sentence: usize,
    end_sentence: usize,
}

pub fn write_prompts(path: &Path, prompts: &[RenderedPrompt]) -> Result<(), String> {
    ensure_parent(path)?;
    let body = toml::to_string_pretty(&PromptsFile { prompts })
        .map_err(|e| format!("Failed to serialise prompts: {}", e))?;
    let header = "# Generated illustration prompts.\n\
                  # Regenerate with 'av generate prompts'. Character appearance in these\n\
                  # prompts is rendered verbatim from _bible/characters.toml — edit the\n\
                  # bible rather than this file, then re-run (costs no API calls).\n\n";
    fs::write(path, format!("{}{}", header, body))
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

/// The video timeline is derived from this file, so the thumbnails are
/// deliberately absent: a title card has no place inside the video, and
/// `create_video.py` gives every listed entry screen time.
pub fn write_illustration_map(path: &Path, prompts: &[RenderedPrompt]) -> Result<(), String> {
    ensure_parent(path)?;
    let map = IllustrationMap {
        illustrations: prompts
            .iter()
            .filter(|p| !p.is_thumbnail())
            .map(|p| IllustrationMapEntry {
                index: p.index,
                file: format!("{:03}.png", p.index),
                start_sentence: p.paragraph_start,
                end_sentence: p.paragraph_end,
            })
            .collect(),
    };
    let json = serde_json::to_string_pretty(&map)
        .map_err(|e| format!("Failed to serialise illustration map: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

pub fn write_scenes(path: &Path, plan: &ScenePlan) -> Result<(), String> {
    ensure_parent(path)?;
    let json = serde_json::to_string_pretty(plan)
        .map_err(|e| format!("Failed to serialise scene plan: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

pub fn read_scenes(path: &Path) -> Result<ScenePlan, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&text).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// A human-readable audit of what the generator decided. Leads with the
/// character roster and face blends, because a wrong alias merge is the failure
/// mode most worth catching by eye — and the cheapest to fix, since correcting
/// the bible re-renders for free.
pub fn write_report(
    path: &Path,
    bible: &Bible,
    plan: &ScenePlan,
    lint: &LintReport,
) -> Result<(), String> {
    ensure_parent(path)?;
    let mut s = String::new();

    s.push_str("# Illustration generation report\n\n");
    s.push_str(&format!(
        "- characters: {}\n- minor characters: {}\n- locations: {}\n- ensembles: {}\n- scenes: {}\n\n",
        bible.characters.len(),
        bible.minors.len(),
        bible.locations.len(),
        bible.ensembles.len(),
        plan.scenes.len()
    ));

    s.push_str("## Cast\n\n");
    s.push_str("Check these first. A character split across two entries (or two merged \
                into one) is the highest-impact error, and fixing it is a one-line edit \
                to `_bible/characters.toml` plus a free re-render. A wrong `species` is \
                the other one to watch: it decides human vs animal anatomy.\n\n");
    s.push_str("| id | name | species | aliases | age | source | face blend | wardrobe |\n");
    s.push_str("|---|---|---|---|---|---|---|---|\n");
    for c in &bible.characters {
        s.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
            c.id,
            c.name,
            if c.is_human() { "human" } else { &c.species },
            if c.aliases.is_empty() { "—".into() } else { c.aliases.join(", ") },
            c.age_phrase,
            if c.age_source.is_empty() { "—" } else { &c.age_source },
            if c.face_blend.is_empty() { "—".into() } else { c.face_blend.join(" + ") },
            if c.wardrobe.is_empty() {
                "—".into()
            } else {
                c.wardrobe.keys().cloned().collect::<Vec<_>>().join(", ")
            },
        ));
    }

    if !bible.minors.is_empty() {
        s.push_str("\n## Minor cast\n\n");
        for m in &bible.minors {
            s.push_str(&format!("- `{}` {} — {}\n", m.id, m.name, m.clause));
        }
    }

    let inferred: Vec<&str> = bible
        .characters
        .iter()
        .filter(|c| c.age_source == "inferred")
        .map(|c| c.name.as_str())
        .collect();
    if !inferred.is_empty() {
        s.push_str(&format!(
            "\n## Inferred ages\n\nNot stated in the text; invented once and frozen: {}\n",
            inferred.join(", ")
        ));
    }

    s.push_str("\n## Scene breakdown\n\n");
    let tableaux = plan
        .scenes
        .iter()
        .filter(|s| s.kind == super::types::SceneKind::Tableau)
        .count();
    s.push_str(&format!(
        "- cast scenes: {}\n- tableau scenes: {}\n\n",
        plan.scenes.len() - tableaux,
        tableaux
    ));

    s.push_str("## Lint\n\n");
    s.push_str(&format!(
        "- unrepaired errors: {}\n- auto-repaired: {}\n- warnings: {}\n\n",
        lint.errors(),
        lint.repaired(),
        lint.warnings()
    ));
    if !lint.findings.is_empty() {
        s.push_str("| scene | severity | rule | detail | repaired |\n");
        s.push_str("|---|---|---|---|---|\n");
        for f in &lint.findings {
            s.push_str(&format!(
                "| {} | {} | `{}` | {} | {} |\n",
                f.scene_index,
                f.severity.as_str(),
                f.rule,
                f.message,
                if f.repaired { "yes" } else { "**no**" }
            ));
        }
    }

    if lint.findings.iter().any(|f| f.severity == Severity::Error && !f.repaired) {
        s.push_str(
            "\n> Unrepaired errors above could not be fixed deterministically. \
             Edit `_bible/characters.toml` or `_scenes.json` and re-run.\n",
        );
    }

    fs::write(path, s).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::illustration::types::{Scene, SceneKind, SCENES_SCHEMA_VERSION};

    fn prompts() -> Vec<RenderedPrompt> {
        vec![
            RenderedPrompt {
                index: 1,
                text: "A watercolour of a girl in the snow.".into(),
                style: "watercolour".into(),
                paragraph_start: 1,
                paragraph_end: 25,
                cast: vec!["cosette".into()],
                wardrobe: vec!["plain".into()],
                ..Default::default()
            },
            RenderedPrompt {
                index: 2,
                text: "A road at dawn.".into(),
                style: "watercolour".into(),
                paragraph_start: 26,
                paragraph_end: 50,
                cast: vec![],
                wardrobe: vec![],
                ..Default::default()
            },
        ]
    }

    #[test]
    fn prompts_toml_uses_the_existing_consumer_schema() {
        let body = toml::to_string_pretty(&PromptsFile { prompts: &prompts() }).unwrap();
        assert!(body.contains("[[prompt]]"));
        assert!(body.contains("index = 1"));
        assert!(body.contains("paragraph_start = 1"));
        assert!(body.contains("paragraph_end = 25"));
        assert!(body.contains("style ="));
        // Round-trips as valid TOML.
        let parsed: toml::Value = toml::from_str(&body).unwrap();
        assert_eq!(parsed["prompt"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn prompt_text_with_quotes_survives_serialisation() {
        let mut p = prompts();
        p[0].text = "He said \"stop\" — and a 12\" blade, plus a backslash \\ here.".into();
        let body = toml::to_string_pretty(&PromptsFile { prompts: &p }).unwrap();
        let parsed: toml::Value = toml::from_str(&body).unwrap();
        assert_eq!(parsed["prompt"][0]["text"].as_str().unwrap(), p[0].text);
    }

    #[test]
    fn illustration_map_matches_the_video_synchronisation_schema() {
        let map = IllustrationMap {
            illustrations: prompts()
                .iter()
                .map(|p| IllustrationMapEntry {
                    index: p.index,
                    file: format!("{:03}.png", p.index),
                    start_sentence: p.paragraph_start,
                    end_sentence: p.paragraph_end,
                })
                .collect(),
        };
        let json = serde_json::to_value(&map).unwrap();
        let first = &json["illustrations"][0];
        assert_eq!(first["file"], "001.png");
        assert_eq!(first["start_sentence"], 1);
        assert_eq!(first["end_sentence"], 25);
    }

    #[test]
    fn scene_prompts_serialise_without_the_optional_thumbnail_keys() {
        let body = toml::to_string_pretty(&PromptsFile { prompts: &prompts() }).unwrap();
        assert!(!body.contains("kind ="));
        assert!(!body.contains("file ="));
        assert!(!body.contains("resize ="));
        assert!(!body.contains("ref_files"));
    }

    #[test]
    fn thumbnails_are_kept_out_of_the_video_timeline() {
        let mut all = prompts();
        all.push(RenderedPrompt {
            index: 3,
            text: "Key art.".into(),
            kind: crate::services::illustration::types::KIND_THUMBNAIL.into(),
            file: "_thumbnail.jpg".into(),
            resize: "1280x720".into(),
            ..Default::default()
        });

        let dir = std::env::temp_dir().join(format!("wl_ill_map_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("_illustration_map.json");
        write_illustration_map(&path, &all).unwrap();

        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["illustrations"].as_array().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scene_plan_round_trips_through_json() {
        let plan = ScenePlan {
            schema_version: SCENES_SCHEMA_VERSION,
            chapter: "Ch01".into(),
            scenes: vec![Scene {
                index: 1,
                paragraph_start: 1,
                paragraph_end: 25,
                kind: SceneKind::Tableau,
                subject: "a road".into(),
                ..Default::default()
            }],
            thumbnail: None,
        };
        let json = serde_json::to_string(&plan).unwrap();
        let back: ScenePlan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.scenes.len(), 1);
        assert_eq!(back.scenes[0].kind, SceneKind::Tableau);
        assert_eq!(back.scenes[0].subject, "a road");
    }
}
