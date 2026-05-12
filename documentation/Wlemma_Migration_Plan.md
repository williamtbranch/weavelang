# Wlemma Migration Plan

## Motivation

The current frequency-rank lookup is keyed on spaCy lemmas, which has two
failure modes that conspire to break tier discrimination (especially basic vs
moderate AVD):

1. spaCy regularly fails to lemmatize and returns the surface form
   (`Niños` → `Niños`, `Camioneros` → `Camioneros`, `Corres` → `Corres`,
   `gritándoles` → `gritándoles`).
2. Because the master frequency list was *also* built with spaCy, the same
   surface forms appear as their own entries at very high (rare) ranks
   (`ninos` rank 52,370 next to `nino` at 154; `corres` 86,075 next to
   `correr` 662; `camioneros` 1,139,313 next to `camionero` 12,157).

The two bugs together mean that a sentence containing a perfectly common
inflection of a top-200 word can score at rank > 50,000 — which forces it
into "advanced" tier and blocks it from basic/moderate weaves.

We do not actually need accurate lemmas. We need a stable equivalence class
("bucket") such that all forms of a word family share one rank, and that
rank is the *minimum* (most common) rank among the family's members.

## The Wlemma

A **wlemma** is the canonical bucket key for a token: a stemmed string,
produced by a language-specific stemmer. It is computed once, at spaCy
ingestion time, when both the surface form and the spaCy lemma are still
in scope.

### Language neutrality (design rule)

Weavelang is designed to be language-neutral. The wlemma machinery must
follow that principle so adding a language is a configuration change, not
a refactor. Concretely:

- The stemmer is accessed through a trait, e.g.:
  ```
  trait Stemmer: Send + Sync {
      fn stem(&self, word: &str) -> String;
  }
  ```
  with concrete impls per language (`SpanishSnowball`, `EnglishSnowball`,
  …) registered in a small factory keyed by language code.
- `compute_wlemma` and `bucket_rank` construction take the active stemmer
  as a parameter (or pull it from a `LanguageContext`), never hard-code
  Spanish.
- `normalize_spanish_lemma` should also be reviewed — its diacritic-strip
  rule is fine for most European languages but the function name and the
  implicit `a-z` alphabet inside its regex are Spanish-shaped. Consider
  generalizing to `normalize_word_for(lang)` driven by the same language
  registry.
- The frequency list path, the stemmer, and (eventually) the lemmatizer
  should all be selected by the same `lang_code` so adding a new language
  is one registration, not a search-and-replace tour.

```
wlemma(surface, spacy_lemma) =
    s = stem(normalize(surface))
    l = stem(normalize(spacy_lemma))
    rs = bucket_rank.get(s)            // Option<u32>
    rl = bucket_rank.get(l)            // Option<u32>
    pick the stem whose bucket-rank is lower (ties → l)
    return that stem (or l if neither stem has a bucket hit)
```

Where:
- `normalize` is the existing `normalize_spanish_lemma` (lowercase, NFD strip).
- `stem` is `rust-stemmers::Stemmer::create(Algorithm::Spanish)` (Snowball).
- `bucket_rank: HashMap<String, u32>` is a startup-built map: stem every
  entry in `es_master_frequency_list.txt`, aggregate by `min(rank)`.

A wlemma is an implementation detail. It is **not** human-readable
(`nin`, `corr`, `camioner`). The spaCy lemma and surface form remain the
only human-facing strings.

## Empirical evidence (La_Llarona_ULa34.txt, 1,913 word tokens)

| Strategy | Mean rank | Median | p99 | Max | tokens >50k |
|---|---|---|---|---|---|
| Direct (current) | 2,099 | 29 | 22,821 | 1,139,313 | 7 |
| `min(stem(lemma), stem(surface))` | **723** | 29 | 12,735 | **42,635** | **0** |

3× lower mean, half the p99, max cut by 27×, no tokens >50k, zero regressions.
See `bucket_rank_prototype.py` and `compare_lemmatizers.py` for the full
analysis.

## Architecture

### Where wlemmas are computed

Two ingestion sites have surface and spaCy lemma in scope; both must
populate wlemmas:

1. **Tier text lemmatization** —
   `update_lemmas_from_spacy` in
   [src/domain/token_stream.rs](src/domain/token_stream.rs), invoked from
   [src/app/engine.rs](src/app/engine.rs) `lemmatize_tier`. Fills
   `WordData.{text,lemmas}`. Add: also fill `WordData.wlemmas`.
2. **Mapping target lemmatization** — `lemmatize_mapping_targets` in
   [src/app/engine.rs](src/app/engine.rs). Fills
   `MappingEntry.{target_text, target_lemmas}`. Add: also fill
   `MappingEntry.target_wlemmas`.

Derived caches (`Segment.lemmas`, `Tier.lemmas`) are rebuilt next to
where they already are, gaining a parallel `wlemmas` rebuild.

