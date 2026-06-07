//! Tier dependency graph and stage→prompt dispatch.
//!
//! This module is the single source of truth for "which prompt should I run
//! to produce tier X, and which tier is its source?". The mapping depends on
//! whether the project is in **English-source mode** (default) or
//! **Spanish-source mode** (`source_is_target == true`). The two graphs are:
//!
//! ```text
//! English-source:
//!   advanced_target  ← base                 (advanced     | en-es)
//!   moderate_target  ← advanced_target      (moderate      | es-es)
//!   basic_base       ← base                 (basic_base    | en-en)
//!   basic_target     ← basic_base           (basic_target  | en-es)
//!
//! Spanish-source (source_is_target):
//!   advanced_target  ← base                 (advanced      | es-es, echo + segment)
//!   moderate_target  ← advanced_target      (moderate      | es-es)
//!   basic_target     ← base                 (basic_target  | es-es)
//!   basic_base       ← basic_target         (basic_base    | es-en)
//! ```
//!
//! Every prompt is one of seven standardized names — `advanced`, `segment`,
//! `moderate`, `basic_base`, `basic_target`, `basic_diglot`, `inverse_diglot`.
//! The *directory* (`{input_lang}-{output_lang}`, e.g. `en-es`, `es-es`) is
//! computed per stage by [`prompt_pair_for_stage`] from the stage's tier
//! wiring, so the same name in different directories denotes a different
//! operation (e.g. `basic_base` in `en-en` simplifies, in `es-en` translates).
//!
//! The basic-branch direction flips between the two modes; the advanced
//! branch only changes which prompt produces `advanced_target`.

/// Stage-name keys used everywhere else in the codebase. These must
/// stay stable — the GUI and CLI surface them directly.
pub const STAGE_GENERATE_BASIC_BASE: &str = "GenerateBasicBase";
pub const STAGE_GENERATE_BASIC_TARGET: &str = "GenerateBasicTarget";
pub const STAGE_GENERATE_ADVANCED_TARGET: &str = "GenerateAdvancedTarget";
pub const STAGE_GENERATE_MODERATE_TARGET: &str = "GenerateModerateTarget";
pub const STAGE_GENERATE_PHRASE_MAP: &str = "GeneratePhraseMap";
pub const STAGE_GENERATE_INVERSE_PHRASE_MAP: &str = "GenerateInversePhraseMap";

/// Resolution of a generation stage into the prompt + tier wiring used
/// to spawn an LLM job.
#[derive(Debug, Clone, Copy)]
pub struct StageResolution {
    pub prompt_name: &'static str,
    pub target_tier: &'static str,
    pub source_tier: &'static str,
    /// True for stages that only need to *segment* their source text
    /// (no translation, no simplification). When set, the LLM worker
    /// can skip the main generation prompt and run only the segmenter.
    pub segmentation_only: bool,
    /// True when this stage should bypass the LLM entirely and copy
    /// `source_tier` content into `target_tier` verbatim. Set when
    /// `source_is_basic: on` is asserted and the stage would have
    /// produced a basic tier in the source's own language
    /// (`GenerateBasicBase` for en-es; `GenerateBasicTarget` for es-es).
    /// `prompt_name` is set to [`PROMPT_PASSTHROUGH_COPY`] in that case
    /// and the LLM worker short-circuits the API call.
    pub copy_from_source_tier: bool,
}

/// Sentinel prompt name used by `copy_from_source_tier` resolutions.
/// The LLM worker recognises this and echoes input verbatim instead of
/// calling the model.
pub const PROMPT_PASSTHROUGH_COPY: &str = "passthrough_copy";

