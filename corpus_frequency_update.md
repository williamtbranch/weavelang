# Corpus & Frequency List Update — Migration Plan

**Status:** Draft for execution
**Owner:** Bill
**Created:** 2026-08-05

Replace the current parliamentary-weighted Spanish master frequency list with a
list built from a closed-captions (and optionally prose) corpus, force official
CEFR band membership into the resulting ranking, and recalibrate the level
system without invalidating the ~200 already-published YouTube videos.

---

## 1. Why

The frequency list is not only the *measuring* instrument (AVD → UL labels). It
is also the *generation* instrument: `is_lemma_known_for_tier()` in
[src/simulation/core_algo.rs](src/simulation/core_algo.rs) tests `rank <=
tier_v_level` to decide which lemmas a learner at a given level is assumed to
know, and therefore what gets glossed and simplified in every video produced.

The current list is EU-parliament/formal weighted. Evidence from our own
generated artifact, [assets/level_maps/es_master_level_map.txt](assets/level_maps/es_master_level_map.txt)
(threshold-crossing lemma per level):

| Level | Vocab size | Crossing lemma |
|------:|-----------:|----------------|
| 11 | 58 | dos |
| **12** | 77 | **comisión** |
| 14 | 132 | sistema |
| 16 | 213 | relativo |
| 19 | 411 | trabajador |
| 21 | 617 | tecnología |
| 24 | 1,122 | ok |
| 27 | 2,119 | intervenir |
| **31** | 7,629 | **transbordador** |
| 33 | 23,937 | prostituir |

A UL-12 learner is assumed to know *comisión* before *sol*, *frío* or *árbol*.
Story-core vocabulary (weather, nature, body, emotion) is systematically
under-ranked, which is why the adaptation DRC penalises exactly the words a
graded reader needs.

The tail is worse. The list holds **3,420,765 "lemmas"** for a language with
roughly 100–150k. Levels 36–40 cross at `antojasar`, `loverde`, `erespolicia`,
`cacuruse`, `quelala` — concatenation and ASR garbage. Roughly 97% of the list
is noise.

---

## 2. Scope

**In scope**

- New corpus ingestion → new master frequency list
- CEFR (A1/A2/B1/B2) band forcing into the ranking
- Regeneration of the level map and all calibration artifacts
- Re-fit of the AVD→UL curve to preserve published UL labels
- Validation harness comparing old vs. new instrument

**Out of scope (explicitly not changing)**

- The AVD formula itself (tail-weighted p85/p95, Gregor tally cap)
- The i-score / coverage gate definition
- The wlemma bucket (Snowball stem) mechanism
- Already-published video files — they are rendered artifacts and are not
  retroactively affected

---

## 3. Current architecture — the dependency chain

| Layer | Artifact / code | Notes |
|---|---|---|
| Corpus | [assets/corpus_output.txt](assets/corpus_output.txt) | Coverage stats only, 2.74B tokens, dated 2025-09-12 |
| List builder | [create_frequency_list.py](create_frequency_list.py) | Resumable, spaCy lemmas, **ASCII-folds accents and ñ→n** |
| List | [assets/frequency_lists/es_master_frequency_list.txt](assets/frequency_lists/es_master_frequency_list.txt) | TSV: `lemma \t rank \t count`, 1 header line |
| Loader | [src/simulation/frequency_manager.rs](src/simulation/frequency_manager.rs) | `load_master_frequency_list()`; builds `lemma_to_rank`, `rank_to_lemma`, `bucket_rank` |
| Bucketing | `build_bucket_rank()` | Snowball stem → `min(rank)` across the bucket; language hardcoded `"es"` |
| Lookup | `rank_of_lemma_string()` | Primary access point for all scoring |
| Level map | [assets/level_maps/es_master_level_map.txt](assets/level_maps/es_master_level_map.txt) | Produced by [src/bin/level_analyzer.rs](src/bin/level_analyzer.rs) |
| AVD | [src/simulation/metrics.rs](src/simulation/metrics.rs) | Tail-weighted p85/p95 |
| Curve | `A_FIT = 4.15`, `B_FIT = 0.02` | `UL = 4.15·ln(AVD+1) + 0.02` |
| Hunter | [src/simulation/avd_hunter.rs](src/simulation/avd_hunter.rs) | Binary-searches v_level for a target AVD |
| Calibrator | [src/simulation/calibrator.rs](src/simulation/calibrator.rs) | Writes `u_level_maps` (recipe + target/actual AVD) into book JSON |
| Generation | [src/simulation/core_algo.rs](src/simulation/core_algo.rs) | `is_lemma_known_for_tier(rank <= tier_v_level)` |
| Adapt DRC | [src/simulation/escore.rs](src/simulation/escore.rs) | `RARE_RANK_THRESHOLD = 400`, `UNKNOWN_RANK = 20_000`, `OFFENDER_LIMIT = 30` |

