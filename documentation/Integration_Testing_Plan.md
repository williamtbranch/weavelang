# WeaveLang End-to-End Integration Testing Plan

**Document Version:** 1.0  
**Created:** February 2026  
**Status:** Design — Not Yet Implemented

---

## 1. Intent & Motivation

WeaveLang is a complex pipeline that ingests a literary source text, processes it through multiple LLM-driven stages (simplification, translation, phrase mapping), and produces a graded set of ~32 learner-facing books. Today, correctness can only be verified by running the full pipeline with live LLM calls, manually inspecting output, and hoping nothing regressed.

This sub-project introduces **deterministic, end-to-end integration testing** of the entire WeaveLang pipeline — from raw source text import through to final graded book output — without making any real LLM calls. The core insight is:

> WeaveLang already has a **command-driven architecture** whose commands are accessible through a terminal, GUI, or HTTP API. We are one step away from being able to write a batch script that drives the server through its complete workflow. The only missing piece is a **test-mode LLM interceptor** that returns canned responses instead of calling the Anthropic API.

With canned LLM responses, the pipeline becomes **fully deterministic** for a given input + frequency file combination. This means we can:

1. Run the entire pipeline automatically via batch scripts.
2. Compare output against golden (expected) files.
3. Detect regressions instantly.
4. Allow an AI agent to drive the server via the `send` command, observe failures, and iterate on bug fixes autonomously.

This is arguably the most important testing infrastructure we can build, because it validates the **entire system** — not just isolated units.

---

## 2. Architecture Overview

### 2.1 The Test Loop

```
┌──────────────────────────────────────────────────────────┐
│                   Test Harness (script)                   │
│                                                          │
│  1. Start weavelang daemon in --test-mode <test_dir>     │
│  2. Read test batch file (sequence of terminal commands)  │
│  3. Send commands to daemon via CLI `send` or HTTP API   │
│  4. Daemon processes commands using MockLlmProvider      │
│  5. Export final output to test_output/                   │
│  6. Compare test_output/ against expected_output/        │
│  7. Compare debug_dump.txt against expected_dump.txt     │
│  8. Report PASS / FAIL with structured diffs             │
│  9. Shut down daemon                                     │
└──────────────────────────────────────────────────────────┘
```

### 2.2 Key Design Decisions

| Decision | Resolution |
|----------|-----------|
| **How to mock LLM calls** | Dependency Injection + Strategy Pattern: define an `LlmProvider` trait; inject `MockLlmProvider` when `--test-mode` is active |
| **How to key canned responses** | Sentence-label-based dispatch: the mock parses outgoing prompt for `S3`, `S4`, `S5` labels and returns matching entries from a per-stage canned response file |
| **Frequency file** | Use the production Spanish frequency file (`en-es`). No custom test fixture. The file is read-only and deterministic. |
| **Output comparison** | Two-tier: (1) `debug_dump.txt` for internal state comparison (~95% of debugging), (2) final book text files for edge cases where internal state is correct but output diverges |
| **Future language isolation** | If the production frequency file ever needs independent evolution, create an `en-ts` (test-Spanish) language pair in `assets/` with a frozen copy. Not needed now. |

---

## 3. Directory Structure

```
master_integration_tests/
├── run_all_tests.ps1              # Top-level harness: iterates test dirs, runs each, reports
├── compare_outputs.py             # Structured diff tool for golden file comparison
├── README.md                      # Quick-start guide for adding new test cases
│
├── test01/
│   ├── test_batch.txt             # Sequence of weavelang terminal commands
│   ├── source_input.txt           # The raw literary source text (e.g., 10 sentences)
│   ├── llm_responses/
│   │   ├── segment_sentence_universal.txt   # Canned segmentation responses (see §4.4)
│   │   ├── simplify_to_basic_english.txt    # Canned responses keyed by sentence ID
│   │   ├── translate_text_basic.txt         # Canned responses keyed by sentence ID
│   │   ├── translate_text.txt               # Canned responses keyed by sentence ID
│   │   ├── simplify_segments.txt            # Canned responses keyed by sentence ID
│   │   ├── generate_phrase_map.txt          # Canned responses keyed by sentence ID
│   │   └── generate_inverse_phrase_map.txt  # Canned responses keyed by sentence ID
│   ├── expected_output/
│   │   ├── book_01.txt
│   │   ├── book_02.txt
│   │   ├── ...
│   │   └── book_32.txt
│   ├── expected_dump.txt          # Golden debug dump (all sentences × all tiers × word maps)
│   └── test_output/               # Generated at runtime; gitignored except for golden copies
│       ├── book_01.txt
│       ├── ...
│       ├── book_32.txt
│       └── debug_dump.txt
│
├── test02/
│   ├── ...  (same structure)
│
└── ...
```

