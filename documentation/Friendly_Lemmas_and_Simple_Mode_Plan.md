# Friendly Lemmas, Spanish-Source Mode, and Simple Mode — Plan

Last updated: 2026-04-30

## 1. Motivation

Weavelang lessons that *teach* a Spanish word should never substitute that
word away. Currently the diglot algorithm picks the **highest-rank** (rarest)
lemma in a mapping group as its gate, so when an English source word like
*Golden* maps to Spanish *de oro* the group is gated on the rare lemma
`oro` even when the lesson is centered on the common preposition `de`.
At the lesson's emission level the engine then falls back to English
*Golden*, defeating the lesson.

We also want to:

1. Author lessons whose **source text is Spanish** (not English), so the
   tier dependency graph must be invertible.
2. Add a **Simple Mode** that emits only the two `basic_*` tiers, since
   LLM-authored lessons are already in simple Spanish and the
   advanced/moderate tiers add cost with no pedagogical gain.
3. Surface all of these flags in a **Project Settings / Flags pane** so a
   user does not waste expensive TTS runs on a misconfigured project.

## 2. Concepts

### 2.1 Friendly lemma

A lemma marked by the author (or by an import directive) as *protective*.

Mappings in both directions are **N-to-M**: source words may be merged
into a single source token whose `lemmas` carry the full set, and
entries may also have a multi-word `target_text` whose lemmas are
produced by tokenizing it. The candidate lemma set for shielding is
always `entry.target_lemmas` on a single `MappingEntry` — nothing else
needs to change.

When the lemmatizer finalises `entry.target_lemmas` and **one or more
friendly lemmas appear in that set**:

- Drop **all non-friendly** lemmas from the entry.
- Of the remaining friendly lemmas, keep the **single lemma with the
  lowest frequency rank** (most common word). This is the new sole gate.

Effect: any rank ≥ that gate will accept substitution, so the lesson
target word is never blocked by a rarer co-occurring lemma.

If **no** friendly lemma appears in the candidate set, behaviour is
unchanged from today.

The shielding pass is itself toggleable via a project flag
`friendly_shielding_enabled` (default: on, but visible in the flags
pane).

**Worked examples (both directions are N-to-M):**

- Forward (`basic_base → basic_target`): English `Golden` → Spanish
  `"de oro"`. The forward lemmatizer tokenizes `"de oro"` and assigns
  `target_lemmas = [de, oro]`. With `friendly_lemma=de`, shielding
  collapses the entry to `[de]`.
- Inverse (`basic_target → basic_base`): Spanish words `te` +
  `encontrabas` are merged into a single source token `te encontrabas`
  whose `lemmas = [tu, encontrabas]`; the inverse lemmatizer copies
  these into the entry's `target_lemmas` and the entry's
  `target_text` is the multi-word English `"you find yourself"`. With
  `friendly_lemma=tu`, shielding collapses the entry to `[tu]`.
- Inverse merged-source variant: `de` + `madre` merged into a single
  source token `de madre` (`lemmas=[de, madre]`) mapping to English
  `"mother's"`. With `friendly_lemma=de`, shielding collapses to
  `[de]`. Note this only works when the two Spanish words are merged
  into one source token; if they remain separate they form two
  independent single-lemma entries and the shielding pass is a no-op.
  Authoring lessons should rely on the LLM mapping (or an explicit
  `merge` command) to produce the merged token.

### 2.2 Source language and tier dependency graph

The tier graph today is **two parallel branches** rooted at `base`, not a
single chain. There is an "advanced branch" and a "basic branch":