**Isolation note:** `escore.rs` is consumed only by the adaptation feature
(`src/app/adapt.rs`, `src/services/adapt_worker.rs`,
`src/gui/components/raw_source_view.rs`). Nothing published depends on it. This
makes it a zero-risk pilot surface — see Phase 4.

---

## 4. Defects to fix *before* migrating (Phase 0)

These are pre-existing problems that will silently corrupt the migration if
left in place.

### 4.1 The UL curve constants are duplicated in three places

- [src/simulation/calibrator.rs](src/simulation/calibrator.rs) lines 55–56
- [src/app/engine.rs](src/app/engine.rs) lines 4762–4763
- [fit_and_curve.py](fit_and_curve.py) line 70

Re-fitting with three copies in play will desync the GUI from the calibrator and
produce two different UL numbers for the same text. **Collapse to a single
source** (Rust constant exported from `calibrator`, Python reading it from a
generated file or a shared TOML) before any re-fit.

### 4.2 `UNKNOWN_RANK` is smaller than the list's maximum rank

`UNKNOWN_RANK = 20_000`, but the list contains 3.42M ranked lemmas. Any lemma
ranked worse than 20,000 is therefore scored as **rarer than a word that is not
in the list at all** — the "conservative screen" is inverted for 99% of the
list's entries.

Fix during migration: truncate the published list at or below `UNKNOWN_RANK`, or
raise `UNKNOWN_RANK` above the list length. Recommendation is truncation
(see §6.4) — nothing beyond ~20k affects scoring anyway.

### 4.3 Output path mismatch in the list builder

[create_frequency_list.py](create_frequency_list.py) writes to `assets/` while
[src/bin/level_analyzer.rs](src/bin/level_analyzer.rs) reads the hardcoded
`assets/frequency_lists/es_master_frequency_list.txt`. Align them, and make the
level analyzer take the path as an argument so old/new lists can be compared
without moving files.

### 4.4 Normalization convention is load-bearing and undocumented

`create_frequency_list.py` folds accents and ñ→n (`año` → `ano`), and
`normalize_spanish_lemma` in the Rust side must match exactly. **Any new list
must use the identical folding**, or every lookup silently misses and returns
`UNKNOWN_RANK`. Add a startup assertion: probe a handful of known lemmas
(`ano`, `nino`, `pequeno`) after load and fail loudly if they miss.

---

## 5. Phase 1 — Corpus acquisition

### 5.1 Source selection

Closed captions alone are dialogue-weighted: `vale`, `tío`, `hola`,
interjections and address forms rank high, while narrative and descriptive
vocabulary (`paisaje`, `sendero`, `orilla`) stays low. We publish *narrated
graded readers*, not dialogue.

**Recommended blend** (weights to be decided in Phase 1 review):

| Source | Purpose | Indicative weight |
|---|---|---|
| Closed captions / subtitles | Spoken register, high-frequency core | 50–60% |
| Prose / books (public domain + contemporary if licensable) | Narrative and descriptive vocabulary | 30–40% |
| News / general web | Topical breadth | 10% |

Record the exact blend and per-source token counts in the header of the
generated list — the list must be reproducible.

### 5.2 Noise control

Current tail garbage (`erespolicia`, `quelala`) shows the builder has no
filtering. Add, in order:

1. **Minimum occurrence floor** — drop lemmas below N occurrences (start at
   N = 50 for a 1B-token corpus; tune so the surviving count is plausible).