### Where wlemmas are stored

The codebase has **four** lemma-bearing locations, but only two are sources
of truth; the others are derived caches:

| Location | File | Field | Role | Wlemma strategy |
|---|---|---|---|---|
| Per-word | `domain/primitives.rs` | `WordData.lemmas` | **source of truth** (filled by spaCy) | Add `wlemmas: Vec<String>` parallel field. Computed in `update_lemmas_from_spacy` from `(text, lemmas[0])`. |
| Per-segment | `domain/segment.rs` | `Segment.lemmas` | derived: flattened from `WordData.lemmas` | Either add `wlemmas` parallel field, or compute on-the-fly from words. Recommend parallel field for consistency and serde ergonomics. |
| Per-tier | `domain/tier.rs` | `Tier.lemmas` | derived: flattened+deduped from `Segment.lemmas` | Same: add `wlemmas: Vec<String>` parallel field. |
| Per-mapping | `domain/mapping.rs` | `MappingEntry.target_lemmas` | **source of truth** for cross-language mappings (filled by spaCy on `target_text`) | Add `target_wlemmas: Vec<String>` parallel field. Computed where `lemmatize_mapping_targets` runs. |

Both source-of-truth sites have the surface form alongside (`WordData.text`,
`MappingEntry.target_text`), so wlemma computation never lacks input.

The triple `(surface, lemma, wlemma)` travels together through the pipeline
and lands in the persisted `.wvl` cache. On lazy upgrade for old `.wvl`
files, derived levels (`Segment.wlemmas`, `Tier.wlemmas`) are rebuilt by
the same flatten/dedup logic from the freshly-computed per-word wlemmas.

### Frequency lookup

`FrequencyManager` gains:
- `bucket_rank: HashMap<String, u32>` built at load time.
- `rank_of_wlemma(&self, w: &str) -> Option<u32>`.

The legacy `get_rank_for_lemma` is kept temporarily (for migration) and
deprecated. New code never calls it.

### Display vs ranking

Two different jobs, two different fields:

- **Ranking / bucketing / AVD scoring / coverage**: `wlemma`.
- **Display to user (flashcards, vocab tracking, lesson UI)**: spaCy lemma
  (or surface, depending on context).

The user-facing rank shown next to a lemma is the **wlemma rank**, since
the spaCy lemma rank is no longer the source of truth for difficulty.

## Persisted-data migration

### `.wvl` cached books

Books that were processed before this change carry `(surface, spacy_lemma)`
per token but no `wlemma`. Migration approach:

1. **Primary mechanism — one-shot upgrade script.** A standalone tool
   (e.g. `scripts/upgrade_wvl_to_wlemma.py` or a Rust subcommand) walks a
   directory, opens each `.wvl`, computes wlemmas in-memory from
   `(text, lemmas)`, and rewrites the file in the new schema. This keeps
   migration logic out of the permanent runtime path. Idempotent — safe
   to re-run.
2. **In-app detection on load (courtesy path).** When the GUI/CLI opens
   a `.wvl` lacking wlemmas:
   - Detect via schema version at deserialization (`#[serde(default)]`
     for the new fields, plus an explicit `schema_version` field).
   - Show a modal dialog: *"This project was created before the wlemma
     upgrade. The app will compute the new data in memory now. Save the
     project to write the upgrade to disk, or run
     `scripts/upgrade_wvl_to_wlemma.py` to upgrade all your projects at
     once."*
   - Compute wlemmas in-memory immediately; session continues normally.
   - Mark project dirty so a save writes the new schema.
   - Show a brief completion toast/dialog when in-memory upgrade finishes.
3. **Schema versioning.** Add a top-level `schema_version` field to the
   `.wvl` JSON. Bump from `1` (or absent) to `2`. The detector keys on
   this rather than on field-presence sniffing.

User plan recorded:
> Bill will copy all current `.wvl` directories before running anything
> (belt-and-suspenders), then either run the script once or save each
> file as it's opened. Once Bill's corpus is upgraded, the in-app path
> remains as a courtesy for cloned-repo users; it can be removed later
> if/when we decide it has served its purpose.

### Frequency list itself

`es_master_frequency_list.txt` does **not** change format. The bucket map
is a runtime artifact built from it on every app startup (~1 second for
3.5M entries). No regeneration of the master file required.

### User progress data (Lessons / Flashcards / known-words)

Existing user-progress storage keys off lemma strings. After the migration:

- New progress entries will be keyed by wlemma (so all forms of a family
  count as one).
- Existing entries keyed by old spaCy lemmas need a one-time map: for each
  saved key `k`, compute `stem(normalize(k))`, treat that as the new
  wlemma. If multiple old keys collapse to one wlemma, take the **most
  generous** progress (max known-count, max mastery level, etc.) so users
  never lose progress.

