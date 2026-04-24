Context Diffusion Frontier Filter
================================

Goal
----
Improve learning efficiency by exposing a controlled fraction of out-of-level target-language words at all times, so upcoming vocabulary is partially familiar before it becomes fully in-level.

This feature is called the context diffusion frontier filter.

Core Product Behavior
---------------------
1. Feature flag on weave output enables frontier exposure.
2. Frontier target is an absolute fraction of total text tokens, not a fraction of unknown tokens.
3. Initial default frontier target is 5% (later tunable, likely 3-5%).
4. Determinism is required:
	- Same inputs and level must produce identical output.
	- Different levels should naturally produce different frontier distributions.

Example:
- If in-level expression is 30%, unknown is 70%.
- A 5% absolute frontier target means approximately 5/70 of unknown tokens may be frontier-passed.
- If in-level reaches 95%, the remaining 5% can be fully frontier-passed, yielding near-100% target language output.

Lemma Semantics and Sentence Consistency
----------------------------------------
Definition:
- Word means lemma.

Rule:
- Frontier decisions are sentence-scoped on unknown lemmas, not token-by-token during streaming.

Process per sentence:
1. Collect unknown lemma set for the sentence.
2. Compute sentence-level pass lemma subset.
3. During rendering, known check becomes:
	- known_by_level OR known_by_sentence_frontier.

Consistency requirement:
- A lemma that frontier-passes in a sentence must be rendered consistently in that sentence.
- Never mix both expression modes for the same lemma in one sentence (for example, avoid showing both "horse" and "caballo" in conflicting modes in the same sentence).

Deterministic Frontier Engine
-----------------------------
Use a deterministic RNG stream from Rust rand crate with seed composition, not custom reseed logic.

Config:
- frontier_seed in .wvl (default 777).

Deterministic seed mix should include:
- frontier_seed
- file/stem identity
- level-boundary identity (or segment index)
- optional stable run-mode salts

No custom FIFO seed mixing is needed.

Deck Model
----------
The deck model is retained to keep distribution spread and predictable rates.

Definitions:
- expected_unknown_pct_int: rounded integer percent of unknown tokens for current level boundary.
- frontier_target_pct: absolute target over total tokens (default 5).

Deck sizing:
- base_deck = 3 * expected_unknown_pct_int
- deck_size = max(300, base_deck)

Pass sizing (unknown-domain pass rate inferred from absolute target):
- desired_unknown_pass_rate = min(1.0, frontier_target_pct / expected_unknown_pct)
- pass_count = round(deck_size * desired_unknown_pass_rate)
- clamp pass_count to [0, deck_size]

Consumption:
- As unknown opportunities are evaluated, consume deck slots.
- When empty, reshuffle using next values from same deterministic RNG stream.

Steering Toward Absolute 5% (Consume-Ahead)
-------------------------------------------
Because selection happens on lemmas but target is token-based, raw lemma-uniform picks can drift.

Steering rule:
- For each sentence, estimate unknown token mass represented by each unknown lemma (token count in sentence).
- Prefer pass decisions that better match running global frontier token budget.

Budget concept per level boundary:
- target_frontier_tokens = round(total_tokens_in_boundary * frontier_target_pct)
- Track emitted_frontier_tokens while weaving.
- Use a bounded steering bias (not hard forcing) so distribution stays natural but converges toward target.

Pre-Pass Calibration (Per Level Boundary)
-----------------------------------------
Must be calibrated per level-map boundary, not globally.

Reason:
- Global unknown rate distorts low-level sections in multi-level works.

Decision:
- Use full boundary pre-pass (not 100-sentence sample) since runtime is acceptable and behavior is cleaner.

Per boundary pre-pass outputs:
- total_tokens
- unknown_tokens
- expected_unknown_pct
- optional unknown lemma histogram (for diagnostics)

Scope and Reset Rules
---------------------
Reset frontier state:
1. Per weave_out target file/stem.
2. At each level-map boundary transition within a file.

This ensures each boundary uses its own calibrated expected_unknown_pct and deterministic stream slice.

Level Shift Rule (Difficulty Alignment)
---------------------------------------
When frontier mode is enabled:
- Execute requested level L using level-map recipe from L-1.

Edge case:
- For requested level 1, use baseline "all unknown" assumption with recipe 0,0,0.

This compensates for added exposure difficulty and keeps practical level feel aligned.

Testing Strategy
----------------
Because this is statistical, rely on integration/acceptance tests plus diagnostics, not only unit tests.

Primary A/B test:
1. Generate reference output with frontier OFF at level L.
2. Generate comparison output with frontier ON at level L+1 (level shift alignment active).
3. Compare realized target-language token percentage.
4. Expected delta is approximately +5 percentage points (within tolerance band).

Dogfooding stress test mode:
- Add a test profile that starts from near-zero known set.
- Exclude first N familiar lemmas (for example N=100) from frontier eligibility to avoid polluted difficulty signal.
- Generate TTS and manually rate comprehensibility.

Recommended diagnostics to emit in analysis output:
- requested_frontier_pct
- realized_frontier_pct
- expected_unknown_pct (per boundary)
- total_tokens / unknown_tokens / frontier_tokens (per boundary)
- deck_size / pass_count
- steering adjustments applied (count)

Open Implementation Notes
-------------------------
1. Keep all logic deterministic and reproducible.
2. Preserve sentence-level lemma consistency as a hard invariant.
3. Keep steering bounded to prevent unnatural clustering.
4. If boundary token count is very small, log low-sample warning for interpretation.