2. **Dictionary intersection** — keep only lemmas present in a Spanish
   dictionary / spaCy vocab, with an explicit allowlist for legitimate items
   the dictionary misses (proper-noun-derived, loanwords, regional forms).
3. **Length and shape filters** — reject > 20 chars, reject items that split
   cleanly into two high-frequency lemmas (catches `erespolicia`).
4. **Manual review of the top 2,000** — this is the band that determines
   almost all generation behaviour. Eyeball it.

Target size after filtering: **60k–150k lemmas**.

### 5.3 Deliverables

- `assets/frequency_lists/es_master_frequency_list_v2_raw.txt` (pre-CEFR)
- Updated corpus coverage stats (regenerate `assets/corpus_output.txt` format)
- A short build manifest: sources, weights, token counts, filter settings, date

---

## 6. Phase 2 — Forcing the official CEFR lists in

This is the part with the most design freedom, so the mechanism is spelled out.

### 6.1 The problem with the obvious approaches

- **Pure clamp / rank flooring** ("every A1 word gets rank ≤ 500") produces
  collisions and gaps — many lemmas want the same rank, and the resulting
  ordering is undefined.
- **Pure CEFR ordering** (A1 block, then A2 block, …) guarantees curriculum
  alignment but buries genuinely high-frequency words that the CEFR inventory
  happens to omit, behind all of B2.
- **Score blending** (`w·corpus_rank + (1-w)·cefr_rank`) is smooth but opaque:
  no one can explain why a given word landed where it did.

### 6.2 Recommended: banded partition sort with a frequency rescue

Build the final rank by sorting on a composite key, then assigning rank =
position:

```
sort_key(lemma) = (band_index(lemma), corpus_rank(lemma))

band_index:  A1 → 0
             A2 → 1
             B1 → 2
             B2 → 3
             non-CEFR → 4
```

Properties:

- CEFR bands land as contiguous rank blocks, in curriculum order.
- **Within** a band, ordering is still corpus frequency — so the most useful A1
  words come first and the odd classroom noun (`bolígrafo`, `pizarra`) sinks to
  the back of its own band instead of poisoning the top 100.
- Deterministic, collision-free, trivially explainable, and fully reversible
  (drop the band term to get the pure corpus list back).

**Frequency rescue rule.** Any non-CEFR lemma whose corpus rank is better than
`RESCUE_THRESHOLD` (start at 500) is promoted to `band_index = 0`. Rationale: if
a word is that common in real speech, it is functionally A1 regardless of
whether the published inventory lists it. This covers function words, common
discourse markers, and regional variants the CEFR lists omit. Every rescued
lemma must be written to a rescue log for review — a long rescue list is a
signal the CEFR extraction is incomplete.

**Demotion guard (optional, decide in review).** A CEFR lemma whose corpus rank
is worse than `DEMOTE_THRESHOLD` (e.g. 25,000) is suspicious — likely an
extraction error or a genuinely dead word. Log these; do not auto-demote in v1.

### 6.3 Sourcing the CEFR inventories — open work item

The authoritative Spanish source is the **Instituto Cervantes *Plan Curricular
del Instituto Cervantes* (PCIC)** inventories (Nociones generales / Nociones
específicas, banded A1-A2 / B1-B2 / C1-C2).

Known complications, to be resolved before Phase 2 executes:

- It is published as prose/HTML, not a machine-readable word list. Extraction
  and cleanup is real work, not a download.
- It is **copyrighted by Instituto Cervantes** — check licensing before
  redistributing any derived list in the repo. A derived *rank ordering* may be
  defensible where a verbatim word list is not. Confirm before committing.
- Entries are frequently **multi-word** ("de vez en cuando", "tener ganas de").
  Decide: drop them, or reduce to the head lemma. Note that
  `create_frequency_list.py` already takes only the first token of a multi-word
  lemma, so be consistent.
- Entries are **surface forms and senses**, not lemmas. They must be lemmatized
  with the *same* spaCy pipeline and put through the *same* ASCII folding as
  the corpus list, or they will not match.