### 3.1 File Naming Conventions

- **Canned response files** are named after the **prompt name** used by `PromptManager` / `LlmStageService` (e.g., `simplify_to_basic_english.txt`). This is the same string passed to `generate_for_items()` as `prompt_name`, or set via `set_context()` for segmentation. This creates a 1:1 correspondence between the stage requesting an LLM call and the file the mock reads from. The segmentation file (`segment_sentence_universal.txt`) has a unique format documented in §4.4.

- **Source file** is always `source_input.txt` in the test directory root.

- **Test batch file** is always `test_batch.txt`. It contains one terminal command per line, exactly as you would type them in the weavelang REPL or send via the HTTP `/api/v1/terminal` endpoint.

---

## 4. The LLM Interceptor (MockLlmProvider)

### 4.1 The Trait: Dependency Injection + Strategy Pattern

The current `LlmClient` struct calls the Anthropic API directly. We introduce an abstraction layer:

```rust
/// Strategy trait for LLM completion — enables swapping real API calls 
/// with canned test responses (Dependency Injection / Strategy Pattern).
pub trait LlmProvider: Send + Sync {
    fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String>;
}
```

**Production path:** `RealLlmProvider` wraps the existing `LlmClient` (Anthropic API + SHA-256 cache). Zero behavioral change.

**Test path:** `MockLlmProvider` reads canned response files.

The `LlmService` is modified to hold a `Box<dyn LlmProvider>` instead of a concrete `LlmClient`. At server startup:

```rust
// In daemon_start():
if test_mode_path.is_some() {
    let provider = MockLlmProvider::new(test_dir);
    LlmService::from_provider(Box::new(provider))
} else {
    let provider = RealLlmProvider::new(cache_root)?;
    LlmService::from_provider(Box::new(provider))
}
```

The rest of the application — `LlmStageService`, `Engine`, `AppState` — is completely unaware of which provider is active. The main app is none the wiser.

### 4.2 Sentence-Label-Based Dispatch

When `LlmStageService::generate_for_items()` builds a user prompt, it produces text like:

```
STRICT REQUIREMENT: Provide exactly one line for each ID provided below. Do not merge sentences. Do not skip IDs.

S3: The old woman walked slowly through the dark forest.
S4: He could not believe what he was seeing.
S5: The children played happily in the garden.
```

The `MockLlmProvider` handles this as follows:

1. **Identify the stage:** Parse the `system_prompt` to determine which prompt template is in use.  Alternatively, the mock can be given the prompt name as metadata (see §4.3). This identifies which canned response file to read (e.g., `simplify_to_basic_english.txt`).

2. **Parse requested sentence IDs:** Extract `S3`, `S4`, `S5` from the `user_prompt` using the existing ID regex (`^\s*([A-Za-z0-9_:-]+)\s*:`).

3. **Look up responses:** The canned response file contains *all* sentences for that stage:

    ```
    S1: The woman went through the forest.
    S2: The man looked at the sky.
    S3: The old woman went slowly in the dark forest.
    S4: He did not believe what he saw.
    S5: The children played in the garden with joy.
    S6: ...
    ...
    S10: ...
    ```

4. **Return only the requested subset:**

    ```
    S3: The old woman went slowly in the dark forest.
    S4: He did not believe what he saw.
    S5: The children played in the garden with joy.
    ```

