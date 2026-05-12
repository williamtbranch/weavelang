// src/services/wlemma_upgrade.rs
//
// Phase 6 — migration tooling.
//
// Walks an in-memory `AppState` and populates wlemma fields on every
// word, segment, tier, and mapping entry; bumps `schema_version` to
// `TARGET_SCHEMA_VERSION` (= 2). Idempotent: a state already at the
// target version is a no-op.
//
// Used in two places:
// - `LoadProject` in `engine.rs` — auto-upgrade legacy `.wvl` files on
//   load. The user is notified via the load-result message.
// - `weavelang_cli upgrade-wvl <path>` — batch-upgrade .wvl files from
//   the command line.
//
// See `documentation/Wlemma_Migration_Plan.md` (Phase 6).

use crate::app::state::AppState;
use crate::domain::stemmer::{self, Stemmer};
use crate::domain::token_stream::Token;
use crate::domain::wlemma::{compute_wlemmas_for_word, BucketRanks};
use crate::simulation::frequency_manager::GlobalBucketRanks;

/// Schema version produced by this upgrade. Bump when the wlemma
/// representation changes again.
pub const TARGET_SCHEMA_VERSION: u32 = 2;

/// Tiers whose lemmas/wlemmas live in the source language and therefore
/// don't need wlemma population (mirrors `lemmatize_tier_segments`'s
/// early-return guard).
const NON_TARGET_TIERS: &[&str] = &["base", "basic_base"];

/// Summary returned by [`upgrade_app_state_with`]. All counts are zero
/// when the state is already at `TARGET_SCHEMA_VERSION`.
#[derive(Debug, Default, Clone, Copy)]
pub struct UpgradeReport {
    pub from_version: u32,
    pub to_version: u32,
    pub already_at_target: bool,
    pub words_updated: usize,
    pub segments_updated: usize,
    pub tiers_updated: usize,
    pub mapping_entries_updated: usize,
}

/// Idempotent in-place upgrade. Pure function — takes the stemmer and a
/// `BucketRanks` impl explicitly so unit tests can drive it without
/// touching the global `FrequencyManager`.
///
/// If `stemmer` is `None`, wlemma fields are *cleared* (so the on-disk
/// representation remains consistent for languages with no Snowball
/// support) and the schema version is still bumped — we've established
/// the post-upgrade invariant that the field exists, just empty.
pub fn upgrade_app_state_with<R: BucketRanks>(
    state: &mut AppState,
    stemmer: Option<&dyn Stemmer>,
    ranks: &R,
) -> UpgradeReport {
    upgrade_app_state_with_force(state, stemmer, ranks, false)
}

/// Like [`upgrade_app_state_with`], but with an explicit `force` flag.
///
/// When `force` is `true`, the schema-version short-circuit is skipped
/// and every word/segment/tier/mapping wlemma is re-computed from
/// scratch. Use this after a wlemma-algorithm tweak (new clitic-strip
/// rule, new diacritic fold, …) to bring already-v2 `.wvl` files in
/// line with the new logic without re-ingesting through spaCy.
///
/// `already_at_target` in the returned report still reflects whether
/// the input was at the target version *before* this call, so callers
/// can distinguish a forced re-compute from a regular upgrade in their
/// log output.
pub fn upgrade_app_state_with_force<R: BucketRanks>(
    state: &mut AppState,
    stemmer: Option<&dyn Stemmer>,
    ranks: &R,
    force: bool,
) -> UpgradeReport {
    let mut report = UpgradeReport {
        from_version: state.schema_version,
        to_version: TARGET_SCHEMA_VERSION,
        already_at_target: state.schema_version >= TARGET_SCHEMA_VERSION,
        ..Default::default()
    };
    if report.already_at_target && !force {
        return report;
    }

    for sent in state.document.iter_mut() {
        for (tier_id, tier) in sent.tiers.iter_mut() {
            let skip_words = NON_TARGET_TIERS.contains(&tier_id.as_str());
            let mut tier_changed = false;

            for seg in tier.segments.iter_mut() {
                if !skip_words {
                    for token in seg.stream.tokens_mut().iter_mut() {
                        if let Token::Word(wd) = token {
                            let new_wlemmas = match stemmer {
                                Some(s) => compute_wlemmas_for_word(
                                    &wd.text, &wd.lemmas, s, ranks,
                                ),
                                None => Vec::new(),
                            };
                            if new_wlemmas != wd.wlemmas {
                                wd.wlemmas = new_wlemmas;
                                report.words_updated += 1;
                                tier_changed = true;
                            }
                        }
                    }
                }

                let aggregated_seg: Vec<String> = seg.stream.tokens()
                    .iter()
                    .filter_map(|t| match t {
                        Token::Word(wd) => Some(wd.wlemmas.clone()),
                        _ => None,
                    })
                    .flatten()
                    .collect();
                if aggregated_seg != seg.wlemmas {
                    seg.wlemmas = aggregated_seg;
                    report.segments_updated += 1;
                    tier_changed = true;
                }
            }

            let mut aggregated_tier: Vec<String> = tier
                .segments
                .iter()
                .flat_map(|s| s.wlemmas.clone())
                .collect();
            aggregated_tier.sort();
            aggregated_tier.dedup();
            if aggregated_tier != tier.wlemmas {
                tier.wlemmas = aggregated_tier;
                tier_changed = true;
            }
            if tier_changed {
                report.tiers_updated += 1;
            }
        }

        // Mapping entries — target_text is target-language for forward
        // mappings and source-language for inverse mappings, but in both
        // cases `target_lemmas` already holds target-language lemmas
        // (see `lemmatize_mapping_targets`). For inverse mappings the
        // surface tie-breaker would need access to the basic_target
        // tier; here we conservatively pass `target_text` as the surface
        // — for forward mappings this is exactly right, and for inverse
        // mappings it falls back to the lemma-stem path (since the
        // English surface won't have a Spanish stem in the bucket).
        for mapping in sent.mappings.iter_mut() {
            for entry in mapping.entries.iter_mut() {
                let new_wlemmas = match stemmer {
                    Some(s) => compute_wlemmas_for_word(
                        &entry.target_text, &entry.target_lemmas, s, ranks,
                    ),
                    None => Vec::new(),
                };
                if new_wlemmas != entry.target_wlemmas {
                    entry.target_wlemmas = new_wlemmas;
                    report.mapping_entries_updated += 1;
                }
            }
        }
    }

    state.schema_version = TARGET_SCHEMA_VERSION;
    report
}

