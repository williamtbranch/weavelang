# Simple-Triple Mode — Implementation Plan

Status: **Stages 1–6 complete.** The mode is fully wired and consumed by the
pipeline (toggle, DRC relaxation, recipe override, verbatim passthrough,
frontier bump, and unit tests).

## Goal

Produce exactly **4 YouTube videos per work** from a single project, with
minimal weaving overhead:

| Output    | Approx level | How it is produced                                  |
|-----------|-------------:|-----------------------------------------------------|
| advanced  |        35–37 | emitted **verbatim** (no weave) — `generate_weave a`|
| moderate  |        31–32 | emitted **verbatim** (no weave) — `generate_weave m`|
| basic     |        ~27   | the single woven non-diglot tier (`basic_target`)   |
| diglot    |        ~16   | `basic_target` diglotted with a higher frontier mix |

Only the **basic_target** tier is actually woven. `basic_base` is turned
**off** in this mode. The advanced and moderate tiers are passed through
unchanged (their TTS comes straight from `generate_weave a` / `m`).

## Core spec (verbatim from design discussion)

> The triple-simple only generates weave on the basic_t tier. The advanced
> and moderate tiers do not get used for weaving — they get output directly
> as-is. To get the advanced TTS output we `generate_weave a`; similarly for
> moderate `generate_weave m`. `generate_weave UL10 ...` produces weave using
> strictly the basic_t diglotting only. The upper two tiers never get woven.
> This means for this mode, `core_algo.rs` automatically **fails** the top
> two tiers for every sentence — they won't be used and we drop down to
> basic_t. basic_t **never fails**; we just get the diglotting function
> as-is. The mode turns basic_b OFF.

## Stages

### Stage 1 — Toggle wiring  ✅ DONE
- `AppState.simple_triple: bool` (`#[serde(default)]`, default false).
- `AppCommand::SetSimpleTriple { enabled }`.
- Terminal: `set simple_triple on|off`.
- Engine handler sets the flag.
- GUI Project Flags pane: "Simple-triple mode" checkbox.
- `flags` command / `ProjectFlagsSummary` shows `Simple-triple : on|off`.

### Stage 2 — basic_base OFF in this mode  ✅ DONE
- `Sentence.simple_triple` (`#[serde(skip)]`) + `set_simple_triple`; stamped
  by `AppState::refresh_sentence_modes()`.
- `weave_completeness()`: required tiers in simple_triple are just
  `["base", "basic_target"]`; the forward (basic_base→basic_target) mapping
  is NOT required, but the inverse-diglot mapping IS (needed for the diglot
  output).
- `run_drc` relaxed for simple_triple: skips the advanced/moderate/basic_base
  tier checks (Rules 1–3), skips Rule 4 (forward mapping), and skips Rule 6
  (advanced/moderate segment-count parity).
- `simple_triple` is an **independent** toggle (it does NOT set `simple_mode`),
  so `generate_weave a` / `m` remain permitted.

### Stage 3 — core_algo fails top two tiers  ✅ DONE
- Implemented via **recipe override** rather than threading a bool through
  `generate_book_instance*`: in `execute_generate_weave`, a `triple_levels`
  helper zeroes `mod_v` and `adv` for the numeric-level recipe at all four
  call sites (chapter prepass + gen, full-book prepass + gen).
- With `adv = mod_v = 0`, `try_build_advanced_weave` returns `None` for every
  sentence, so selection drops to `basic_target` (`bas` left untouched).

### Stage 4 — Verbatim passthrough for advanced + moderate  ✅ DONE
- No code change required: `simple_triple` ≠ `simple_mode`, so the existing
  `generate_weave a` / `m` dispatch emits the advanced/moderate tiers verbatim.
  The recipe override only applies to the numeric-level path.
- **Segmentation skipped:** because the advanced/moderate tiers are never
  woven, their segment boundaries are irrelevant. `spawn_llm_job` takes a
  `skip_advanced_segmentation` flag (wired to `simple_triple`); when set, the
  `advanced_target` result is emitted as a single segment and the per-sentence
  segmentation LLM call is skipped entirely. Moderate is derived segment-level
  from advanced, so it becomes single-segment automatically.

### Stage 5 — Diglot frontier bump  ✅ DONE
- Automatic on enable: `SetSimpleTriple` turns `frontier_enabled = true` and,
  if `frontier_target_pct` is still at the 5.0 default, bumps it to 18.0%.
- The operator can still override `frontier_target_pct` manually for tuning.

### Stage 6 — Validation / tests  ✅ DONE
- `core_algo.rs` unit tests: `simple_triple_recipe_drops_to_basic_target`
  (mod_v/adv = 0 → BasicTarget) and `full_recipe_uses_advanced_weave`
  (all MAX → AdvancedWeave). Both pass.

## Notes
- `simple_triple` is an independent toggle: it does **not** imply `simple_mode`.
- End-to-end usage: import → `set simple_triple on` → `generate_weave a`
  (advanced verbatim), `generate_weave m` (moderate verbatim),
  `generate_weave b`/`UL...` (woven basic_target), and the diglot pass which
  uses the bumped frontier mix (~18% by default).
- See `documentation/Prompt_Flow_and_Asset_Layout.md` for prompt/asset layout.