This approach is **order-independent** and **subset-safe**: the mock returns exactly the sentences the pipeline asked for, regardless of batch size, batching order, or which sentences were already processed. Adding a new sentence to the test case only requires appending one line to each canned response file.

### 4.2.1 Segment-Level Dispatch for GenerateModerateTarget

The `GenerateModerateTarget` stage (`simplify_segments` prompt) is a special case: it sends **segment-level** items with `Sn_Sm` IDs (e.g., `S5_S1`, `S5_S2`, `S5_S3`) rather than sentence-level `Sn` IDs. This was inherited from the Python pipeline, where explicit per-segment labeling was found to prevent LLM segment-boundary drift — a subtle failure mode where the LLM shifts a few words from one segment's output into the next, producing grammatically valid but structurally incorrect text.

The user prompt for this stage looks like:

```
STRICT REQUIREMENT: ...

S5_S1: cuando Gregor Samsa despertó de sueños intranquilos
S5_S2: se encontró en su cama transformado
S5_S3: en un monstruoso insecto.
S6_S1: Estaba echado sobre su espalda dura,
```

The canned response file (`simplify_segments.txt`) uses the same `Sn_Sm:` format:

```
S1_S1: Capítulo 0
S2_S1: La metamorfosis
...
S5_S1: Una mañana, cuando Gregor Samsa despertó
S5_S2: de sueños malos, se encontró cambiado
S5_S3: en su cama en un insecto horrible.
```

The `MockLlmProvider` dispatches these using the same ID-regex mechanism — the regex `^\s*([A-Za-z0-9_:-]+)\s*:` already matches `S5_S1` as a valid ID.

**Reassembly:** After the LLM returns segment-level results, the `llm_worker` reassembles them into sentence-level results before sending through the channel. Segments are grouped by sentence index, sorted by ordinal, and joined with spaces (using the same separator logic as `reconstruct_and_separate_segments`). The rest of the pipeline — result handling, `apply_llm_result`, tier storage — sees the same sentence-level results as any other stage.

### 4.3 Stage Identification

The `MockLlmProvider` needs to know which canned response file to read for a given `complete()` call. Options:

**Option A — System prompt fingerprinting:** The mock maintains a map of `(system_prompt_hash → prompt_name)` built by loading all prompt templates at startup and hashing them. When `complete()` is called, it hashes the incoming `system_prompt` and looks up the prompt name. This requires no changes to the `LlmProvider` trait but is fragile if prompts change.

**Option B — Extended trait with metadata (recommended):** Add an optional `prompt_name` parameter:

```rust
pub trait LlmProvider: Send + Sync {
    fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String>;

    /// Optional: provide the prompt template name for test dispatch.
    /// Default implementation ignores it (production path).
    fn set_context(&mut self, _prompt_name: &str) {}
}
```

`LlmStageService` calls `provider.set_context("simplify_to_basic_english")` before issuing the batch. The `MockLlmProvider` stores this and uses it to resolve the correct canned file. The `RealLlmProvider` ignores it. This is clean, explicit, and not fragile.

**Option C — Thread-local or scoped context:** Pass the prompt name via a thread-local variable set by `LlmStageService` before calling `complete()`. Avoids trait changes but is less idiomatic.

We recommend **Option B**.

### 4.4 Segmentation: Now Unified with Standard Sn: Format

**Historical note:** Segmentation originally used a different format — the sentence text was embedded in the system prompt via `{{TEXT}}` substitution, the user prompt was a fixed string, and the response had no `Sn:` labels. This would have required special-case dispatch logic in the `MockLlmProvider`.

**Resolution:** As part of this testing initiative, the segmentation prompt (`segment_sentence_universal.txt`) and the Rust code (`llm_segmenter.rs`) were updated to use the **standard `Sn:` label format** shared by all other LLM stages. The system prompt is now a static template (no `{{TEXT}}`), and the user prompt sends `Sn: <text>` just like every other stage.

The response format is:

```
S3:
The old woman walked slowly
through the dark forest
at midnight.
```