/// Resolve a stage name into its concrete prompt + tier wiring.
///
/// Returns `None` for unknown stage names. `source_is_basic` only
/// affects the in-source-language basic tier (`GenerateBasicBase` in
/// en-es mode, `GenerateBasicTarget` in es-es mode); all other stages
/// are unaffected.
pub fn stage_dispatch(
    stage_name: &str,
    source_is_target: bool,
    source_is_basic: bool,
) -> Option<StageResolution> {
    use StageResolution as R;
    let r = match (stage_name, source_is_target) {
        // ── English-source (default) ──────────────────────────────────────
        (STAGE_GENERATE_BASIC_BASE, false) if source_is_basic => R {
            // Source is already basic-level English; copy base → basic_base
            // verbatim instead of running the simplifier.
            prompt_name: PROMPT_PASSTHROUGH_COPY,
            target_tier: "basic_base",
            source_tier: "base",
            segmentation_only: false,
            copy_from_source_tier: true,
        },
        (STAGE_GENERATE_BASIC_BASE, false) => R {
            prompt_name: "basic_base",
            target_tier: "basic_base",
            source_tier: "base",
            segmentation_only: false,
            copy_from_source_tier: false,
        },
        (STAGE_GENERATE_BASIC_TARGET, false) => R {
            prompt_name: "basic_target",
            target_tier: "basic_target",
            source_tier: "basic_base",
            segmentation_only: false,
            copy_from_source_tier: false,
        },
        (STAGE_GENERATE_ADVANCED_TARGET, false) => R {
            prompt_name: "advanced",
            target_tier: "advanced_target",
            source_tier: "base",
            segmentation_only: false,
            copy_from_source_tier: false,
        },
        (STAGE_GENERATE_MODERATE_TARGET, _) => R {
            prompt_name: "moderate",
            target_tier: "moderate_target",
            source_tier: "advanced_target",
            segmentation_only: false,
            copy_from_source_tier: false,
        },
        (STAGE_GENERATE_PHRASE_MAP, _) => R {
            prompt_name: "basic_diglot",
            target_tier: "MAPPING:basic_base:basic_target",
            source_tier: "basic_base",
            segmentation_only: false,
            copy_from_source_tier: false,
        },
        (STAGE_GENERATE_INVERSE_PHRASE_MAP, _) => R {
            prompt_name: "inverse_diglot",
            target_tier: "MAPPING:basic_target:basic_base",
            source_tier: "basic_target",
            segmentation_only: false,
            copy_from_source_tier: false,
        },

        // ── Spanish-source (source_is_target) ─────────────────────────────
        (STAGE_GENERATE_ADVANCED_TARGET, true) => R {
            // No translation needed: source IS already in target language.
            // The `advanced` prompt in the same-language directory (es-es)
            // echoes the source verbatim; the worker then runs a
            // segmentation pass so advanced_target carries a clean segment
            // structure for downstream moderate_target.
            prompt_name: "advanced",
            target_tier: "advanced_target",
            source_tier: "base",
            segmentation_only: true,
            copy_from_source_tier: false,
        },
        (STAGE_GENERATE_BASIC_TARGET, true) if source_is_basic => R {
            // Source is already basic-level Spanish; copy base →
            // basic_target verbatim instead of running the simplifier.
            prompt_name: PROMPT_PASSTHROUGH_COPY,
            target_tier: "basic_target",
            source_tier: "base",
            segmentation_only: false,
            copy_from_source_tier: true,
        },
        (STAGE_GENERATE_BASIC_TARGET, true) => R {
            // basic_target is built directly from the (target-language) base.
            prompt_name: "basic_target",
            target_tier: "basic_target",
            source_tier: "base",
            segmentation_only: false,
            copy_from_source_tier: false,
        },
        (STAGE_GENERATE_BASIC_BASE, true) => R {
            // basic_base is the base-language translation of basic_target.
            // Direction reversed vs English-source.
            prompt_name: "basic_base",
            target_tier: "basic_base",
            source_tier: "basic_target",
            segmentation_only: false,
            copy_from_source_tier: false,
        },

        _ => return None,
    };
    Some(r)
}

/// Output language for a tier, taking source-language direction into
/// account.
///
/// In English-source mode this matches the legacy
/// `tier_processor::lang_for_tier`. In Spanish-source mode:
/// - `base`, `advanced_target`, `moderate_target`, `basic_target`
///   all report the target (Spanish) language.
/// - `basic_base` reports the *learner's* native language. We currently
///   hardcode this to `"en"` — the only learner language Spanish-source
///   mode supports today. (TODO: add a `learner_lang` config field for
///   non-English learners.)
pub fn lang_for_tier(
    tier_id: &str,
    base_lang: &str,
    target_lang: &str,
    source_is_target: bool,
) -> String {
    if source_is_target {
        return match tier_id {
            "basic_base" => "en".to_string(),
            "base"
            | "advanced_target"
            | "moderate_target"
            | "basic_target" => target_lang.to_string(),
            t if t.starts_with("MAPPING:") => target_lang.to_string(),
            _ => target_lang.to_string(),
        };
    }
    crate::services::tier_processor::lang_for_tier(tier_id, base_lang, target_lang)
}