This conversion can run on first load after the upgrade and overwrite
the saved progress file in-place (with a one-time backup).

**Side effect**: A user who previously "knew" `niño` will now also be
treated as knowing `niña`, `niños`, `niñas`, etc., because they share the
bucket. This is desired behavior in the common case (knowing a word
implies recognizing its inflections), but it is not always linguistically
correct (e.g., bucket `cas` collapses `casa`/`caso`/`casar`).

The decision recorded here:

> The first ~2000 lemmas are believed accurate enough that bucket
> collisions are edge cases. Edge-case "free" vocabulary credit is
> acceptable; if it becomes noticeable in practice, address it as a
> separate project (see Future Work).

## Implementation order

1. Add `rust-stemmers` to `Cargo.toml`.
2. Add `wlemma` module: `compute_wlemma`, `Wlemma` newtype (or alias).
3. Extend `FrequencyManager`:
   - Build `bucket_rank` on load.
   - Add `rank_of_wlemma`.
   - Mark `get_rank_for_lemma` deprecated.
4. Extend the token model with a `wlemma` field.
5. In `update_lemmas_from_spacy`, populate `wlemma` for every token.
6. Migrate AVD/metrics/coverage call sites to `rank_of_wlemma(token.wlemma)`.
7. Add lazy upgrade on `.wvl` load: recompute `wlemma` if missing.
8. Audit and update non-AVD call sites:
   - Lessons module
   - Flashcards module
   - Any UI that displays a rank
   - Any user-progress storage
9. Write user-progress migration (one-time, on first load post-upgrade).
10. Regression test: regenerate basic/moderate/advanced for La_Llorona,
    verify mean log10(rank) and coverage metrics show the expected
    separation between tiers.
11. Remove deprecated `get_rank_for_lemma`.

## Open questions to resolve before implementation

- [x] **Confirm `.wvl` schema currently stores `(surface, spacy_lemma)` per
      token.** ✅ Verified. `WordData` in
      [src/domain/primitives.rs](src/domain/primitives.rs) has both
      `text: String` (surface) and `lemmas: Vec<String>` (spaCy). Lazy
      upgrade is fully viable at the per-word level.
- [x] **Determine where lemma lists live and how they're built.**
      Verified across `src/domain/`:
      - `WordData.lemmas`: source of truth, populated by spaCy ingestion
        in `update_lemmas_from_spacy`.
      - `Segment.lemmas`: derived — flattened from `WordData.lemmas` of
        the segment's stream (see [src/app/engine.rs](src/app/engine.rs)
        lines ~111-118). On lazy upgrade we simply rebuild from words.
      - `Tier.lemmas`: derived — flattened from `Segment.lemmas`, sorted
        and deduped. Same: rebuild from segments.
      - `MappingEntry.target_lemmas` in
        [src/domain/mapping.rs](src/domain/mapping.rs): lemmas for
        `target_text` (the cross-language target word). `target_text` is
        a String stored on the entry, so wlemmas can be computed
        per-entry from `(target_text, target_lemmas)`.
- [x] **Confirm corpus generation tool participation.** ✅
      `create_frequency_list.py` does not need changes; it produces the
      master list as-is, and bucketing happens at consumer side at
      Rust-startup load time.
- [x] **`friendly_shielding` and Lessons friendly lemmas — switch to
      wlemma-rank.** Friendly lemmas for lessons are defined and compared
      on the wlemma side, since that is where "friendliness" (rank) now
      lives. The friendly list itself should be normalized to wlemmas
      at load time, and `apply_friendly_shielding` consults
      `rank_of_wlemma`.
- [ ] Determine whether any cached intermediate JSON between pipeline
      stages stores lemma keys that need migration.
- [ ] Decide on `Wlemma` newtype vs plain `String`: newtype catches
      "passed surface where wlemma expected" bugs at compile time;
      `String` is less ceremony.
- [ ] Confirm location of any user-progress / known-words / Lessons /
      Flashcards storage and the exact serialization format before
      writing the one-time progress migrator.

## Future Work

### Bucket-collision exception list

If specific bucket merges cause measurable issues (e.g., `casa` and
`casar` collapsing into `cas` confuses the difficulty model in a way
users notice), add a manual override map:

```
override_buckets: HashMap<&str /* lemma */, &str /* forced wlemma */>
```

Words in this map bypass the stemmer and get a forced bucket key.
Stays empty by default; populated only as concrete problems are found.

### Family-aware vocabulary tracking

The "knowing `niño` implies knowing `niños`" effect is mostly desirable
but can over-credit users for irregular words (`ir`/`fui`/`vamos` won't
share a stem). A future feature could:
- Show users which forms in a family they've actually seen vs. which are
  inferred-known.
- Allow users to "split" a bucket if they feel the equivalence is wrong
  for their learning style.

### Better stemmer