- PCIC bands are A1-A2 / B1-B2 / C1-C2 *pairs*. If a finer A1-vs-A2 split is
  needed, a secondary source is required. If it is not available, collapse to
  three bands and adjust `band_index` accordingly.

**Fallback if PCIC proves impractical:** use an openly licensed CEFR-graded
Spanish vocabulary list, and document the substitution. Do not silently swap
sources — the provenance belongs in the build manifest.

### 6.4 Truncation

After banding, truncate at the point where remaining lemmas no longer affect
scoring. Given `UNKNOWN_RANK` (see §4.2), truncate at **20,000** and set
`UNKNOWN_RANK` to `list_len + 1`, or keep ~60k and raise `UNKNOWN_RANK` to
match. Either is fine; the invariant to enforce is:

> `UNKNOWN_RANK > max(rank)` for every entry in the shipped list.

Add a load-time assertion in `frequency_manager` so this can never silently
regress.

### 6.5 Interaction with wlemma bucketing — validate explicitly

`build_bucket_rank()` collapses every lemma to its Snowball stem and takes
`min(rank)` across the bucket. Forcing a CEFR word to a low rank therefore
drags its **entire stem bucket** down with it. Usually desirable (`hablar`
pulls `hablado`, `hablando`), occasionally not (an unrelated rare word sharing
a stem inherits an A1 rank).

Deliverable: a report of the top-50 buckets by rank spread after banding,
reusing the existing `log_top_spread_buckets()` instrumentation. Review by hand.

### 6.6 Deliverable

- `assets/frequency_lists/es_master_frequency_list_v2.txt` (banded, filtered,
  truncated)
- `rescue_log.txt`, `demotion_log.txt`, `bucket_spread_report.txt`

---

## 7. Phase 3 — Recalibration chain

Strict order; each step consumes the previous step's output.

1. **Install list** → `assets/frequency_lists/`. Keep v1 alongside; use
   `config set custom_frequency_list_path` as the A/B switch.
2. **Regenerate the level map**
   `cargo run --bin level_analyzer --release > assets/level_maps/es_master_level_map_v2.txt`
   (after §4.3 makes the input path a parameter).
   *Acceptance:* eyeball the crossing-lemma column. `comisión` at L12 and
   `transbordador` at L31 must be gone; expect concrete high-frequency
   vocabulary instead.
3. **Re-run the AVD hunter** — [run_avd_hunter.ps1](run_avd_hunter.ps1) — to
   regenerate the empirical (AVD, UL) pairs under the new list.
4. **Re-fit the curve** — see Phase 4. Do *not* simply reuse 4.15 / 0.02.
5. **Recalibrate books** — [run_l_level_calibrator.ps1](run_l_level_calibrator.ps1),
   or `calibrate 40` per book. Every book JSON's `u_level_maps` (recipe +
   `target_avd`) is stale until this runs.
6. **Regenerate corpus coverage stats** for the record.

---

## 8. Phase 4 — Protecting the 200 published videos

### 8.1 The risk

Nothing already rendered changes. The risk is **catalog discontinuity**: "UL 15"
published next year not meaning what "UL 15" meant last year. Direction of the
shift: narrative vocabulary gets much better ranks under the new list, so
measured AVD drops and the *same* story scores a *lower* UL. Left uncorrected, a
post-migration "UL 20" would be noticeably harder than an existing "UL 20".

### 8.2 The lever

`A_FIT` / `B_FIT` are a free two-parameter mapping from AVD to a printed label.
The corpus change can be absorbed into the curve fit instead of into the
catalog.

**Anchor-fit procedure:**

1. Choose 12–20 **anchor texts** from already-published videos, spanning the
   full UL range actually used, with their published UL labels.
2. Compute `AVD_new` for each under the v2 list.
3. Fit `A`, `B` in `UL = A·ln(AVD+1) + B` minimising squared error against the
   **published** labels.
4. Report per-anchor residuals.

**Acceptance gate:** median absolute residual ≤ 0.5 UL and max ≤ 1.5 UL.

