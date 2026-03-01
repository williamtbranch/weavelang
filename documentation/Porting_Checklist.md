# WeaveLang Porting Checklist: Pipeline to Studio

This checklist tracks the migration of features from the Python `llm2books` pipeline into the Rust `weavelang_studio` GUI application. The goal is to retire the batch-process pipeline in favor of interactive, on-demand tools within the Studio.

## Phase 1: Foundation & Linguistic Primitives (Completed)
- [x] **Enhance Python Bridge:** Support segmentation via Stanza/Spacy in `src/services/python_bridge.rs`.
- [x] **Port Text Cleaning & Regex:** Port `clean_italics_and_underscores` and chapter regexes to `src/domain/normalization.rs`.
- [x] **Port Token Stream Fusion:** Port `pre_fuse_word_tokens` logic to `src/domain/token_stream.rs`.

## Phase 2: Feature Parity Services (The "Engine" Room)
We need to make sure the Rust backend can perform the specific linguistic and LLM tasks that the Python scripts used to do.

- [x] **Port "Raw to Sentence" Logic (Book Import):**
    - Implement a `BookImporter` service in Rust. (Completed in `src/services/importer.rs`)
    - **Inputs:** Raw text file.
    - **Logic:** Apply cleaning regexes, detect chapters, segment text (using `PythonBridge`), and produce the initial `Tier 0` (English Source) structure in memory.
    - **Goal:** User clicks "Import Book", selects a text file, and the GUI populates with sentences. (Gutenberg cleaning logic fixed and verified with tests).

- [ ] **Port "Simplification" Logic (Stage 1 Capability):**
    - Implement a service that takes a range of Source Sentences.
    - **Logic:** Construct the "Simplification" prompt (from `llm_prompts.py`), call LLM via `LlmClient`, parse JSON response into `Tier 1` (Basic English).
    - **Goal:** User selects 10 sentences, clicks "Generate Simplified", and Tier 1 fills in.

- [ ] **Port "Translation" Logic (Stage 2 Capability):**
    - Implement a service to take `Tier 1` sentences.
    - **Logic:** Batch sentences (to save tokens/time), prompt LLM for Spanish translations, parse output into `Tier 2` (Basic Spanish).
    - **Goal:** User selects Simplified sentences, clicks "Translate", and Tier 2 fills in.

- [ ] **Port "Phrase Mapping" Logic (Stage 2/3 Capability):**
    - Implement logic to ask LLM for word-level alignments.
    - **Logic:** Align `Tier 0` <-> `Tier 1` and `Tier 1` <-> `Tier 2`. Corresponds to `GeneratePhraseMap`.
    - **Goal:** User views a sentence detail, clicks "Align", and the app draws connection lines between words.

## Phase 3: Infrastructure & Polish
- [ ] **LLM Caching:**
    - Implement a caching layer in `LlmClient` (file-based or SQLite) so re-running generation for the exact same input returns instantly (matching `PoolManager` behavior).

- [ ] **Prompt Management:**
    - Move prompt templates from `llm2books/llm_prompts.py` into a Rust-friendly format (e.g., `assets/prompts/` or `lazy_static` map) for easy editing.

- [ ] **Token Counting:**
    - Implement or integrate a tokenizer (e.g., `tiktoken-rs`) to estimate costs and manage context windows.

## Phase 4: Verification
- [ ] **Unit Tests for Prompts & Parsing:**
    - Write Rust tests verifying that we build the exact same prompts as the Python code for a given input.
    - Verify we can parse the LLM's expected output format correctly.