Snowball is rule-based and language-specific but not perfect. If
collision quality becomes the bottleneck, alternatives include:
- A morphologically-trained Spanish lemmatizer (e.g., Stanza) used
  alongside Snowball, with `min(stanza_stem, snowball_stem, ...)`.
- A learned subword tokenizer (BPE/Unigram) trained on the freq list.
Out of scope for this migration.

### Display: family transparency

The `(surface, lemma, wlemma)` triple is an implementation detail for end
learners, but **must be visible to content creators**. As the app
fleshes out, hiding the wlemma layer from authoring tooling will produce
"why is this rare word counted as common?" confusion that is much harder
to explain after the fact than up front.

Design rule:

- **Authoring/inspection UI** (rank inspectors, sentence breakdowns,
  vocabulary previews, debug overlays): show wlemma alongside human
  lemma, surface the bucket family on demand.
- **Learner UI**: wlemma stays hidden; only surface/lemma are shown.

Future polish: optional "family" hover/popover for advanced learners who
opt in ("you've seen: niño, niños, niña").

---

## TODO (work order)

Mark these completed as we go. Items roughly grouped by phase.

### Phase 1 — Plumbing (no behavior change yet)

- [x] **T1.1** Add `rust-stemmers` to `Cargo.toml`.
- [x] **T1.2** Create `src/domain/stemmer.rs` with a `Stemmer` trait and
      a `SpanishSnowball` impl. Add a small factory keyed by `lang_code`.
- [x] **T1.3** Create `src/domain/wlemma.rs` exposing
      `compute_wlemma(surface, spacy_lemma, &Stemmer, &BucketRanks) -> String`
      and a doc-comment explaining the min(lemma, surface) algorithm.
- [x] **T1.4** Decide newtype vs alias for `Wlemma`. **Decision: alias**
      (`pub type Wlemma = String`). Minimal serde churn; fits existing
      `Vec<String>` lemma fields. Promote to a newtype later if we need
      stronger type-level guarantees.

### Phase 2 — Frequency manager + bucket map

- [x] **T2.1** Extend `FrequencyManager` to build
      `bucket_rank: HashMap<String, u32>` at load time using the active
      stemmer.