/// Production wrapper: builds a stemmer from `target_lang` and uses the
/// global `FrequencyManager` bucket ranks. The frequency list must
/// already be loaded; if it isn't, ranks come back as `None` and
/// wlemmas fall back to the lemma-stem path.
pub fn upgrade_app_state(state: &mut AppState, target_lang: &str) -> UpgradeReport {
    upgrade_app_state_force(state, target_lang, false)
}

/// Production wrapper with an explicit `force` flag — re-computes
/// wlemmas even when the state is already at the target schema
/// version. See [`upgrade_app_state_with_force`].
pub fn upgrade_app_state_force(
    state: &mut AppState,
    target_lang: &str,
    force: bool,
) -> UpgradeReport {
    let stemmer_box = stemmer::for_language(target_lang);
    let ranks = GlobalBucketRanks;
    upgrade_app_state_with_force(state, stemmer_box.as_deref(), &ranks, force)
}

#[cfg(test)]
mod tests {
    //! TT7 — golden/idempotency. We test against a hand-built `AppState`
    //! with a single sentence, single tier, single word, and a single
    //! mapping entry, using a fake `BucketRanks`.

    use super::*;
    use crate::domain::mapping::{MappingEntry, TierMapping};
    use crate::domain::primitives::{WordData, WordId};
    use crate::domain::segment::Segment;
    use crate::domain::sentence::Sentence;
    use crate::domain::stemmer::SpanishSnowball;
    use crate::domain::tier::Tier;
    use crate::domain::token_stream::TokenStream;
    use std::collections::HashMap;

    struct FakeRanks(HashMap<String, u32>);
    impl BucketRanks for FakeRanks {
        fn rank_of(&self, w: &str) -> Option<u32> {
            self.0.get(w).copied()
        }
    }

    /// Build a minimal legacy (`schema_version = 0`) `AppState` containing
    /// one sentence with a `basic_target` tier holding `"niños"` and one
    /// forward mapping entry.
    fn make_legacy_state() -> AppState {
        let mut wd = WordData::new(WordId(1), "niños".into(), vec!["niño".into()]);
        wd.wlemmas.clear(); // simulate pre-wlemma payload
        let stream = TokenStream::from_tokens(vec![
            Token::Background("".into()),
            Token::Word(wd),
            Token::Background("".into()),
        ]);
        let seg = Segment::from_stream("S1".into(), stream, vec!["niño".into()]);
        let mut tier = Tier::new("basic_target".into());
        tier.segments.push(seg);
        tier.lemmas = vec!["niño".into()];

        let mut sent = Sentence::new("Sent1".into());
        sent.tiers.insert("basic_target".into(), tier);

        let entry = MappingEntry::new(WordId(1), "niños".into(), vec!["niño".into()]);
        let mapping = TierMapping {
            from_tier_id: "basic_base".into(),
            to_tier_id: "basic_target".into(),
            entries: vec![entry],
        };
        sent.mappings.push(mapping);

        let mut state = AppState::default();
        state.schema_version = 0;
        state.document = vec![sent];
        state
    }