**Default — English source (today's behaviour):**

```
base(en) ──► advanced_target(es) ──► moderate_target(es)
        └──► basic_base(en)       ──► basic_target(es)
```

- `advanced_target` is produced by translating + segmenting the English
  base.
- `moderate_target` is a simplification of `advanced_target`.
- `basic_base` is a simplified English derived from `base`.
- `basic_target` is the Spanish translation of `basic_base`.

**Spanish-source mode (new):**

```
base(es) ──► advanced_target(es) ──► moderate_target(es)   (no LLM call;
        │                                                   segmentation only)
        └──► basic_target(es)     ──► basic_base(en)
```

- `advanced_target` is now **segmentation-only** of the source
  (no translation prompt).
- `moderate_target` is unchanged: simplification of `advanced_target`.
- `basic_target` is a simplified Spanish derived directly from `base`.
- `basic_base` is a direct Spanish→English translation of
  `basic_target`. Note the direction of the basic-branch edge **flips**
  versus the English-source case.

**Simple mode** (orthogonal — only emits the basic branch):

```
English-source + simple_mode:   base(en) ──► basic_base(en) ──► basic_target(es)
Spanish-source + simple_mode:   base(es) ──► basic_target(es) ──► basic_base(en)
```

In simple mode the advanced branch is not built, not validated, and not
woven; only the basic branch must be DRC-clean.

### 2.3 Simple Mode

A project-level flag, **orthogonal** to source-language direction. When on:

- Only the basic branch is built. The advanced branch
  (`advanced_target`, `moderate_target`) is not produced, not validated
  and not woven.
- `generate_weave` only emits levels backed by `basic_target` /
  `basic_base` (plus any L0 / Lmax sentinels).
- DRC requires **only the basic branch** to be Valid + clean;
  `advanced_target` and `moderate_target` may be Stale / Dirty /
  missing.
- The weave preview, AV manifest, and SF assembly all skip the
  advanced branch.
- Title bar gains a `[simple mode]` suffix.

The internal direction of the basic branch still depends on the source
language (see §2.2): `basic_base → basic_target` for English-source,
`basic_target → basic_base` for Spanish-source.

## 3. Source-text directive syntax

We already silently consume `%%CHAPTER_MARKER%%` lines. We adopt a
single, generic preamble syntax for all source-text metadata:

```
%%META key: value%%
```

Recognized keys (initial set):

| Key                  | Value                                | Effect |
|----------------------|--------------------------------------|--------|
| `source_language`    | ISO code or name (e.g. `es`)         | Sets `project_languages.0`. If it equals the target language, derives `source_is_target=true` and flips the basic-branch direction (§2.2). |
| `target_language`    | ISO code or name (e.g. `es`)         | Sets `project_languages.1`. |
| `simple_mode`        | `on` / `off`                         | Sets `state.simple_mode`. |
| `friendly_lemma`     | `<lemma>` (one per directive)        | Appends to `state.friendly_lemmas`. May appear many times. |
| `friendly_shielding` | `on` / `off`                         | Sets `state.friendly_shielding_enabled`. |
| `book_name`          | string                               | Sets `state.book_name`. |
| `teaching_mode`      | `on` / `off`                         | **Preset.** `on` = `simple_mode=on` + `frontier_enabled=off` (and asserts `friendly_shielding_enabled=on`). `off` is a no-op (does not unset the underlying flags). |
| `lm_entry`           | `bas=N` or `bas=+N` (`, from=Sk` opt) | Appends an embedded-level-map recipe entry. `bas=+N` = relative bump from previous entry. `from=Sk` overrides the implicit "next sentence" anchor. Sets `level_map_embedded=true`. Always level=1, `mod_v=adv=0` (simple-mode only — reject when `simple_mode=off`). |
| `lesson_progression` | `bas_start=N, step=M, per=lesson`    | Sugar: emits one `lm_entry bas=+M` at every `lesson_marker` (and an absolute `bas=N` at the first sentence). |
| `lesson_marker`      | (no value)                           | Sentence-position anchor between lessons; consumed by `lesson_progression`. Ignored if no progression is active. |

Existing `%%CHAPTER_MARKER%%` continues to work unchanged.

Example header for a lesson file:

```
%%META source_language: es%%
%%META target_language: es%%
%%META teaching_mode: on%%
%%META friendly_lemma: de%%
%%META book_name: Spanish_Lessons_2026%%
%%META lesson_progression: bas_start=1, step=1, per=lesson%%
{S1: El.}
{S2: ...}
%%META lesson_marker%%
{S3: La.}
...
```

Or, equivalently, with explicit per-lesson recipes (safer when authoring
in chunks where you can verify each `bas` value by hand):

```
%%META lm_entry: bas=1%%
{S1: ...}
{S2: ...}
%%META lm_entry: bas=2%%
{S3: ...}
%%META lm_entry: bas=3%%
{S4: ...}
```

## 4. Implementation Plan

The work is sequenced in eight phases. Each phase ends in a buildable
state with tests passing.

### Phase A — Data model and persistence

1. Extend `AppState` (`src/app/state.rs`) with:
   ```rust
   #[serde(default)] pub friendly_lemmas: Vec<String>,
   #[serde(default = "default_true")] pub friendly_shielding_enabled: bool,
   #[serde(default)] pub simple_mode: bool,
   #[serde(default)] pub level_map_embedded: bool,
   ```
2. `source_is_target` is **derived**, not persisted — recompute on every
   language change as `project_languages.0 == project_languages.1`.
3. Backwards-compatible defaults so existing `.wvl` files load unchanged.
4. Add `ProjectFlagsSummary` helper used by both terminal `flags`
   command and GUI flags pane. Includes a derived
   `teaching_mode_active: bool` field (true iff
   `simple_mode && !frontier_enabled && friendly_shielding_enabled`)
   for the "Teaching Mode: on / off (custom)" label.

### Phase B — Source-text directive parser

1. Update `src/parsing/source_parser.rs`:
   - Pre-scan lines for `%%META key: value%%` directives.
   - Return a `SourceMeta` struct (languages, simple_mode, frontier_enabled,
     teaching_mode, friendly_lemmas, friendly_shielding, book_name,
     `lm_entries: Vec<EmbeddedLmEntry>`, `lesson_progression:
     Option<LessonProgression>`) alongside `Vec<Sentence>`.
   - Track sentence-position anchors so `lm_entry`/`lesson_marker`
     resolve to the correct `start_sentence_idx` (next sentence after
     the directive, or explicit `from=Sk`).
   - `lesson_progression` expansion: emit one `lm_entry bas=bas_start`
     anchored at the first sentence after the directive, then one
     `lm_entry bas=+step` anchored at every subsequent
     `%%META lesson_marker%%`. Reject if `simple_mode` is not on.
   - `teaching_mode: on` is expanded by the parser into the underlying
     flag set before `SourceMeta` is returned, so downstream code only
     sees the resolved primitives.
   - Continue to ignore `%%CHAPTER_MARKER%%` for sentence flow.
2. Update the `import` flow (`AppCommand::ImportSource` in
   `src/app/engine.rs` — search for `parse_source_file` callsite) to
   apply `SourceMeta` into `AppState`. When `lm_entries` is non-empty,
   build the embedded `LevelMapFile` (single-key `"1"`, `mod_v=adv=0`,
   one `JsonCurriculumMapEntry` per resolved entry) and set
   `state.level_map_embedded = true`.
3. New unit tests in `src/parsing/source_parser.rs` covering: each
   key, multiple `friendly_lemma` directives, unknown-key warning,
   `teaching_mode` preset expansion, `lm_entry` absolute and relative
   forms, `lesson_progression` + `lesson_marker` expansion, and
   rejection of `lm_entry` when `simple_mode` is off.

### Phase C — Friendly-lemma shielding

1. New free function in `src/domain/mapping_logic.rs`:
   ```rust
   fn apply_friendly_shielding(
       lemmas: &mut Vec<String>,
       friendly: &HashSet<String>,
       freq_rank: &dyn Fn(&str) -> Option<u32>,
   );
   ```
   - If no overlap with `friendly` → leave `lemmas` unchanged.
   - Else → replace `lemmas` with `vec![lowest_rank_friendly]`.
2. Hook in `lemmatize_mapping_targets` (`src/app/engine.rs:123`) in
   **both** branches, immediately after `entry.target_lemmas` is
   populated (forward branch: after the per-entry tokenization of
   `target_text`; inverse branch: after the per-entry copy from the
   merged source word's `lemmas`). Skip the call entirely when
   `state.friendly_shielding_enabled` is false.
3. Frequency lookup uses the existing level map / frequency list
   (`assets/frequency_lists/`); add a small accessor on `AppState` if
   one does not already exist.
4. Unit tests covering: no overlap, single overlap, multiple overlap
   picks lowest rank, missing rank treated as `u32::MAX`, shielding-off
   bypass.
5. No changes needed in `core_algo` — the gate is computed from
   `entry.target_lemmas` exactly as today; shielding only narrows that
   set.

### Phase D — Spanish-source dependency graph

1. New module `src/services/tier_graph.rs` (or extend
   `tier_processor.rs`) returning a list of
   `(tier_id, depends_on, prompt_track)` for the project, derived from
   `project_languages` and `source_is_target`. The two cases:

   English-source (default, today):
   ```
   advanced_target  ← base               (translate + segment)
   moderate_target  ← advanced_target    (simplify)
   basic_base       ← base               (simplify, same language)
   basic_target     ← basic_base         (translate)
   ```

   Spanish-source (new, when `source_is_target == true`):
   ```
   advanced_target  ← base               (segmentation only, no LLM translate)
   moderate_target  ← advanced_target    (simplify, unchanged)
   basic_target     ← base               (simplify, same language)
   basic_base       ← basic_target       (translate to base language)
   ```

   Note that the **basic branch reverses direction** in Spanish-source
   mode; the advanced branch only changes its top edge from
   *translate+segment* to *segment-only*.

2. Update prompt selection (`PromptManager`) so:
   - When `source_is_target`, `advanced_target` uses a
     **segmentation-only** prompt (new: `advanced_segment_es.toml`).
     This step still produces a tier output but without an LLM
     translation call — only sentence/segment splitting.
   - When `source_is_target`, `basic_target` uses a **simplify**
     prompt against the source-language base (new:
     `basic_target_simplify_es.toml`), and `basic_base` uses a
     **translate** prompt against `basic_target`
     (new: `basic_base_translate.toml`).
   - The existing English-source prompts are preserved unchanged when
     `source_is_target == false`.
3. Update `lang_for_tier` so:
   - `base` reports source language (which equals target language in
     Spanish-source mode);
   - `advanced_target`, `moderate_target`, `basic_target` always report
     target language;
   - `basic_base` always reports base (English) language.
4. The `base` tier in Spanish-source mode keeps its source text
   verbatim so existing UI (navigator, sentence pane) keeps working.
5. Tests: small Spanish-source fixture project that runs through
   approve_tier on all four tiers and produces a clean DRC; verify
   `basic_target` is built before `basic_base` (dependency reversed).

### Phase E — Simple Mode

1. `generate_weave`: filter the level list to only those backed by
   `basic_*` tiers when `state.simple_mode`. Reuse existing per-level
   tier-source map; no new level numbers.
2. DRC: in `run_drc`, when `simple_mode`, skip rules 1-3 for
   `advanced_target`/`moderate_target`, and skip rule 6 (segment-count
   match) entirely.
3. AV / SF: the manifest validator must reject levels that resolve to
   advanced/moderate tiers when simple_mode is on. Emit a clear error
   listing the offending levels.
4. Title bar: append ` [simple mode]` when on. (Update
   `WeaveLangApp::window_title` or wherever `book_name` flows to the
   eframe title.)
5. Tests: simple_mode on/off matrix for `generate_weave all` and
   `run_drc`.

### Phase F — User interface

1. **Terminal commands** (`src/app/terminal.rs`):
   - `flags` — print the project flags pane (read-only).
   - `set_friendly_lemma <lemma>` / `unset_friendly_lemma <lemma>` /
     `clear_friendly_lemmas`.
   - `set simple_mode on|off`.
   - `set friendly_shielding on|off`.
   - `set teaching_mode on|off` (preset: applies/unapplies the bundle).
   - `set source_language <code>` / `set target_language <code>`
     (already partially exists for languages; ensure wiring updates
     the derived `source_is_target` flag).
2. **GUI** (`src/gui/app.rs`):
   - New **Project Flags** pane (Settings tab or modal dialog).
     Displays:
     - Source language / Target language / `source_is_target` derived label
     - Teaching Mode: on / off (custom)
     - Simple Mode (toggle)
     - Friendly Shielding (toggle)
     - Friendly Lemmas (editable list with add/remove)
     - Frontier Enabled (existing flag, surfaced read-only when
       teaching_mode is on)
     - Book name
     - Level Map source: embedded / calibrated / imported / none
   - Each toggle/edit emits the matching terminal command via the
     existing `execute_terminal_command` path so behavior stays
     consistent.
3. **Title bar** — append `[simple mode]` when on.

### Phase H — Embedded level map + teaching_mode preset

1. **`level_map_embedded` flag** in `AppState` (Phase A) is the
   single source of truth that the loaded level map came from
   `%%META lm_entry%%` directives rather than `calibrate` or
   `import level_map`.
2. **Parser → LevelMapFile** conversion (lives next to `SourceMeta`
   handling in the import flow):
   - One key in `LevelMapFile.levels`: `"1"`.
   - One `JsonCurriculumMapEntry` per resolved `lm_entry`, ordered by
     `start_sentence_idx`.
   - `recipe = VLevelRecipe { bas, mod_v: 0, adv: 0 }`,
     `l_level_recipe = LLevelRecipe::default()`.
   - `level: f32 = 1.0`, `target_avd = actual_avd = 0.0`.
   - `LevelMapMeta`: `book_name` from directive (or filename),
     languages from directives, `schema_version` current,
     `calibration_sentence_count = None`.
3. **`calibrate` guard**: when `state.level_map_embedded == true`,
   `calibrate` returns an error
   `"Cannot calibrate: level map is embedded in source. Use
   strip_level_map first."` Add the error variant; do not silently
   skip.
4. **`strip_level_map` terminal command** (new): clears the loaded
   level map and sets `level_map_embedded = false`. Documented but
   primarily an escape hatch — not required for the lesson workflow.
5. **`teaching_mode` preset** (parser-side, see Phase B):
   - `on` → `simple_mode = true`, `frontier_enabled = false`,
     assert `friendly_shielding_enabled = true` (warn if user
     explicitly set it off later).
   - `off` → no-op (does not unset the underlying flags).
   - The flags pane derives a `teaching_mode_active` label from the
     primitive flag values; toggling any underlying flag flips the
     label to `off (custom)` automatically.
6. **Tests**:
   - `lm_entry` absolute and relative-bump produce identical maps for
     equivalent inputs.
   - `lesson_progression` + N `lesson_marker` directives produce N+1
     entries with the right `bas` values.
   - `calibrate` rejects with the expected error when the embedded
     flag is set.
   - `strip_level_map` clears the flag.
   - `teaching_mode: on` followed by manual `set frontier on` flips
     the derived label to `off (custom)` without erroring.
   - Round-trip: save and reload `.wvl`, embedded flag persists.

### Phase G — Documentation

1. Update `documentation/Project_Documentation_V8.md` (or the next
   version) with the new tier graph, flags pane, and directive syntax.
2. Update `documentation/Data_Flow_Diagrams.md` to include the
   Spanish-source variant.
3. Add a short "Authoring lessons" guide describing how to write a
   lesson file with friendly_lemma + simple_mode preamble.

## 5. Acceptance criteria

- A lesson file with `%%META source_language: es%%`,
  `%%META teaching_mode: on%%`, `%%META friendly_lemma: de%%`, and a
  `%%META lesson_progression: bas_start=1, step=1, per=lesson%%`
  preamble imports cleanly, produces only `basic_target` /
  `basic_base` weaves, never substitutes the word `de` away in any
  sentence containing a `de oro → Golden` (or `de madre → mother's`)
  mapping, and uses an embedded level map that bumps `bas` by 1 at
  each `lesson_marker`.
- `calibrate` refuses to run while the embedded level map is loaded.
- An English-source project with no directives behaves exactly as
  today (regression-clean).
- The flags pane and title bar both reflect `simple_mode` and
  `teaching_mode_active` immediately on toggle.
- DRC failures for advanced/moderate tiers do **not** block weave
  generation when `simple_mode` is on.

## 6. Open questions / gaps to resolve before / during implementation

1. **Frequency rank lookup precondition.** The shielding pass needs a
   single, cheap rank accessor that works whenever the level map is
   loaded (otherwise import order matters). Stash the ranking inside
   the loaded level map and require it as a precondition for
   `friendly_shielding_enabled`; warn (don't fail) if shielding is on
   but no rank source is loaded.
2. **Re-shielding on edits.** When the user runs `edit_target`,
   `merge`, `split`, or any other path that re-runs
   `lemmatize_mapping_targets`, friendly shielding must run again so
   newly added lemmas are filtered. The current design hooks shielding
   inside `lemmatize_mapping_targets` itself, so any caller of that
   function gets shielding for free. Confirm during implementation
   that no other code path writes `entry.target_lemmas` directly
   (one known exception: the `merge` consolidation path in
   `engine.rs` extends `merged_lemmas` from existing entries; either
   route it through the shielding helper or rely on the next
   approve-time re-lemmatization to clean up).
3. **Friendly lemmas across languages.** The list is target-language
   only today, but in Spanish-source mode the same list still applies
   (it gates substitution of the *target* tokens). No change needed,
   but document it.
4. **Hidden `base` tier in Spanish-source mode.** Decide whether to
   leave `base` empty, mirror it from source, or hide it from the
   navigator. Recommendation: mirror source into `base` so existing UI
   keeps working; the dependency edge from `base → adv_target` simply
   becomes the identity (segmentation only).
5. **Case sensitivity / lemma normalization** of friendly_lemma values.
   They must match the lemmatizer's output (lowercased). Validate on
   set and warn on mismatch.
6. **Persisted vs. project-scoped flags.** `simple_mode` and
   `friendly_lemmas` clearly belong in the `.wvl` file. Confirm none of
   them belong in `GlobalSettings` instead.
7. **AV manifest interplay.** When `simple_mode` flips on **after**
   audio for advanced/moderate has been built, do we delete that audio
   or keep it? Recommendation: keep it, but exclude from `sf build` and
   show a warning in `av status`.
8. **Migration of existing `.wvl` files.** All new fields use serde
   defaults; no migration script needed. Add a `wvl_schema_version`
   bump if we want to be explicit.
9. **Multiple friendly lemmas in one mapping group.** The plan keeps
   the **lowest-rank** one. Consider whether to keep *all* friendly
   lemmas (so any of them gates) — current core_algo gates on the
   rarest, so a single survivor is sufficient and simpler. Documented
   choice; revisit if a counterexample emerges.

## 7. Suggested naming

We prefer `simple_mode` over `strict_simple` — it pairs cleanly with the
existing `frontier_enabled`, reads well in the flags pane, and the
title-bar suffix `[simple mode]` is unambiguous.

## 8. Notes on dependency-graph correctness

Earlier drafts of this plan (and some older comments in the codebase)
described the tier graph as a single linear chain
`base → adv → mod → basic_target → basic_base`. That is **not** how
the project is wired. The graph is two parallel branches sharing
`base` as their root: an *advanced* branch (`advanced_target` →
`moderate_target`) and a *basic* branch (`basic_base` ↔ `basic_target`,
direction depending on source language). All implementation work in
this plan must follow the parallel-branch model documented in §2.2 and
Phase D. Update any stale documentation encountered along the way.