- [x] **T2.2** Add `rank_of_wlemma(&str) -> Option<u32>`.
- [x] **T2.3** Mark `get_rank_for_lemma` `#[deprecated]` (don't remove yet).
- [x] **T2.4** Add a startup log line: bucket count, avg lemmas/bucket,
      top-20 buckets by `max_rank - min_rank` spread (the spots the fix
      is actually rescuing).

### Phase 3 — Domain model carries wlemmas

- [x] **T3.1** Add `wlemmas: Vec<String>` to `WordData` (with `#[serde(default)]`).
- [x] **T3.2** Add `wlemmas: Vec<String>` to `Segment` (derived).
- [x] **T3.3** Add `wlemmas: Vec<String>` to `Tier` (derived, deduped).
- [x] **T3.4** Add `target_wlemmas: Vec<String>` to `MappingEntry`.
- [x] **T3.5** Add top-level `schema_version: u32` to the `.wvl`
      project root struct.

### Phase 4 — Populate at ingestion

- [x] **T4.1** In `update_lemmas_from_spacy` (`token_stream.rs`), compute
      and store `wlemmas` on every `WordData`.
      *Implementation note*: to avoid a `domain → simulation` dependency,
      wlemma population was extracted into
      `domain::wlemma::compute_wlemmas_for_word` and called from the two
      ingestion wrappers in `engine.rs` (Phase 4.2/4.3) immediately after
      `update_lemmas_from_spacy`. Net effect is identical: every word
      ends up with `wlemmas` populated.
- [x] **T4.2** In `lemmatize_tier` (`engine.rs`), rebuild
      `Segment.wlemmas` and `Tier.wlemmas` next to the existing
      `lemmas` rebuild.
- [x] **T4.3** In `lemmatize_mapping_targets` (`engine.rs`), populate
      `target_wlemmas`.

### Phase 5 — Switch consumers to wlemma rank

- [x] **T5.1** AVD scoring / metrics / coverage call sites (audit
      `get_rank_for_lemma` consumers in `engine.rs`,
      `corpus_generator.rs`, `simulation/`) → switch to `rank_of_wlemma`.
      *Implementation note*: added `frequency_manager::rank_of_lemma_string`
      (stem-then-bucket-lookup) as the migration helper; all 24 legacy
      call sites now use it. The stemmer is stored on `FrequencyData`
      so lookups can stem at query time.
- [x] **T5.2** `friendly_shielding` (`mapping_logic.rs`) → take
      `rank_for_wlemma` instead of `rank_for_lemma`. Friendly list
      itself normalized to wlemmas at load time.
      *Implementation note*: `apply_friendly_shielding` gained a
      `key_for_lemma: G` closure parameter. Production caller in
      `engine.rs` builds it from the active-language stemmer and
      stems both the user's friendly list and each candidate lemma
      through it.
- [x] **T5.3** Terminal command `lemma_rank` (or whatever displays rank
      to the user) → show wlemma rank as the primary number; lemma rank
      retained as a debug detail.
      *Implementation note*: `format_lemma_with_rank` in `terminal.rs`
      now uses `rank_of_lemma_string`, which returns the wlemma
      bucket rank. The displayed lemma text is unchanged; only the
      rank number reflects the bucket.
- [ ] **T5.4** Lessons / Flashcards / known-words storage and lookups
      (after the open-question audit completes).

### Phase 6 — Migration tooling

- [x] **T6.1** Write `scripts/upgrade_wvl_to_wlemma.py` (or a Rust
      subcommand `weavelang upgrade-wvl <dir>`). Idempotent. Walks dir,
      computes wlemmas in-memory, rewrites with `schema_version: 2`.
      *Implementation note*: chose the Rust subcommand path —
      `weavelang_cli upgrade-wvl <path> [--lang <code>]`. Accepts a
      single .wvl file or a directory (recursively scanned). Reuses
      `services::wlemma_upgrade::upgrade_app_state` so the in-app and
      CLI paths share semantics. Files already at v2 are skipped.
- [x] **T6.2** In-app schema-version detector: on load, if
      `schema_version < 2`, run the in-memory upgrade and show a
      modal/toast notifying the user. Mark project dirty.
      *Implementation note*: `LoadProject` in `engine.rs` calls
      `upgrade_app_state` immediately after deserialization. The user
      is notified via the load-result message (e.g.
      `"... [upgraded schema v0 → v2: words=N, segs=N, tiers=N,
      mappings=N; please re-save]"`). The codebase has no
      project-level dirty flag; user re-saves manually after the
      notification.
- [ ] **T6.3** User-progress migration (one-time, on first post-upgrade
      load) — exact form pending the open-question audit.

### Phase 7 — Authoring UI transparency

- [x] **T7.1** Wherever rank or lemma is shown in an authoring/debug
      view, also show wlemma and (on demand) bucket members.

      *Implementation note (Phase 7).* Added `inspect_bucket(lemma)` and
      its pure variant `inspect_bucket_in` to
      `src/simulation/frequency_manager.rs`, returning a
      `BucketInspection { wlemma, rank, members }` (members sorted by
      ascending rank). Exposed via the new terminal command
      **`wlemma <word>`** (`TerminalCommand::WlemmaInspect` in
      `src/app/commands.rs`, parser + dispatcher in
      `src/app/terminal.rs`, help text updated). The existing
      `lemma<rank>` displays already show bucket ranks (since the
      Phase 5 `rank_of_lemma_string` migration), so transparency is now
      on-demand: any surprising rank can be explained by running
      `wlemma <word>` to see the stem and its members. New TT7-style
      unit test `inspect_bucket_in_returns_members_sorted_by_rank`
      covers the pure helper.

- [x] **T7.2** Document the wlemma concept briefly in
      `documentation/Studio_Architecture.md` so future content creators
      learn it before they get confused by it.

      *Implementation note (Phase 7).* Authored the new file with a
      "Wlemma Buckets" section covering: what a wlemma is, why it
      exists, how `compute_wlemmas_for_word` resolves lookups, the
      practical impact on displayed ranks, the `wlemma <word>`
      inspection command, schema-v2 migration pointers, and links to
      the relevant source modules and to this migration plan.

### Phase 8 — Validation

- [ ] **T8.1** Regenerate basic/moderate/advanced for La_Llorona;
      confirm mean log10(rank) and head-share coverage now distinguish
      basic from moderate.

      *Implementation note (Phase 8, side-fix).* While auditing how the
      calibrator handles proper nouns, found that
      `simulation::calibrator::build_unified_avd_cache` was using
      `TextMetrics::new(&result.all_output_lemma_instances, …)`
      (V1: includes proper nouns, applies a 0.2% tally cap), and was
      *incidentally* dropping proper nouns only because spaCy's
      surface-form lemmas (`María`, `Llorona`, `Camionero`, …) were
      missing from the master frequency list. Once wlemma bucketing
      started stemming those surfaces to real buckets (`mari`, `llor`,
      `camioner`), they began slipping into the AVD calculation and
      pulling it down. Switched the calibrator to
      `TextMetrics::new_v2(&result.all_output_lemma_instances_v2, …)`,
      which excludes proper nouns explicitly by ID (the
      `corpus_generator` already populates the `_v2` list this way) and
      drops the V1-only tally cap. Tier eligibility (`core_algo::
      is_lemma_known_for_tier`) was always handled correctly — it
      short-circuits on `pn_lemma_ids` before any rank lookup — so the
      weave decision is unchanged. Note: the `A_FIT`/`B_FIT` analytical
      fit constants in `calibrator.rs` were originally fit against V1
      measurements; the curve is still monotonic against V2 so level
      maps remain well-formed, but absolute V-level boundaries will
      shift. Every book needs recalibration after this change (which
      was already required for the wlemma migration anyway). 265/4
      lib tests still passing.
- [x] **T8.2** Regression: full `cargo test` suite passes.

      *Implementation note (Phase 8).* `cargo test --lib` is at
      265 passed / 4 failed. The 4 failures are the pre-existing
      `services::av_producer::tests::*` set tracked under T9.3; no new
      regressions were introduced by Phases 1–8.

- [x] **T8.3** Spot-check the previously-broken sentences (`Niños`,
      `Camioneros`, `gritándoles`, `Corres`) and confirm they no longer
      get flagged as advanced.

      *Implementation note (Phase 8).* Authored
      `tests/wlemma_spot_check.rs` (`#[ignore]`'d so it doesn't load the
      full master list on default `cargo test` runs). It loads the real
      `assets/frequency_lists/es_master_frequency_list.txt` and asserts
      that each broken surface (a) shares a wlemma bucket with its
      canonical lemma, (b) has a bucket rank below the 50,000
      advanced-tier penalty floor, and (c) returns the same rank via
      `rank_of_lemma_string` and `inspect_bucket`. Initial run revealed
      a real bug: the master list is ASCII-folded
      (`niño` → `nino`, `gritándoles` → `gritandoles`, …), and Snowball
      Spanish strips á/é/í/ó/ú on its own but **preserves `ñ`** — so any
      `ñ`-bearing surface was stemming to a bucket key that didn't
      exist in the loaded list (`niño` → `niñ`, but the list only has
      `nin`). Fix: added `SpanishSnowball::fold_diacritics` (an ASCII
      pre-pass folding á/à/ä/â, é/è/ë/ê, í/ì/ï/î, ó/ò/ö/ô, ú/ù/ü/û and
      ñ together with their uppercase variants) before delegating to
      Snowball. New unit test
      `domain::stemmer::tests::ascii_folded_and_diacritic_forms_share_a_bucket`
      pins the contract. Both spot-check tests pass against the real
      master list. Run with
      `cargo test --test wlemma_spot_check -- --ignored`.

### Phase 8c — Enclitic-pronoun rescue (Spanish)

After Phase 8 went live, post-migration analysis on
`La_Llorona_ULa34.txt` surfaced a class of high-rank lemmas that wlemma
could not rescue: spaCy hallucinations triggered by Spanish enclitic
pronouns attached to imperatives, infinitives, or gerunds.

| Surface | Real verb + clitic | spaCy hallucinated lemma | Wlemma bucket | Rank |
|---|---|---|---|---|
| `Acércate` | acerca + te | `acercatir` | `acercatir` | 10,063 |
| `sentarte` | sentar + te | `sentarte` | `sentart` | 23,134 |
| `siéntate` | sienta + te | `sientatir` | `sientatir` | 2,350 |
| `gritándoles` | gritando + les | `gritándoles` | `gritandol` | 1,545,889 |

The `min(stem(surface), stem(lemma))` rule could not help here because
surface and lemma agree on the malformed form, and the master frequency
list (built with the same spaCy) contains the malformed bucket as a
"real-looking" entry.

- [x] **T8c.1** Add `Stemmer::strip_enclitics(word) -> Option<String>`
      to the trait. Default returns `None` (no-op for languages without
      enclitics). Implement on `SpanishSnowball` with a closed list of
      pronoun suffixes (`me te se lo la le nos os los las les` plus the
      dative+accusative combos `melo mela telo tela selo sela …` and
      `nos`-prefixed compounds, longest-first). Gate: only strip when
      the original word contains an accented vowel (imperative/gerund
      with stress shift) or the stripped remainder ends in `-ar`/`-er`/
      `-ir` (infinitive + clitic), and the remainder is ≥3 chars.
      Stop at first matching suffix — if it fails the gates, return
      `None` rather than fall through to a shorter clitic (which would
      leave a residual clitic on the front of the remainder). New
      tests in `domain::stemmer::tests`:
      `strip_enclitics_imperative_with_accent`,
      `strip_enclitics_infinitive_plus_clitic`,
      `strip_enclitics_gerund_with_compound_clitic`,
      `strip_enclitics_skips_non_verb_lookalikes` (locks in that
      `carteles`/`papeles`/`hoteles` are NOT stripped).

- [x] **T8c.2** Extend `compute_wlemma` to consider a third "salvage"
      candidate: `stem(strip_enclitics(surface))`. The `min`-rank rule
      now picks the best of `{lemma_stem, surface_stem, stripped_stem}`.
      Tie-break order unchanged (lemma > surface > stripped). The
      stripped candidate only wins on strict rank improvement, so it
      cannot pollute non-verb buckets that happen to end in
      clitic-shaped suffixes. New tests in `domain::wlemma::tests`:
      `enclitic_hallucinations_rescued_by_strip_candidate` (asserts
      the four canonical surfaces resolve to the rescue bucket when
      both lemma and surface point at malformed buckets) and
      `strip_candidate_does_not_steal_non_verb_buckets` (asserts that
      even an artificially-attractive `cart`/`carte` bucket cannot
      hijack `carteles` because the strip path is gated off).

- [x] **T8c.3** Regression: full `cargo test --lib` suite is at
      271 passed / 4 failed (the 4 remain the pre-existing
      `services::av_producer::tests` set tracked under T9.3); the
      `wlemma_spot_check` integration test against the real master
      list still passes.

- [x] **T8c.4** Add `--force` to `weavelang_cli upgrade-wvl` so saved
      `.wvl` files can be re-bucketed in place after a wlemma-algorithm
      tweak (Phase 8c, future Phase 8d, …) without re-ingesting through
      spaCy. Wlemmas are computed at ingestion and persisted on every
      `WordData`, `Segment`, `Tier`, and `MappingEntry`; without a
      forced re-compute path, an algorithm change has no effect on
      already-saved books.

      *Implementation note.* Added `upgrade_app_state_with_force` and
      `upgrade_app_state_force` in
      `src/services/wlemma_upgrade.rs`. When `force=true`, the
      schema-version short-circuit is skipped and every wlemma field
      is re-computed from `(text, lemmas)` + the active stemmer.
      `already_at_target` in the returned report still reflects the
      pre-call schema version so the CLI can emit `[RECOMPUTED]` (vs
      `[UPGRADED]`) and skip the "already at v2" early-out. Existing
      `upgrade_app_state_with` / `upgrade_app_state` are kept as
      `force=false` thin wrappers so all current call sites are
      unchanged.

      CLI: `weavelang_cli upgrade-wvl <path> [--lang <code>] [--force]`.
      `--force` re-writes every file regardless of schema version and
      tags the log line `[RECOMPUTED]`. Use after any change to
      `compute_wlemma`, `Stemmer::stem`, or `Stemmer::strip_enclitics`.

- [x] **T8c.5** Make the print/inspect surfaces show the stored wlemma
      bucket's rank rather than re-stemming the displayed lemma text.
      Before this fix, `format_lemma_with_rank` in
      `src/app/terminal.rs` called `rank_of_lemma_string(lemma)`,
      which stems the *lemma* and looks up its bucket — bypassing the
      `min(stem(lemma), stem(surface), stem(strip_enclitics(surface)))`
      logic that produced the *stored* wlemma. Result: a sentence
      whose AVD scoring correctly used the rescued bucket would still
      print the malformed bucket's rank
      (`acercatir<10063>`, `sentarte<23134>`).

      *Implementation note.* Added
      `format_lemma_with_wlemma_rank(lemma, wlemma)` and
      `format_lemmas_with_wlemma_ranks(lemmas, wlemmas)` in
      `terminal.rs`, which look up rank by the stored wlemma bucket
      key but display the lemma text. Falls back to the legacy
      re-stem path when the parallel slices are not 1:1 aligned
      (zero-length wlemmas, or rare cases where multiple lemmas in a
      word collapsed into a single wlemma during dedup). Wired the
      four print sites that have parallel data: `ShowMapping`,
      `Print` (column-width pre-pass + inline lemma column for mapped
      tiers), and the segment-tier branch of `Print`.

      *Note.* Strictly fixing this in the wlemma layer means rebuilding
      the master frequency list with a better lemmatizer is no longer
      urgent for clitic-heavy text. The remaining tail of malformed
      buckets (`esperser`, `necesitarar`) appear to be a different
      class — spaCy generating wrong infinitives without clitic input
      — and affect a much smaller token fraction. Defer until a
      concrete book exposes them at non-trivial scale.

### Phase 8d — Radical-change (stem-changing) verb rescue (Spanish)

Snowball Spanish does not undo stressed-stem diphthongization, so even
after Phase 8c clitic strip, forms like `siéntate` (sit-imperative)
land in the `sient` bucket while their infinitive `sentar` lives in
`sent`. Same hazard applies to `cuéntame`/`contar`,
`duérmete`/`dormir`, `piénsalo`/`pensar`, etc. Phase 8d adds a fourth
salvage candidate: un-mutate the stressed-stem diphthong (`ie → e`,
`ue → o`) in the strip remainder, then snowball-stem.

Safety is gated by `strip_enclitics` having already matched. The
un-mutation operation by itself is over-applicable (it would happily
mangle `puerta` → `porta`, `tiempo` → `tempo`, `bueno` → `bono`), but
none of those non-verbs survive the strip-enclitics gate, so the
unsafe rule never sees them.

- [x] **T8d.1** `Stemmer::unmutate_radical_change(word) -> Vec<String>`
      trait method, default empty. `SpanishSnowball` implementation
      replaces the first occurrence of `ie` with `e` and the first
      occurrence of `ue` with `o`, independently — both candidates
      emitted if both diphthongs appear. Diacritic fold applied
      internally so callers can pass the raw strip remainder
      (`siénta` → fold `sienta` → un-mutate `senta`). Word-initial
      occurrences (`idx == 0`) are skipped.

- [x] **T8d.2** Wire un-mutated candidates into `compute_wlemma`. The
      strip remainder is computed once and reused for both Phase 8c
      strip-stem and Phase 8d radical-change derivations. Each
      un-mutated variant is snowball-stemmed and deduped against the
      lemma/surface/stripped candidates. Same strict-improvement
      tie-break rule — radical candidates only win on lower rank.

- [x] **T8d.3** Regression coverage:
      - `unmutate_radical_change_round_trip_stems_match_infinitive`
        (stemmer.rs): for `siénta`, `cuénta`, `duérme`, `piénsa`,
        `vuélve`, the un-mutated candidate's snowball stem equals
        the infinitive's snowball stem.
      - `radical_change_rescues_stressed_stem_imperatives` (wlemma.rs):
        `siéntate`, `cuéntame`, `duérmete` route to the infinitive
        bucket when it's strictly cheaper.
      - `radical_change_skipped_for_non_enclitic_words` (wlemma.rs):
        `puerta`, `tiempo`, `bueno`, `fuerte`, `cuerpo` retain their
        own stems even with artificially attractive un-mutated buckets,
        because the strip-enclitics gate refuses them.

      *Coverage gap (acceptable).* The third radical-change pattern,
      `e → i` for `-ir` verbs (`pídelo` ← `pedir`, `sirve` ← `servir`),
      is not handled. The pattern is harder to gate safely (many
      stressed-`i` words in Spanish are not stem-changing verbs) and
      affects a smaller corpus fraction. Defer until a concrete book
      surfaces the gap.

### Unit tests (woven through phases)

Existing unit tests are already failing on `main`; do not treat that as a
hard blocker. The rule is:

- When a failing test covers code we are about to touch, fix it (update
  expectations to the new wlemma-based reality) as part of the same
  commit.
- When a failing test covers unrelated code, leave a `// FIXME(wlemma):`
  comment and keep going — log it but don't fold it into this migration.
- Every new module/function added in this plan ships with at least one
  unit test in the same commit.

Per-phase test obligations:

- [x] **TT1** (with Phase 1) `stemmer.rs`: SpanishSnowball produces
      expected stems for `niños/niño/niña`, `camioneros/camionero`,
      `corres/correr`, `gritándoles/gritar`, plus a few closed-class
      words (`los`, `de`, `que`) so we lock in their stem behavior.
- [x] **TT2** (with Phase 1) `wlemma.rs`: `compute_wlemma` returns the
      lower-rank stem given a stub `BucketRanks`; tie-breaks to the
      lemma stem; handles empty/missing lemma gracefully.
- [x] **TT3** (with Phase 2) `frequency_manager`: a small fake freq
      list builds a bucket map with correct min-aggregation;
      `rank_of_wlemma` returns expected values; unknown wlemma → `None`.
- [x] **TT4** (with Phase 3) serde round-trip: `WordData`, `Segment`,
      `Tier`, `MappingEntry`, project root all serialize and
      deserialize across the new fields, and an old payload missing
      those fields still loads (default = empty).
- [x] **TT5** (with Phase 4) ingestion: feeding the four canonical
      broken tokens through `update_lemmas_from_spacy` yields wlemmas
      whose ranks land in basic/moderate, not advanced.
- [x] **TT6** (with Phase 5) consumers: AVD/`friendly_shielding` unit
      tests that previously asserted lemma-rank values get updated to
      assert wlemma-rank values; add a regression test that the four
      canonical sentences classify as basic/moderate end-to-end.
- [x] **TT7** (with Phase 6) migration: golden-file test for
      `upgrade_wvl_to_wlemma` (input v1 fixture → output v2 fixture);
      idempotency test (running twice = running once).

### Phase 9 — Cleanup

- [ ] **T9.1** Remove `#[deprecated] get_rank_for_lemma` once no caller
      remains.
- [ ] **T9.2** Decide whether to keep the in-app schema upgrade path or
      remove it in favor of the script (Bill: leave it for now for
      cloned-repo users; revisit later).
- [ ] **T9.3** Investigate the 4 pre-existing failing
      `services::av_producer::tests` (`scan_detects_audio_and_video`,
      `scan_finds_text_files`, `scan_detects_volume_audio_files`,
      `mark_all_and_clear`). They were failing on `main` before this
      migration started — file as a follow-up so we don't lose the
      thread; they may indicate other latent issues.