    fn ranks_with_canonical() -> FakeRanks {
        let s = SpanishSnowball::new();
        let entries = [
            ("niño", 154u32),
            ("niños", 52_370),
        ];
        let mut m: HashMap<String, u32> = HashMap::new();
        for (lemma, rank) in entries {
            let key = s.stem(&lemma.to_lowercase());
            m.entry(key)
                .and_modify(|r| {
                    if rank < *r {
                        *r = rank;
                    }
                })
                .or_insert(rank);
        }
        FakeRanks(m)
    }

    #[test]
    fn upgrade_populates_wlemmas_and_bumps_version() {
        let mut state = make_legacy_state();
        let stemmer = SpanishSnowball::new();
        let ranks = ranks_with_canonical();

        let report = upgrade_app_state_with(&mut state, Some(&stemmer), &ranks);

        assert!(!report.already_at_target);
        assert_eq!(report.from_version, 0);
        assert_eq!(report.to_version, TARGET_SCHEMA_VERSION);
        assert_eq!(state.schema_version, TARGET_SCHEMA_VERSION);
        assert!(report.words_updated >= 1);
        assert!(report.segments_updated >= 1);
        assert!(report.tiers_updated >= 1);
        assert!(report.mapping_entries_updated >= 1);

        // The word's wlemma now matches the niño stem (rank 154 ≪ 52_370).
        let tier = state.document[0].tiers.get("basic_target").unwrap();
        let seg = &tier.segments[0];
        let word_wlemmas: Vec<String> = seg
            .stream
            .tokens()
            .iter()
            .filter_map(|t| match t {
                Token::Word(wd) => Some(wd.wlemmas.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(word_wlemmas.len(), 1);
        let nino_stem = stemmer.stem("niño");
        assert_eq!(word_wlemmas[0], nino_stem);
        assert_eq!(seg.wlemmas, vec![nino_stem.clone()]);
        assert_eq!(tier.wlemmas, vec![nino_stem.clone()]);

        // Mapping entry was populated too.
        let entry = &state.document[0].mappings[0].entries[0];
        assert_eq!(entry.target_wlemmas, vec![nino_stem]);
    }

    #[test]
    fn upgrade_is_idempotent() {
        let mut state = make_legacy_state();
        let stemmer = SpanishSnowball::new();
        let ranks = ranks_with_canonical();

        let r1 = upgrade_app_state_with(&mut state, Some(&stemmer), &ranks);
        assert!(!r1.already_at_target);
        let snapshot = serde_json::to_value(&state).unwrap();

        let r2 = upgrade_app_state_with(&mut state, Some(&stemmer), &ranks);
        assert!(r2.already_at_target);
        assert_eq!(r2.words_updated, 0);
        assert_eq!(r2.segments_updated, 0);
        assert_eq!(r2.tiers_updated, 0);
        assert_eq!(r2.mapping_entries_updated, 0);

        // State is byte-identical after the second pass (modulo HashMap
        // ordering, which serde_json::Value handles).
        let snapshot2 = serde_json::to_value(&state).unwrap();
        assert_eq!(snapshot, snapshot2);
    }

    #[test]
    fn upgrade_already_at_target_is_noop() {
        let mut state = make_legacy_state();
        state.schema_version = TARGET_SCHEMA_VERSION;
        let stemmer = SpanishSnowball::new();
        let ranks = ranks_with_canonical();

        let report = upgrade_app_state_with(&mut state, Some(&stemmer), &ranks);
        assert!(report.already_at_target);
        assert_eq!(report.words_updated, 0);
        assert_eq!(report.mapping_entries_updated, 0);
        // Word wlemmas were left empty — nothing was populated.
        let tier = state.document[0].tiers.get("basic_target").unwrap();
        let seg = &tier.segments[0];
        for token in seg.stream.tokens() {
            if let Token::Word(wd) = token {
                assert!(wd.wlemmas.is_empty());
            }
        }
    }

    #[test]
    fn upgrade_without_stemmer_clears_and_bumps() {
        let mut state = make_legacy_state();
        // Pre-populate a wlemma to confirm it gets cleared.
        if let Some(tier) = state.document[0].tiers.get_mut("basic_target") {
            for token in tier.segments[0].stream.tokens_mut().iter_mut() {
                if let Token::Word(wd) = token {
                    wd.wlemmas = vec!["stale".into()];
                }
            }
        }
        let ranks = FakeRanks(HashMap::new());
        let report = upgrade_app_state_with(&mut state, None, &ranks);
        assert_eq!(state.schema_version, TARGET_SCHEMA_VERSION);
        assert!(report.words_updated >= 1);
        let tier = state.document[0].tiers.get("basic_target").unwrap();
        for token in tier.segments[0].stream.tokens() {
            if let Token::Word(wd) = token {
                assert!(wd.wlemmas.is_empty());
            }
        }
    }
}