**If the gate fails:** the two instruments are not reconcilable by a
two-parameter curve — which is itself a meaningful finding, since it means the
old labels were not internally consistent. In that case abandon continuity,
accept a one-time renumbering, and publish an old→new mapping table. Decide
this consciously; do not force a bad fit.

### 8.3 Why this is legitimate

The frequency list decides *which words are common* (generation quality). The
curve decides *what number gets printed* (catalog continuity). They are
separable, so we can take the better generation instrument without renumbering
the back catalog.

---

## 9. Phase 5 — Validation

### 9.1 Sanity board (fast, run first)

Two hardcoded lists checked after every list build:

- **Must be common** (target: top ~1,000): `sol, frío, lluvia, árbol, agua,
  comer, dormir, bosque, viento, pájaro, montaña, nieve, triste, tranquilo`
- **Must NOT be top-500**: `comisión, reglamento, directiva, transbordador,
  intervenir, concretamente, sancionar`

Fail the build on violation. This is the single highest-value check — it is
exactly the defect being fixed.

### 9.2 Dual-report harness

Score a fixed set of texts under v1 and v2 simultaneously and emit a comparison
table (UL_v1, UL_v2, ΔUL, AVD_v1, AVD_v2, top offenders under each). Use both
published stories and new adaptation drafts.

### 9.3 Adapt-DRC pilot (do this before touching the main pipeline)

Because `escore.rs` is adapt-only and nothing published depends on it, wire v2
into the adaptation DRC first and run existing stories through it. This measures
the real magnitude of the shift on real content at zero risk to the catalog,
before a single book is recalibrated.

### 9.4 End-to-end regression

Generate one known book at 3–4 levels under v2 and read the output. Automated
metrics will not catch "the level-8 text now uses a word no beginner knows".

---

## 10. Phase 6 — Rollout and rollback

**Rollout order:** Phase 0 fixes → v2 list → adapt-DRC pilot → level map →
hunter → curve fit → book recalibration → first new video.

**Rollback:** keep v1 list, v1 level map, and v1 curve constants in the repo,
tagged. `custom_frequency_list_path` plus a tagged commit is a complete
rollback path as long as book JSONs are recalibrated as a batch (not
incrementally interleaved with v1 books).

**Do not** publish v1-calibrated and v2-calibrated videos alternately during the
transition. Cut over cleanly at a known date and record it.

---

## 11. Open decisions

| # | Decision | Owner | Blocking |
|---|---|---|---|
| 1 | Corpus blend and weights (CC vs prose vs news) | Bill | Phase 1 |
| 2 | CEFR source: PCIC extraction vs openly-licensed substitute | Bill | Phase 2 |
| 3 | Licensing check on redistributing a PCIC-derived ordering | Bill | Phase 2 |
| 4 | 3 bands (A1-A2/B1-B2/C1-C2) or 4+ if a finer split is sourceable | Bill | Phase 2 |
| 5 | `RESCUE_THRESHOLD` value (start 500) | Review after first rescue log | Phase 2 |
| 6 | Truncate at 20k vs raise `UNKNOWN_RANK` to ~60k | Bill | Phase 2 |
| 7 | Anchor set for the curve fit — which 12–20 published videos | Bill | Phase 4 |
| 8 | Accept continuity fit, or accept renumbering if the gate fails | Bill | Phase 4 |

---

## 12. Command reference

```powershell
# Build the list (resumable)
python create_frequency_list.py <args>          # see §4.3 re: output path

# Regenerate the level map
cargo run --bin level_analyzer --release

# Re-run the AVD hunter
.\run_avd_hunter.ps1

# Re-fit the AVD -> UL curve
python fit_and_curve.py

# Recalibrate books
.\run_l_level_calibrator.ps1
# or, per book, in the app terminal:
#   calibrate 40

# A/B switch between lists
#   config set custom_frequency_list_path "assets/frequency_lists/es_master_frequency_list_v2.txt"
```

---

## 13. Deferred

**CEFR-as-scoring-override** (clamping ranks at DRC time via the existing
domain-policy hook in `escore.rs::tallies`) is *superseded* by this plan: with
CEFR banding baked into the list itself, a runtime override becomes redundant.
Revisit only if the banded list still mis-scores specific lemmas after Phase 5.
