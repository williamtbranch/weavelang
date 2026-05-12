// tests/wlemma_spot_check.rs
//
// T8.3 — Spot-check the previously-broken Spanish surface forms after the
// wlemma migration. Without bucketing, spaCy returned these surface forms
// as their own lemmas, and the master frequency list also stored them as
// rare entries (rank > 50,000 in some cases), so any sentence containing
// them was forced into "advanced" tier. With wlemma bucketing, every form
// of a word family collapses to the family's most common rank.
//
// This test loads the real master frequency list (so it touches the
// global FrequencyManager singleton) and is `#[ignore]`'d to keep the
// default `cargo test` run fast and hermetic.
//
// Run explicitly:
//   cargo test --test wlemma_spot_check -- --ignored
//   cargo test -- --include-ignored

use std::path::PathBuf;

use weavelang_rust_gui::simulation::frequency_manager;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ensure_loaded() {
    let path = workspace_root().join("assets/frequency_lists/es_master_frequency_list.txt");
    frequency_manager::load_master_frequency_list(&path)
        .expect("master frequency list must load for spot-check");
}

/// The four surface forms called out in the migration plan, with the
/// intuitive "common-bucket" lemma the wlemma machinery should collapse
/// them onto and a generous upper bound on the resulting rank.
///
/// Bound = max acceptable rank for the bucket. Top-1000 territory is
/// the goal for `niño` and `correr`; `gritar` and `camionero` are less
/// common but still must be far below the surface-form penalty (>50k).
const BROKEN_SURFACES: &[(&str, &str, u32)] = &[
    ("Niños", "niño", 1_000),
    ("Corres", "correr", 1_500),
    ("gritándoles", "gritar", 5_000),
    ("Camioneros", "camionero", 20_000),
];

/// The advanced-tier penalty floor that the migration was designed to
/// avoid: with the old lemma-keyed lookup, these surfaces all scored
/// above this and got pushed into advanced.
const ADVANCED_PENALTY_FLOOR: u32 = 50_000;

#[test]
#[ignore]
fn t8_3_broken_surfaces_resolve_to_common_buckets() {
    ensure_loaded();

    for (surface, expected_root, max_rank) in BROKEN_SURFACES {
        // The bucket key for the surface and for the canonical lemma must
        // be identical — that is the whole point of wlemma bucketing.
        let surface_insp = frequency_manager::inspect_bucket(surface).unwrap_or_else(|| {
            panic!("surface '{}' has no bucket in master freq list", surface);
        });
        let root_insp = frequency_manager::inspect_bucket(expected_root).unwrap_or_else(|| {
            panic!("root lemma '{}' has no bucket in master freq list", expected_root);
        });

        assert_eq!(
            surface_insp.wlemma, root_insp.wlemma,
            "surface '{}' and root '{}' must share a wlemma bucket (got {:?} vs {:?})",
            surface, expected_root, surface_insp.wlemma, root_insp.wlemma,
        );

        // The shared bucket rank must be the family's minimum, well below
        // the advanced-tier penalty floor.
        assert!(
            surface_insp.rank < ADVANCED_PENALTY_FLOOR,
            "surface '{}' bucket rank {} still exceeds advanced-tier floor {}",
            surface, surface_insp.rank, ADVANCED_PENALTY_FLOOR,
        );
        assert!(
            surface_insp.rank <= *max_rank,
            "surface '{}' bucket rank {} exceeds expected ceiling {} (bucket key '{}', members={})",
            surface, surface_insp.rank, max_rank, surface_insp.wlemma, surface_insp.members.len(),
        );
    }
}

#[test]
#[ignore]
fn t8_3_rank_of_lemma_string_matches_inspect() {
    // Sanity check that the consumer-facing accessor used everywhere in the
    // pipeline (`rank_of_lemma_string`) returns the same bucket rank that
    // `inspect_bucket` reports for the same surface form.
    ensure_loaded();

    for (surface, _, _) in BROKEN_SURFACES {
        let via_lookup = frequency_manager::rank_of_lemma_string(surface)
            .unwrap_or_else(|| panic!("rank_of_lemma_string returned None for '{}'", surface));
        let via_inspect = frequency_manager::inspect_bucket(surface)
            .unwrap_or_else(|| panic!("inspect_bucket returned None for '{}'", surface))
            .rank;
        assert_eq!(
            via_lookup, via_inspect,
            "rank_of_lemma_string and inspect_bucket disagree on '{}' ({} vs {})",
            surface, via_lookup, via_inspect,
        );
    }
}
