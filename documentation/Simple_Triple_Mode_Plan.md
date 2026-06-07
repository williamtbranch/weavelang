# Simple-Triple Mode — Implementation Plan

Status: **Stage 1 (toggle wiring) complete.** Stages 2+ (behavioral
consumption) are staged below and NOT yet implemented.

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

### Stage 2 — basic_base OFF in this mode  ⬜ TODO
- When `simple_triple` is on, treat `basic_base` as disabled everywhere
  `simple_mode` currently gates the basic branch (generation, DRC, weave).
- `generate_weave` must be permitted even though `basic_base` is unpopulated
  (relax the DRC rule that requires the forward/inverse basic_base mapping
  when `simple_triple` is on).
- Decide interaction with existing `simple_mode` (is `simple_triple` a
  superset that implies `simple_mode`, or an independent toggle?).

### Stage 3 — core_algo fails top two tiers  ⬜ TODO
- In `core_algo.rs`, when `simple_triple` is on, force advanced + moderate
  tiers to "fail" for every sentence so selection drops to `basic_target`.
- `basic_target` must never fail in this mode.
- Verify the drop-down path produces basic_target output for all sentences.

### Stage 4 — Verbatim passthrough for advanced + moderate  ⬜ TODO
- `generate_weave a` (advanced) and `generate_weave m` (moderate) emit the
  tier text unchanged (no weaving) for TTS.
- Confirm the level targets (~35–37 advanced, ~31–32 moderate) are produced
  by existing generation, not altered here.

### Stage 5 — Diglot frontier bump  ⬜ TODO
- Default frontier mix is 5%. For the diglot output, bump to **15–20%**
  (user testing required to pick the exact value).
- Target diglot level ~16 ≈ first 10–15 frequency-list words (~2k vocab).
- Decide whether the bump is automatic when `simple_triple` is on, or a
  separate `set frontier_pct` the operator applies for the diglot pass.

### Stage 6 — Validation / tests  ⬜ TODO
- tier_graph / core_algo unit tests for the fail-top-two-tiers behavior.
- End-to-end: import → `set simple_triple on` → generate the 4 outputs.

## Notes
- Toggle is reversible and consumed nowhere yet; setting it on/off has no
  pipeline effect until Stages 2+ land.
- See `documentation/Prompt_Flow_and_Asset_Layout.md` for prompt/asset layout.