The only difference from other stages is that the response is **multi-line per sentence** (one line per segment) rather than a single line. The `MockLlmProvider` handles this by:

1. Identifying the stage via `set_context("segment_sentence_universal")`.
2. Reading `segment_sentence_universal.txt` from the canned response directory.
3. Parsing the `Sn:` header from the user prompt to identify which sentence is being requested.
4. Returning the `Sn:` header followed by segment lines from the canned file.

The canned response file format is identical to what the LLM produces:

```
S1:
The black cat sat
on the mat quietly.

S2:
He ran quickly
through the narrow alley.

S3:
The old woman walked slowly
through the dark forest
at midnight.

...

S10:
The children played happily
in the garden.
```

This eliminates the need for any special-case dispatch logic in the mock. The standard sentence-label-based dispatch (§4.2) handles segmentation the same as every other stage — the mock just needs to return the multi-line block under the matching `Sn:` header rather than a single line.

**Future optimization:** Segmentation is currently called once per sentence. If batching is added later (multiple `Sn:` labels in one call), both the Rust code and the mock will handle it naturally since the format already supports it.

---

## 5. Test Batch File Format

Each `test_batch.txt` contains the exact terminal commands to drive the pipeline, one per line. Blank lines and `#` comments are ignored.

Example `test_batch.txt`:

```bash
# Test 01: Basic 10-sentence pipeline run
# ----------------------------------------

# Import source text
import source source_input.txt

# Generate simplified base tier for all 10 sentences
generate stage GenerateBasicBase 0 9

# Wait for LLM job to complete
watch job

# Generate translations
generate stage GenerateBasicTarget 0 9
watch job

generate stage GenerateAdvancedTarget 0 9
watch job

generate stage GenerateModerateTarget 0 9
watch job

# Generate phrase maps
generate stage GeneratePhraseMap 0 9
watch job

# Export final output
export json test_output/final.json

# Dump debug state
dump debug test_output/debug_dump.txt
```

**Note:** The `watch job` command blocks until the current LLM job completes. The `dump debug` command is a new terminal command that will be added to produce the structured debug output (see §6).

---

## 6. The Debug Dump File

The `dump debug <path>` command produces a comprehensive text file showing all internal state for every sentence. This is the **primary debugging artifact** — expected to be used ~95% of the time when investigating failures.

### 6.1 Format

```
================================================================
=== S1: "The black cat sat on the mat quietly." ===
================================================================

--- L-Adv (Advanced / Base) ---
  Text: "The black cat sat on the mat quietly."
  State: Clean

--- L-Mod (Moderate Target) ---
  Text: "El gato negro se sentó en la alfombra tranquilamente."
  State: Clean

--- L-Bas (Basic Target) ---
  Text: "El gato negro se sentó en la alfombra."
  State: Clean

--- L-Sim (Simple / Basic Base) ---
  Text: "The dark cat sat on the mat."
  State: Clean

--- Forward Phrase Map (Base → Advanced Target) ---
  "The black cat" → "El gato negro"     [cat: freq=1240, black: freq=890]
  "sat on"        → "se sentó en"       [sat: freq=320, on: freq=12]
  "the mat"       → "la alfombra"       [mat: freq=4520]
  "quietly"       → "tranquilamente"    [quietly: freq=2180]

--- Inverse Phrase Map (Advanced Target → Base) ---
  "El gato negro"       → "The black cat"
  "se sentó en"         → "sat on"
  "la alfombra"         → "the mat"
  "tranquilamente"      → "quietly"

================================================================
=== S2: "He ran quickly through the narrow alley." ===
================================================================
...
```

### 6.2 Key Properties

- Every sentence in the document is included, in order.
- All four tiers are shown with their text and editing state.
- Word mappings include frequency indices from the production frequency file, enabling verification that vocabulary grading is working correctly.
- The format is designed to be **diffable**: fixed-width headers, consistent ordering, deterministic content.

### 6.3 Golden File Comparison

`expected_dump.txt` is the golden copy. The test harness runs a structured diff that reports:

```
FAIL: test01 — debug_dump.txt differs from expected

  S3 / L-Sim (Simple / Basic Base):
    Expected: "The old woman went slowly in the dark forest."
    Actual:   "The old woman went in the dark forest slowly."

  S5 / Forward Phrase Map:
    Missing mapping: "happily" → "felizmente" [happily: freq=3200]
```

This tells the developer (or AI agent) exactly *which sentence*, *which tier or mapping*, and *what the discrepancy is* — without wading through a raw text diff.

---

## 7. Final Book Output Comparison

In addition to the debug dump, the harness compares the final generated book files (`test_output/book_NN.txt`) against golden copies (`expected_output/book_NN.txt`). These books are the ultimate deliverable — approximately 32 short books of ~10 sentences each, representing different user levels.

This comparison catches edge cases where internal state is correct but the text generation or assembly logic produces incorrect output. In practice, ~95% of bugs will surface in the debug dump comparison; the book comparison is the safety net for the remaining ~5%.

The harness reports which book(s) diverge and shows a per-sentence diff.

---

## 8. Server Startup in Test Mode

### 8.1 CLI Interface

```
weavelang_cli daemon start --test-mode <path_to_test_dir> [--name <name>] [--port <port>]
```

When `--test-mode` is specified:

1. The daemon initializes `MockLlmProvider` instead of `RealLlmProvider`, pointing it at `<path_to_test_dir>/llm_responses/`.
2. The `ANTHROPIC_API_KEY` environment variable is **not required** (the mock never calls the API).
3. All other initialization proceeds normally: `PythonBridge`, `PromptManager`, `LlmLogger`, `Config` are set up as usual.
4. The server prints `[SERVER] 'test1' listening on http://127.0.0.1:3031 [TEST MODE]` to make the mode visible.

### 8.2 Test Harness Invocation

The top-level harness script (`run_all_tests.ps1`) operates as follows:

```powershell
foreach ($testDir in Get-ChildItem master_integration_tests/test* -Directory) {
    $port = 3030 + $testDir.Name.Substring(4)  # test01 → 3031, test02 → 3032
    $name = $testDir.Name
    
    # 1. Start daemon in test mode (background)
    Start-Process .\target\debug\weavelang_cli.exe `
        -ArgumentList "daemon", "start", "--test-mode", $testDir.FullName, "--name", $name, "--port", $port

    # 2. Wait for server to be ready
    Wait-ForPing -Port $port
    
    # 3. Send commands from test batch file
    foreach ($line in Get-Content "$testDir\test_batch.txt") {
        if ($line -match '^\s*#' -or $line -match '^\s*$') { continue }
        .\target\debug\weavelang_cli.exe send $name $line
    }
    
    # 4. Compare outputs
    python compare_outputs.py $testDir
    
    # 5. Shutdown
    .\target\debug\weavelang_cli.exe daemon kill $name
}
```

---

## 9. Relationship to Existing Architecture

### 9.1 Code Changes Required

| Component | Change | Scope |
|-----------|--------|-------|
| `src/services/llm_client.rs` | Extract `LlmProvider` trait; wrap existing code in `RealLlmProvider`; add `MockLlmProvider` | Medium |
| `src/services/llm_client.rs` → `LlmService` | Change from `Arc<Mutex<LlmClient>>` to `Arc<Mutex<Box<dyn LlmProvider>>>` | Small |
| `src/services/llm_stage.rs` | Call `provider.set_context(prompt_name)` before batch calls | Small |
| `src/services/llm_segmenter.rs` | Call `provider.set_context("segment_sentence_universal")` before the segmentation LLM call | Small |
| `src/cli/main.rs` | Add `--test-mode <path>` flag to `daemon start`; conditionally create `MockLlmProvider` | Small |
| `src/app/terminal.rs` | Add `dump debug <path>` command | Medium |
| `src/app/engine.rs` | Implement `dump_debug()` method that serializes all sentences/tiers/mappings | Medium |
| **New file:** `compare_outputs.py` | Structured diff tool | New |
| **New file:** `run_all_tests.ps1` | Test harness orchestrator | New |

### 9.2 What Does NOT Change

- `Engine::execute()` — command dispatch logic is unchanged.
- `LlmStageService::generate_for_items()` — batching, retry, and parsing logic is unchanged.
- `AppState`, `Sentence`, `Tier`, `Segment`, `TokenStream` — all domain types are unchanged.
- `PromptManager` — prompts are loaded as usual (the mock just ignores the system prompt content).
- `server.rs` — HTTP routing and command handling is unchanged.
- All existing unit tests and weavetest suite — unchanged and still valid.

### 9.3 Interaction with Existing LLM Cache

The production `LlmClient` has a SHA-256 file cache (`.llm_cache/`). In test mode, this cache is **bypassed entirely** because `MockLlmProvider` never calls `RealLlmProvider`. No interference.

---

## 10. AI-Assisted Debugging Workflow

A key benefit of this architecture is that an AI coding agent (e.g., GitHub Copilot) can autonomously debug regressions:

1. **Observe failure:** The agent reads the structured diff output from `compare_outputs.py`.
2. **Reproduce:** The agent starts a daemon in test mode and sends the same commands via `weavelang_cli send`.
3. **Investigate:** The agent queries server state via `GET /api/v1/state` and `GET /api/v1/state/sentence/<idx>` to inspect internal state.
4. **Fix:** The agent modifies Rust source code based on the failure.
5. **Rebuild & re-test:** The agent runs `cargo build`, restarts the daemon in test mode, and re-runs the failing test batch.
6. **Verify:** The agent re-runs `compare_outputs.py` to confirm the fix.

This is possible because:
- The server's HTTP API provides full state visibility.
- The `send` command allows the agent to drive any workflow.
- Test mode makes every run deterministic — the agent can iterate without worrying about LLM non-determinism.
- The entire cycle (build → start → test → compare) can be scripted.

---

## 11. Creating New Test Cases

### 11.1 One-Time Bootstrap Process

To create a new test case (e.g., `test03`):

1. **Write the source text:** Create `test03/source_input.txt` with ~10 carefully chosen sentences that exercise the feature or edge case you want to test.

2. **Generate canned LLM responses:** Run the pipeline once with the *real* LLM against this source text. Capture the LLM responses (the existing `LlmLogger` writes all prompts and responses to log files). Extract and format them into the per-stage canned response files.

3. **Generate golden output:** Run the pipeline in test mode using the canned responses. Verify the output manually. Once satisfied, copy `test_output/` contents to `expected_output/` and `test_output/debug_dump.txt` to `expected_dump.txt`.

4. **Write the test batch:** This is typically the same sequence of commands for most tests (import → generate stages → export → dump). Customize as needed for edge cases.

### 11.2 Updating Golden Files After Intentional Changes

When the pipeline behavior intentionally changes (e.g., a new prompt improves simplification quality), golden files must be updated:

1. Run the affected test(s) in test mode.
2. Manually review the diff to confirm the changes are correct and intentional.
3. Overwrite `expected_output/` and `expected_dump.txt` with the new output.
4. Commit the updated golden files with a clear commit message explaining the change.

**Important:** If you change a prompt template, you may also need to update the canned LLM response files if the stage identification mechanism (§4.3) relies on system prompt matching. With the recommended Option B (explicit `set_context`), this is not an issue — only the content of the canned responses matters, and those are keyed by prompt *name*, not prompt *content*.

---

## 12. Future Extensions

### 12.1 Language Pair Isolation (`en-ts`)

If the production Spanish frequency file or prompts need to evolve independently of tests, we can create a test-only language pair:

- Copy `assets/frequency_lists/en-es/` → `assets/frequency_lists/en-ts/`
- Copy `assets/prompts/en-es/` → `assets/prompts/en-ts/`
- Set `project_languages = ("en", "ts")` in the test batch or test configuration.

This freezes the test assets while allowing production assets to evolve. Not needed until the frequency file or prompts undergo breaking changes.

### 12.2 Selective Stage Testing

The batch file format naturally supports testing individual stages in isolation:

```bash
# Only test phrase mapping — load pre-built state, run one stage, compare
load project test03/pre_mapped_state.wlp
generate stage GeneratePhraseMap 0 9
watch job
dump debug test_output/debug_dump.txt
```

This enables faster, more focused regression tests when working on a specific stage.

### 12.3 Performance Regression Testing

Since test mode eliminates LLM latency, pipeline execution time is dominated by actual computation (tokenization, fusion, segmentation, etc.). The harness could record execution time and flag regressions (e.g., batch processing time increased >20% from baseline).

### 12.4 Studio: Small-Text AVD Estimation

When a content creator uses the Studio on a small text (e.g., a single fairy tale), the per-book calibrator may not have enough statistical mass to produce meaningful AVD curves. A future feature could:

1. Estimate the source text's vocabulary difficulty (e.g., via quick frequency-rank analysis).
2. Match it to a pre-computed reference curve from a canonical book of similar difficulty.
3. Use that proxy curve for the calibration step, producing a reasonable curriculum map without requiring hundreds of simulation runs on a text too small to be statistically valid.

This is not needed for the current testing plan or pipeline, but is worth noting as a Studio UX improvement.

---

## 13. AVD Calibration: Out of Scope (and Why)

The AVD calibration system (`calibrator.rs`, `avd_hunter.rs`, `metrics.rs`, `core_algo.rs`) is **excluded from the integration test plan**. This is a deliberate decision, not an oversight.

### 13.1 Why Not Test It?

The calibrator is entirely LLM-free — it operates on the tier data that the LLM stages have already produced. It is pure computation: given a book's JSON with all tiers generated, it simulates vocabulary exposure at every user level and finds optimal VLevelRecipes.

The fundamental problem is a **chicken-and-egg**: the only way to know what AVD curve to expect for a given test text is to run the calibrator on it, which is the very thing we'd be testing. Unlike the LLM stages (where we control input and expected output via canned responses), the calibrator's correctness is assessed by human judgment — "does the difficulty progression feel right when reading the output books?"

Additionally, small test texts (~5–10 sentences) produce statistically meaningless P85/P95 percentiles, making AVD scores unreliable as test assertions.

### 13.2 How It Was Validated Instead

The calibrator was validated empirically:
- The Metamorphosis calibration JSON was reviewed manually and the AVD curve deemed reasonable.
- User experience testing confirmed that levels felt incrementally harder.
- The system was designed and tuned over several weeks of iterative refinement.

### 13.3 What We Could Do If Needed

If calibration testing becomes necessary in the future, the approach would be:
1. Use a real book's pre-generated JSON (e.g., Metamorphosis) as a large fixture.
2. Run the calibrator on it.
3. Assert structural properties (monotonically increasing AVD, phase transitions occurring in order, no V-level regressions) rather than exact AVD values.
4. This would be a separate test suite from the LLM integration tests.

---

## 14. Summary

| Aspect | Approach |
|--------|----------|
| **What we're testing** | The entire pipeline: import → segmentation → simplification → translation → phrase mapping → book generation |
| **How we achieve determinism** | `MockLlmProvider` returns canned responses keyed by sentence ID and stage name; segmentation uses text-matching dispatch (§4.4) |
| **How we drive the pipeline** | Batch files containing terminal commands, sent via `weavelang_cli send` |
| **How we detect regressions** | Golden file comparison on debug dumps (primary) and final book output (secondary) |
| **How we debug failures** | Structured diffs showing exact sentence/tier/mapping divergence; AI agent can reproduce via HTTP API |
| **Pattern used** | Dependency Injection + Strategy Pattern (`LlmProvider` trait) |
| **Production impact** | Zero — the trait abstraction is a clean refactor; all existing behavior is preserved in `RealLlmProvider` |
| **Frequency file** | Production `en-es` file used as-is; no test fixture needed |
| **AVD calibration** | Out of scope — LLM-free pure computation with chicken-and-egg validation problem; validated empirically instead (§13) |
