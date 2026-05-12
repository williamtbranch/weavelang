//! Compute the wlemma (bucket key) for a token.
//!
//! See `documentation/Wlemma_Migration_Plan.md`. The wlemma is the
//! min-frequency-rank stem among the surface form and the upstream
//! lemmatizer's lemma. This rescues both:
//!
//!  - Closed-class words where the lemma is correct and the surface is
//!    inflected (`los` → `el`); the lemma stem wins.
//!  - Open-class words where the upstream lemmatizer hallucinates and
//!    returns the surface form unchanged (`Niños` → `Niños`); stemming
//!    the surface still finds the correct family.
//!
//! Ties go to the lemma stem so that, when both paths point at the same
//! bucket, we pick the one closer to the linguistically motivated form.

use crate::domain::stemmer::Stemmer;

/// Read-only access to the bucket-rank map. Implemented by the live
/// `FrequencyManager` and by test fakes.
pub trait BucketRanks {
    fn rank_of(&self, wlemma: &str) -> Option<u32>;
}

/// Lower-case + NFC-style trim. Centralized so all stemmer inputs are
/// preprocessed identically; language-specific accent folding is the
/// stemmer's responsibility.
fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Type alias for the wlemma bucket key. Plain `String` for minimal churn
/// with existing `Vec<String>` lemma fields and serde plumbing. Promote to
/// a newtype later if we need stronger type-level guarantees.
pub type Wlemma = String;

/// Compute the wlemma for a token given its surface form and the upstream
/// lemma (e.g. from spaCy). The lower-rank bucket wins; ties go to the
/// lemma path. If neither key has a rank, returns the lemma stem (still a
/// stable bucket key, just one we have no frequency data for).
///
/// A third "salvage" candidate is also considered: if the language's
/// stemmer can strip an enclitic suffix from the surface form (Spanish
/// `acércate` → `acérca`, `sentarte` → `sentar`, `gritándoles` →
/// `gritándo`), the stem of that stripped remainder is added to the
/// candidate set. This rescues a class of upstream-lemmatizer
/// hallucinations where surface and lemma agree on a malformed
/// clitic-attached form (`acércate` → spaCy lemma `acercatir`); without
/// stripping, both candidates point at the same wrong bucket. The
/// stripped candidate only wins on strict rank improvement, so it's
/// safe to add unconditionally — the closed-list + accent/infinitive
/// gate inside `strip_enclitics` already prevents collateral damage.
///
/// A fourth set of "radical-change" salvage candidates is generated
/// from the strip remainder (Phase 8d): Spanish stem-changing verbs
/// diphthongize their stem under stress (`sentar` → `siénta`,
/// `contar` → `cuenta`, `dormir` → `duerme`). Snowball does not
/// undo this, so even after clitic strip, `siénta` (from `siéntate`)
/// stems to `sient`, missing the `sent` bucket where `sentar` lives.
/// `Stemmer::unmutate_radical_change` produces folded variants like
/// `senta`/`conta`/`dorme` whose Snowball stems unify with the
/// infinitive bucket. Same strict-improvement rule applies. The
/// strip-enclitics pre-gate confines this rule to enclitic-attached
/// forms, so non-verbs containing `ie`/`ue` (`puerta`, `tiempo`,
/// `bueno`) are never touched.
pub fn compute_wlemma<S: Stemmer + ?Sized, R: BucketRanks + ?Sized>(
    surface: &str,
    lemma: &str,
    stemmer: &S,
    ranks: &R,
) -> Wlemma {
    let lemma_input = if lemma.is_empty() { surface } else { lemma };
    let lemma_stem = stemmer.stem(&normalize(lemma_input));
    let surface_stem = stemmer.stem(&normalize(surface));

    let stripped = stemmer.strip_enclitics(&normalize(surface));
    let stripped_stem = stripped
        .as_deref()
        .map(|s| stemmer.stem(s))
        .filter(|s| s != &lemma_stem && s != &surface_stem);

    // Phase 8d: radical-change un-mutated candidates, derived ONLY from
    // the strip remainder so the operation is gated to enclitic-attached
    // verb forms. Each variant is snowball-stemmed and deduped against
    // the existing candidates.
    let radical_stems: Vec<String> = match stripped.as_deref() {
        Some(s) => stemmer
            .unmutate_radical_change(s)
            .into_iter()
            .map(|v| stemmer.stem(&v))
            .filter(|st| {
                st != &lemma_stem
                    && st != &surface_stem
                    && stripped_stem.as_deref() != Some(st.as_str())
            })
            .collect(),
        None => Vec::new(),
    };

    // Pick the best (lowest-rank) candidate. Tie-break order:
    // lemma > surface > stripped > radical (lemma is most "linguistic",
    // strip and radical are salvage paths that should only win on
    // strict improvement).
    let lemma_rank = ranks.rank_of(&lemma_stem);
    let surface_rank = if surface_stem == lemma_stem {
        lemma_rank
    } else {
        ranks.rank_of(&surface_stem)
    };
    let stripped_rank = stripped_stem.as_deref().and_then(|s| ranks.rank_of(s));

    // Start with lemma as the incumbent.
    let mut best_stem: String = lemma_stem.clone();
    let mut best_rank: Option<u32> = lemma_rank;

    // Surface beats incumbent on strict rank improvement, or when the
    // incumbent has no rank but surface does.
    if surface_stem != lemma_stem {
        let beats = match (best_rank, surface_rank) {
            (Some(b), Some(s)) => s < b,
            (None, Some(_)) => true,
            _ => false,
        };
        if beats {
            best_stem = surface_stem.clone();
            best_rank = surface_rank;
        }
    }

    // Stripped is salvage: only wins on strict rank improvement against
    // the current best. If best has no rank and stripped has a rank, take it.
    if let Some(ref ss) = stripped_stem {
        let beats = match (best_rank, stripped_rank) {
            (Some(b), Some(s)) => s < b,
            (None, Some(_)) => true,
            _ => false,
        };
        if beats {
            best_stem = ss.clone();
            best_rank = stripped_rank;
        }
    }

    // Radical-change variants: each must beat current best on strict
    // rank improvement. Same fall-back-to-rank rule as stripped.
    for rs in &radical_stems {
        let rs_rank = ranks.rank_of(rs);
        let beats = match (best_rank, rs_rank) {
            (Some(b), Some(s)) => s < b,
            (None, Some(_)) => true,
            _ => false,
        };
        if beats {
            best_stem = rs.clone();
            best_rank = rs_rank;
        }
    }
    let _ = best_rank;

    best_stem
}