/// Compute the `(input_lang, output_lang)` pair that selects the prompt
/// directory for a stage, given its tier wiring. The prompt is loaded from
/// `assets/prompts/{input_lang}-{output_lang}/{prompt_name}.txt`.
///
/// - `input_lang`  = language of `source_tier`.
/// - `output_lang` = language of `target_tier`. For `MAPPING:a:b` stages
///   (diglot / inverse-diglot) the effective output tier is `b`, so the
///   directory follows the diglot's weave direction (e.g. `en-es` for the
///   forward diglot, `es-en` for the inverse).
///
/// This is what makes the same standardized prompt name resolve to different
/// operations in different directories (e.g. `basic_base` is a simplification
/// in `en-en` but a translation in `es-en`).
pub fn prompt_pair_for_stage(
    source_tier: &str,
    target_tier: &str,
    base_lang: &str,
    target_lang: &str,
    source_is_target: bool,
) -> (String, String) {
    let input = lang_for_tier(source_tier, base_lang, target_lang, source_is_target);
    let effective_target = target_tier
        .strip_prefix("MAPPING:")
        .and_then(|rest| rest.split(':').nth(1))
        .unwrap_or(target_tier);
    let output = lang_for_tier(effective_target, base_lang, target_lang, source_is_target);
    (input, output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_source_basic_chain() {
        let bb = stage_dispatch(STAGE_GENERATE_BASIC_BASE, false, false).unwrap();
        assert_eq!(bb.target_tier, "basic_base");
        assert_eq!(bb.source_tier, "base");
        assert_eq!(bb.prompt_name, "basic_base");
        assert!(!bb.segmentation_only);
        assert!(!bb.copy_from_source_tier);

        let bt = stage_dispatch(STAGE_GENERATE_BASIC_TARGET, false, false).unwrap();
        assert_eq!(bt.target_tier, "basic_target");
        assert_eq!(bt.source_tier, "basic_base");
        assert_eq!(bt.prompt_name, "basic_target");
        assert!(!bt.copy_from_source_tier);
    }

    #[test]
    fn english_source_advanced_chain() {
        let adv = stage_dispatch(STAGE_GENERATE_ADVANCED_TARGET, false, false).unwrap();
        assert_eq!(adv.prompt_name, "advanced");
        assert_eq!(adv.source_tier, "base");
        assert!(!adv.segmentation_only);
    }

    #[test]
    fn spanish_source_basic_chain_reverses_direction() {
        let bt = stage_dispatch(STAGE_GENERATE_BASIC_TARGET, true, false).unwrap();
        // Spanish-source: basic_target ← base (NOT ← basic_base)
        assert_eq!(bt.target_tier, "basic_target");
        assert_eq!(bt.source_tier, "base");
        assert_eq!(bt.prompt_name, "basic_target");

        let bb = stage_dispatch(STAGE_GENERATE_BASIC_BASE, true, false).unwrap();
        // Spanish-source: basic_base ← basic_target
        assert_eq!(bb.target_tier, "basic_base");
        assert_eq!(bb.source_tier, "basic_target");
        assert_eq!(bb.prompt_name, "basic_base");
    }

    #[test]
    fn spanish_source_advanced_is_segmentation_only() {
        let adv = stage_dispatch(STAGE_GENERATE_ADVANCED_TARGET, true, false).unwrap();
        assert_eq!(adv.prompt_name, "advanced");
        assert_eq!(adv.source_tier, "base");
        assert!(adv.segmentation_only);
    }

    #[test]
    fn moderate_and_mapping_stages_are_mode_invariant() {
        for sit in [false, true] {
            let m = stage_dispatch(STAGE_GENERATE_MODERATE_TARGET, sit, false).unwrap();
            assert_eq!(m.source_tier, "advanced_target");

            let pm = stage_dispatch(STAGE_GENERATE_PHRASE_MAP, sit, false).unwrap();
            assert_eq!(pm.source_tier, "basic_base");
            assert_eq!(pm.target_tier, "MAPPING:basic_base:basic_target");

            let ipm = stage_dispatch(STAGE_GENERATE_INVERSE_PHRASE_MAP, sit, false).unwrap();
            assert_eq!(ipm.source_tier, "basic_target");
        }
    }

    #[test]
    fn unknown_stage_returns_none() {
        assert!(stage_dispatch("Bogus", false, false).is_none());
        assert!(stage_dispatch("Bogus", true, false).is_none());
    }

    #[test]
    fn english_source_basic_base_passthrough_when_source_is_basic() {
        // en-es + source_is_basic=true: basic_base copies from base verbatim.
        let bb = stage_dispatch(STAGE_GENERATE_BASIC_BASE, false, true).unwrap();
        assert_eq!(bb.target_tier, "basic_base");
        assert_eq!(bb.source_tier, "base");
        assert_eq!(bb.prompt_name, PROMPT_PASSTHROUGH_COPY);
        assert!(bb.copy_from_source_tier);

        // basic_target is unaffected — still goes through the LLM
        // (cross-language; passthrough doesn't apply).
        let bt = stage_dispatch(STAGE_GENERATE_BASIC_TARGET, false, true).unwrap();
        assert_eq!(bt.prompt_name, "basic_target");
        assert!(!bt.copy_from_source_tier);
    }

    #[test]
    fn spanish_source_basic_target_passthrough_when_source_is_basic() {
        // es-es + source_is_basic=true: basic_target copies from base verbatim.
        let bt = stage_dispatch(STAGE_GENERATE_BASIC_TARGET, true, true).unwrap();
        assert_eq!(bt.target_tier, "basic_target");
        assert_eq!(bt.source_tier, "base");
        assert_eq!(bt.prompt_name, PROMPT_PASSTHROUGH_COPY);
        assert!(bt.copy_from_source_tier);

        // basic_base is unaffected — still translated from basic_target.
        let bb = stage_dispatch(STAGE_GENERATE_BASIC_BASE, true, true).unwrap();
        assert_eq!(bb.prompt_name, "basic_base");
        assert!(!bb.copy_from_source_tier);
    }

    #[test]
    fn lang_for_tier_english_source_unchanged() {
        assert_eq!(lang_for_tier("base", "en", "es", false), "en");
        assert_eq!(lang_for_tier("basic_base", "en", "es", false), "en");
        assert_eq!(lang_for_tier("basic_target", "en", "es", false), "es");
        assert_eq!(lang_for_tier("advanced_target", "en", "es", false), "es");
    }

    #[test]
    fn lang_for_tier_spanish_source_basic_base_is_english() {
        // project_languages = (es, es), source_is_target=true.
        // basic_base is the learner-language output → hardcoded "en".
        assert_eq!(lang_for_tier("basic_base", "es", "es", true), "en");
        assert_eq!(lang_for_tier("base", "es", "es", true), "es");
        assert_eq!(lang_for_tier("basic_target", "es", "es", true), "es");
        assert_eq!(lang_for_tier("advanced_target", "es", "es", true), "es");
        assert_eq!(lang_for_tier("moderate_target", "es", "es", true), "es");
    }

    /// Helper: resolve a stage to its `{input}-{output}` prompt directory.
    fn dir_for(stage: &str, base: &str, target: &str, sit: bool) -> String {
        let r = stage_dispatch(stage, sit, false).unwrap();
        let (i, o) = prompt_pair_for_stage(r.source_tier, r.target_tier, base, target, sit);
        format!("{i}-{o}")
    }

    #[test]
    fn prompt_dirs_english_source() {
        let (b, t) = ("en", "es");
        assert_eq!(dir_for(STAGE_GENERATE_ADVANCED_TARGET, b, t, false), "en-es");
        assert_eq!(dir_for(STAGE_GENERATE_MODERATE_TARGET, b, t, false), "es-es");
        assert_eq!(dir_for(STAGE_GENERATE_BASIC_BASE, b, t, false), "en-en");
        assert_eq!(dir_for(STAGE_GENERATE_BASIC_TARGET, b, t, false), "en-es");
        assert_eq!(dir_for(STAGE_GENERATE_PHRASE_MAP, b, t, false), "en-es");
        assert_eq!(dir_for(STAGE_GENERATE_INVERSE_PHRASE_MAP, b, t, false), "es-en");
    }

    #[test]
    fn prompt_dirs_spanish_source() {
        // project_languages = (es, es), source_is_target = true.
        let (b, t) = ("es", "es");
        assert_eq!(dir_for(STAGE_GENERATE_ADVANCED_TARGET, b, t, true), "es-es");
        assert_eq!(dir_for(STAGE_GENERATE_MODERATE_TARGET, b, t, true), "es-es");
        assert_eq!(dir_for(STAGE_GENERATE_BASIC_TARGET, b, t, true), "es-es");
        assert_eq!(dir_for(STAGE_GENERATE_BASIC_BASE, b, t, true), "es-en");
        assert_eq!(dir_for(STAGE_GENERATE_PHRASE_MAP, b, t, true), "en-es");
        assert_eq!(dir_for(STAGE_GENERATE_INVERSE_PHRASE_MAP, b, t, true), "es-en");
    }
}