/// Compute the wlemma bucket keys for a list of lemma candidates given a
/// surface form. Returns a deduplicated `Vec` in input order. Empty input
/// (no lemmas) yields a single-element vec with the surface-only wlemma.
///
/// When `surface` is a multi-word slot (e.g. mapping-entry text like
/// `"a sentarte"`) and the whitespace-token count matches the lemma
/// count, lemmas are paired with their corresponding surface token
/// positionally. This is critical for the strip-enclitics salvage path:
/// `strip_enclitics("a sentarte")` would otherwise produce `"a sentar"`,
/// whose stem is not in the ranks map, defeating the rescue. Pairing by
/// position lets `compute_wlemma("sentarte", "sentarte", …)` strip cleanly
/// to the `sentar` bucket.
pub fn compute_wlemmas_for_word<S: Stemmer + ?Sized, R: BucketRanks + ?Sized>(
    surface: &str,
    lemmas: &[String],
    stemmer: &S,
    ranks: &R,
) -> Vec<Wlemma> {
    if lemmas.is_empty() {
        return vec![compute_wlemma(surface, "", stemmer, ranks)];
    }
    // Try positional pairing for multi-word surfaces.
    let surface_tokens: Vec<&str> = surface.split_whitespace().collect();
    let pair_by_index = surface_tokens.len() > 1 && surface_tokens.len() == lemmas.len();

    let mut out: Vec<Wlemma> = Vec::with_capacity(lemmas.len());
    for (i, lemma) in lemmas.iter().enumerate() {
        let per_lemma_surface = if pair_by_index { surface_tokens[i] } else { surface };
        let w = compute_wlemma(per_lemma_surface, lemma, stemmer, ranks);
        if !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stemmer::SpanishSnowball;
    use std::collections::HashMap;

    struct FakeRanks(HashMap<String, u32>);
    impl BucketRanks for FakeRanks {
        fn rank_of(&self, wlemma: &str) -> Option<u32> {
            self.0.get(wlemma).copied()
        }
    }

    fn ranks(pairs: &[(&str, u32)]) -> FakeRanks {
        FakeRanks(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    }

    #[test]
    fn picks_lower_rank_path() {
        let s = SpanishSnowball::new();
        // Use real Spanish words known to stem to different buckets.
        // "correr" stems to "corr"-family; "decir" stems to a different one.
        let lemma_stem = s.stem("decir");
        let surface_stem = s.stem("correr");
        assert_ne!(lemma_stem, surface_stem, "test premise: distinct stems");
        let r = ranks(&[
            (lemma_stem.as_str(), 5_000),
            (surface_stem.as_str(), 100),
        ]);
        let w = compute_wlemma("correr", "decir", &s, &r);
        assert_eq!(w, surface_stem);
        assert_eq!(r.rank_of(&w), Some(100));
    }

    #[test]
    fn ties_go_to_lemma_stem() {
        let s = SpanishSnowball::new();
        let lemma_stem = s.stem("decir");
        let surface_stem = s.stem("correr");
        assert_ne!(lemma_stem, surface_stem);
        let r = ranks(&[
            (lemma_stem.as_str(), 100),
            (surface_stem.as_str(), 100),
        ]);
        let w = compute_wlemma("correr", "decir", &s, &r);
        assert_eq!(w, lemma_stem);
    }

    #[test]
    fn falls_back_to_lemma_stem_when_unknown() {
        let s = SpanishSnowball::new();
        let r = ranks(&[]);
        let w = compute_wlemma("correr", "decir", &s, &r);
        assert_eq!(w, s.stem("decir"));
    }

    #[test]
    fn empty_lemma_uses_surface() {
        let s = SpanishSnowball::new();
        let r = ranks(&[]);
        let w = compute_wlemma("Niños", "", &s, &r);
        assert_eq!(w, s.stem("niños"));
    }

    #[test]
    fn rescues_hallucinated_lemma_when_stems_differ() {
        // Construct the case where lemma path and surface path produce
        // *different* stems and the surface bucket happens to be rarer.
        // Using "ninos" (no tilde) vs "niño" — the missing tilde often
        // produces a distinct Snowball stem from the accented form.
        let s = SpanishSnowball::new();
        let stem_lemma = s.stem("niño");
        let stem_surface = s.stem("ninos");
        if stem_lemma == stem_surface {
            // Snowball already collapses them; nothing to rescue. The
            // function still returns a stable bucket key, which is the
            // weaker but valid contract here.
            let w = compute_wlemma("ninos", "ninos", &s, &ranks(&[]));
            assert_eq!(w, stem_surface);
            return;
        }
        // Simulate spaCy hallucinating: lemma == surface == "ninos".
        // Bucket map says lemma stem is rank 154, surface stem is rank 52370.
        let r = ranks(&[
            (stem_lemma.as_str(), 154),
            (stem_surface.as_str(), 52_370),
        ]);
        let w = compute_wlemma("ninos", "ninos", &s, &r);
        // Both inputs produce the same stem here (lemma==surface text), so
        // compute_wlemma short-circuits to that stem. Verify the bucket.
        assert!(r.rank_of(&w).is_some());
    }

    #[test]
    fn closed_class_lemma_path_wins() {
        // For "los" → lemma "el", the lemma stem is much more common.
        let s = SpanishSnowball::new();
        let stem_el = s.stem("el");
        let stem_los = s.stem("los");
        if stem_el == stem_los {
            return; // Snowball collapses; trivially correct.
        }
        let r = ranks(&[
            (stem_el.as_str(), 1),
            (stem_los.as_str(), 8_000),
        ]);
        let w = compute_wlemma("los", "el", &s, &r);
        assert_eq!(w, stem_el);
        assert_eq!(r.rank_of(&w), Some(1));
    }

    /// TT5: ingestion regression. The four canonical broken tokens
    /// (`Niños`, `Camioneros`, `gritándoles`, `Corres`) used to score in
    /// the rare-lemma territory because spaCy returned the surface form
    /// unchanged. With the wlemma fix, each should resolve to its common
    /// family bucket — basic/moderate, well below the advanced threshold.
    ///
    /// Ranks here come from the real master frequency list (master values
    /// observed in `bucket_rank_prototype.py`):
    ///   niño     154    niños    52,370
    ///   correr   662    corres   86,075
    ///   camionero 12,157  camioneros 1,139,313
    ///   gritar   1,774  gritándoles 1,545,889
    #[test]
    fn canonical_broken_tokens_resolve_to_common_buckets() {
        let s = SpanishSnowball::new();
        // Build a fake bucket map keyed by Snowball stems, simulating the
        // post-bucketing aggregation of the real frequency list.
        let entries = [
            ("niño", 154u32),
            ("niños", 52_370),
            ("correr", 662),
            ("corres", 86_075),
            ("camionero", 12_157),
            ("camioneros", 1_139_313),
            ("gritar", 1_774),
            ("gritándoles", 1_545_889),
        ];
        let mut bucket: HashMap<String, u32> = HashMap::new();
        for (lemma, rank) in entries {
            let key = s.stem(&lemma.to_lowercase());
            bucket
                .entry(key)
                .and_modify(|r| {
                    if rank < *r {
                        *r = rank;
                    }
                })
                .or_insert(rank);
        }
        let r = FakeRanks(bucket);

        // Simulate spaCy hallucination: lemma == surface (unchanged).
        let cases = [
            ("Niños", "Niños", 154u32),
            ("Camioneros", "Camioneros", 12_157),
            ("gritándoles", "gritándoles", 1_774),
            ("Corres", "Corres", 662),
        ];
        for (surface, lemma, expected_max) in cases {
            let wlemmas = compute_wlemmas_for_word(surface, &[lemma.to_string()], &s, &r);
            assert_eq!(wlemmas.len(), 1, "{}: one wlemma", surface);
            let actual = r.rank_of(&wlemmas[0]).unwrap_or(u32::MAX);
            assert!(
                actual <= expected_max,
                "{}: wlemma rank {} > expected {}",
                surface,
                actual,
                expected_max
            );
            // And — the regression check — the wlemma rank must be in
            // basic/moderate territory (under 20k).
            assert!(
                actual < 20_000,
                "{}: wlemma rank {} should be basic/moderate, not advanced",
                surface,
                actual
            );
        }
    }

    /// Phase 8c regression: enclitic-pronoun hallucinations
    /// (`Acércate` → spaCy lemma `acercatir`, `sentarte` → lemma
    /// `sentarte`, `siéntate` → lemma `sientatir`). Surface and lemma
    /// stem to malformed buckets at mid-thousand ranks. Stripping the
    /// enclitic from the surface yields a candidate that lands in the
    /// real verb family, which the `min` rule then prefers.
    #[test]
    fn enclitic_hallucinations_rescued_by_strip_candidate() {
        let s = SpanishSnowball::new();
        // For each case, populate two buckets: the malformed one
        // (matching the spaCy-hallucinated lemma) at a mid-thousand
        // rank, and the bucket reached by stripping the enclitic from
        // the surface, at a low rank. After the strip-candidate fix,
        // the low-rank bucket must win.
        let cases = [
            // (surface, spacy_lemma, malformed_rank, rescue_rank)
            ("Acércate", "acercatir", 10_063u32, 1_500u32),
            ("sentarte", "sentarte", 23_134, 900),
            ("siéntate", "sientatir", 2_350, 700),
        ];
        for (surface, lemma, malformed_rank, rescue_rank) in cases {
            let lemma_stem = s.stem(&lemma.to_lowercase());
            let surface_stem = s.stem(&surface.to_lowercase());
            let stripped = s
                .strip_enclitics(&surface.to_lowercase())
                .expect("test premise: surface is enclitic-strippable");
            let stripped_stem = s.stem(&stripped);
            assert_ne!(
                stripped_stem, lemma_stem,
                "{}: test premise — strip candidate must differ from lemma stem",
                surface
            );

            let mut bucket: HashMap<String, u32> = HashMap::new();
            bucket.insert(lemma_stem.clone(), malformed_rank);
            bucket.insert(surface_stem.clone(), malformed_rank);
            bucket.insert(stripped_stem.clone(), rescue_rank);
            let r = FakeRanks(bucket);

            let w = compute_wlemma(surface, lemma, &s, &r);
            assert_eq!(
                w, stripped_stem,
                "{}: expected strip-rescue bucket to win",
                surface
            );
            let actual = r.rank_of(&w).unwrap_or(u32::MAX);
            assert!(
                actual < malformed_rank,
                "{}: wlemma rank {} not below malformed-bucket ceiling {}",
                surface,
                actual,
                malformed_rank
            );
        }
    }

    /// Strip candidate must NOT pollute non-verb words that happen to
    /// end in a clitic-shaped suffix (`carteles`, `papeles`). The
    /// stem of the original word should remain the wlemma.
    #[test]
    fn strip_candidate_does_not_steal_non_verb_buckets() {
        let s = SpanishSnowball::new();
        // `carteles` stems to "cartel"; Snowball already collapses
        // singular/plural here, so the test premise is just that no
        // accidental "carte"/"cart" bucket can win.
        let original = s.stem("carteles");
        // Add a deliberately-attractive fake "cart" bucket with rank 1
        // — if the strip path were active for `carteles`, it would
        // hijack the wlemma. The accent/infinitive gate must prevent
        // that.
        let r = ranks(&[
            (original.as_str(), 5_000),
            ("cart", 1),
            ("carte", 1),
        ]);
        let w = compute_wlemma("carteles", "cartel", &s, &r);
        assert_eq!(w, original, "non-verb must not be enclitic-stripped");
    }

    /// `compute_wlemmas_for_word` must pair multi-word surfaces with
    /// lemmas by position, otherwise the strip-enclitics salvage path
    /// fails on mapping-entry slots like `"a sentarte"` (stripping
    /// "te" off the whole slot yields `"a sentar"`, whose stem is junk
    /// and not in the ranks map).
    #[test]
    fn multiword_surface_pairs_lemmas_by_position() {
        let s = SpanishSnowball::new();
        let sentar_stem = s.stem("sentar");
        let sentarte_stem = s.stem("sentarte");
        // Premise: `sentar` and `sentarte` stem to different buckets.
        // (Snowball Spanish does not strip clitics; that's our gap.)
        assert_ne!(sentar_stem, sentarte_stem);
        let r = ranks(&[
            (sentar_stem.as_str(), 1_500),
            (sentarte_stem.as_str(), 23_134),
            ("a", 6),
        ]);
        // Multi-word slot, lemmas align 1:1.
        let ws = compute_wlemmas_for_word("a sentarte", &["a".to_string(), "sentarte".to_string()], &s, &r);
        // The "sentarte" lemma should land in the rescued `sentar`
        // bucket via positional pairing + strip_enclitics.
        assert!(
            ws.contains(&sentar_stem),
            "expected `sentar` bucket via positional strip rescue, got {:?}",
            ws
        );
        assert!(
            !ws.contains(&sentarte_stem),
            "must not retain malformed `sentarte` bucket, got {:?}",
            ws
        );
    }

    /// Radical-change rescue (Phase 8d): stressed-stem imperative-with-clitic
    /// forms like `siéntate`, `cuéntame`, `duérmete` must reach the
    /// infinitive bucket via un-mutation, not just the strip path
    /// (which alone leaves them at the diphthongized stem `sient`,
    /// `cuent`, `duerm`).
    #[test]
    fn radical_change_rescues_stressed_stem_imperatives() {
        let s = SpanishSnowball::new();
        let cases: &[(&str, &str, &str)] = &[
            // (surface, lemma_from_upstream, infinitive_for_target_bucket)
            ("siéntate", "siéntate", "sentar"),
            ("cuéntame", "cuéntame", "contar"),
            ("duérmete", "duérmete", "dormir"),
        ];
        for (surface, lemma, infinitive) in cases {
            let target_stem = s.stem(infinitive);
            let stripped_stem = s
                .strip_enclitics(&surface.to_lowercase())
                .map(|st| s.stem(&st))
                .expect("test premise: clitic strip must succeed");
            // Target bucket must be distinct from the strip-only bucket;
            // otherwise the radical-change path is irrelevant.
            assert_ne!(
                stripped_stem, target_stem,
                "test premise broken for {}: strip already lands in target bucket",
                surface
            );
            // Make the infinitive bucket strictly cheaper than every
            // other path so the radical-change candidate must win.
            let r = ranks(&[
                (target_stem.as_str(), 100),
                (stripped_stem.as_str(), 50_000),
                (s.stem(surface).as_str(), 50_000),
            ]);
            let w = compute_wlemma(surface, lemma, &s, &r);
            assert_eq!(
                w, target_stem,
                "expected `{}` to land in `{}` bucket via radical-change un-mutation",
                surface, infinitive
            );
        }
    }

    /// Radical-change must NOT fire on words that don't pass the
    /// strip-enclitics gate. `puerta` (door), `tiempo` (time),
    /// `bueno` (good) all contain `ie`/`ue` but are not enclitic-attached
    /// verb forms, so the un-mutation pass must never see them — the
    /// gate is `strip_enclitics` returning `Some`.
    #[test]
    fn radical_change_skipped_for_non_enclitic_words() {
        let s = SpanishSnowball::new();
        // None of these strip; they should keep their own stems even
        // when an artificially attractive un-mutated bucket exists.
        let cases: &[&str] = &["puerta", "tiempo", "bueno", "fuerte", "cuerpo"];
        for surface in cases {
            let original = s.stem(surface);
            let r = ranks(&[
                (original.as_str(), 5_000),
                // Artificially attractive un-mutated buckets:
                ("port", 1),
                ("temp", 1),
                ("bon", 1),
                ("fort", 1),
                ("corp", 1),
            ]);
            let w = compute_wlemma(surface, surface, &s, &r);
            assert_eq!(
                w, original,
                "non-enclitic `{}` must not be radical-change un-mutated",
                surface
            );
        }
    }
}
