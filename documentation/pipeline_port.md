# Pipeline Port Checklist

Tracking the porting of active Python pipeline code to Rust.

---

## Active Pipeline Source Files (24 files)

### Core (`llm2books/`)

| Ported | File | Purpose |
|--------|------|---------|
| [ ] | `__init__.py` | Package init |
| [ ] | `__main__.py` | CLI entry point → `orchestrate_pipeline.main()` |
| [ ] | `orchestrate_pipeline.py` | V11 pipeline orchestrator with human-in-the-loop state machine (START → ENGLISH_APPROVED → MAPPINGS_APPROVED → COMPLETE) |
| [x] | `helper.py` | Core utilities: SpaCy tokenization (`create_golden_token_stream`), NLP component fusion, lemma normalization, LLM client init → `token_stream.rs` (from_raw_spacy, fuse_word_tokens, fuse_across_background, from_raw_spacy_unfused) + `normalization.rs` (normalize_spanish_lemma, preprocess_for_spacy) + `llm_client.rs` |
| [x] | `llm_logger.py` | Logs LLM prompts, responses, usage stats, and validation failures to per-job log files → `src/services/llm_logger.rs` |
| [x] | `llm_utils.py` | LLM batch execution engine with retry/fallback, temperature escalation, Gemini caching, response parsing → parsing logic in `mapping_logic.rs` (`parse_structured_llm_response`, `parse_singleline_llm_response`, `validate_parsed_response`, 10 tests). Orchestration (`run_llm_batch_job`, `create_gemini_cache`) is infra — Rust has `LlmClient`/`LlmService`. |
| [x] | `llm_prompts.py` | System prompt loader with language-pair-specific → default fallback from `assets/prompts/` → `src/services/prompt_manager.rs` |
| [x] | `llm_overrides.py` | Parses `%%MANUAL_FIX%%` blocks from LLM log files for human corrections → `src/services/llm_overrides.rs` (`load_manual_overrides`, `parse_fix_block`, 9 tests) |
| [ ] | `pool_manager.py` | Manages Common Pool: generates `.std.json` files, handles translation, segmentation, and simplification of source texts |
| [x] | `stanza_segmenter.py` | LLM-based sentence segmenter with rule-based short-segment merging → `src/services/llm_segmenter.rs` + `tier_processor.rs` |
| [x] | `validator.py` | Data integrity: BWBWB invariant, lossless text reconstruction, diglot index sequencing, exhaustive mapping coverage → `src/domain/validation.rs` |
| [x] | `standardize.py` | Segment boundary alignment, smart token boundary editing, text reconstruction → `src/domain/standardize.rs` |
| [x] | `phrase_mapper_helpers.py` | Token stream refactoring, edit-distance alignment, proper noun parsing, atom sanitization → `src/domain/mapping_logic.rs` (`fuse_tokens_from_groups`, `apply_llm_mapping`, `parse_llm_mapping` with proper noun detection, 14 tests). `sanitize_atoms` is dead code (zero production callers). |
| [ ] | `conftest.py` | Pytest session-scoped fixtures for SpaCy English/Spanish model loading |

### Stages (`llm2books/stages/`)

| Ported | File | Stage | Purpose |
|--------|------|-------|---------|
| [ ] | `__init__.py` | — | Stage registry; imports the 9 V11 stages |
| [ ] | `base.py` | — | Abstract base classes: `Stage`, `SpaCyStage`, `LLMStage` (with resume, caching, override support) |
| [ ] | `assemble_tiers.py` | 1 | Assembles `.std.json` pool files into unified pipeline JSON with all tiers |
| [ ] | `generate_basic_base.py` | 2 | Simplifies literary base text to "basic English" via LLM |
| [ ] | `translate_basic_target.py` | 3 | Translates approved basic-base text to target language via LLM, tokenizes/lemmatizes |
| [ ] | `generate_phrase_map.py` | 4 | Generates forward diglot phrase map (base→target) via LLM |
| [ ] | `generate_inverse_diglot_map.py` | 5 | Generates inverse diglot phrase map (target→base) via LLM |
| [ ] | `apply_phrase_mappings.py` | 6 | Validates human-approved forward map, refactors tokens, builds diglot map |
| [ ] | `apply_inverse_phrase_mappings.py` | 7 | Validates human-approved inverse map, refactors tokens, builds inverse diglot map |
| [ ] | `finalize_mappings.py` | 8 | Lemmatizes target-language words in both diglot maps |
| [ ] | `finalize_book.py` | 9 | Strips intermediate data, cleans tiers, copies final JSON to library |

---

## Active Test Files (17 files)

| Ported | File | What It Tests |
|--------|------|---------------|
| [ ] | `tests/__init__.py` | Package init |
| [x] | `tests/test_helper.py` | `helper.create_v2_token_list` → `unfused_tests` in `token_stream.rs` (5 tests), `normalize_spanish_lemma` → `normalization.rs` (6 tests) |
| [x] | `tests/test_llm_utils.py` | `llm_utils._parse_structured_llm_response`, `validate_parsed_llm_response` → `mapping_logic::llm_response_tests` (10 tests) |
| [ ] | `tests/test_orchestrator_logic.py` | `orchestrate_pipeline.build_language_config` |
| [x] | `tests/test_phrase_mapper.py` | `phrase_mapper_helpers.sanitize_atoms` — dead code (zero production callers); skipped |
| [x] | `tests/test_phrase_mapper_alignment.py` | `phrase_mapper_helpers.align_and_parse_to_atoms` → `mapping_logic::apply_llm_mapping` + `parse_llm_mapping` tests |
| [x] | `tests/test_phrase_mapper_helpers.py` | `phrase_mapper_helpers.refactor_token_stream` → `mapping_logic::fuse_tokens_from_groups` tests (fuzzy match, internal punct, "to eat" fusion) |
| [x] | `tests/test_pre_fusion_pass.py` | `helper.pre_fuse_word_tokens` → `fusion_tests` in `token_stream.rs` (6 tests) |
| [x] | `tests/test_segmentation.py` | `stanza_segmenter.EnglishStanzaProcessor` → `src/services/llm_segmenter.rs` (8 tests) + `tier_processor.rs` (3 tests) |
| [x] | `tests/test_smart_matcher.py` | `standardize.smart_match_and_edit` → `src/domain/standardize.rs` (10 tests) |
| [ ] | `tests/test_stage1_assemble_tiers.py` | `stages.AssembleTiers` V11 stage |
| [ ] | `tests/test_stage_6_apply_phrase_mappings.py` | `stages.ApplyPhraseMappings` (diglot re-indexing after token fusion) |
| [x] | `tests/test_standardize.py` | `standardize.reconstruct_and_separate_segments` → `src/domain/standardize.rs` (4 tests) |
| [x] | `tests/test_tokenization.py` | `helper.create_golden_token_stream` — SpaCy-integration; core logic covered by `token_stream::tests` (possessive fusion, em-dash, hyphen, B/W invariant) |
| [x] | `tests/test_tokenizer.py` | `helper.create_golden_token_stream` — SpaCy-integration; core logic covered by `token_stream::tests` |
| [x] | `tests/test_validator.py` | `validator.*` — reconstruction, BWBWB, diglot indices, exhaustive mapping coverage → `src/domain/validation.rs` (21 tests) |
| [ ] | `tests/test_final_schema.py` | `validator.validate_precomputed_word_counts` — **deferred**: needs word-count fields on MappingEntry |

---

## Import Dependency Graph

```
__main__.py
  └── orchestrate_pipeline.py
        ├── helper.py
        ├── llm_logger.py
        ├── pool_manager.py
        │     ├── helper, llm_prompts, llm_utils, standardize
        │     └── llm_utils → llm_logger, llm_overrides → llm_logger
        ├── stanza_segmenter.py → llm_prompts, helper, llm_logger
        └── stages/* → base.py → (validator, llm_utils, llm_overrides, llm_logger)
              └── individual stages → phrase_mapper_helpers → standardize, helper, validator
```

---

## Stale / Unused Files (not part of active pipeline — do not port)

**Source (3):** `xxhelper.py`, `stage1.py` (legacy V1), `semantic_validator.py` (not imported by V11)

**Stages (14):** `xxfinalize_base_tier.py`, `xxfinalize_simpler_adv_target.py`, `process_target_tiers.py`, `translate_basic_base.py` (duplicate), `finalize_diglot_map.py`, `finalize_simpler_target.py`, `lemmatize_advanced_target.py`, `lemmatize_diglot_map.py`, `lemmatize_inverse_diglot_map.py`, `lemmatize_simple_target.py`, `segment_advanced_target.py`, `segment_base.py`, `simplify_advanced_target.py`, `generate_diglot_map.py`

**Tests (5):** `xxtest_component_fuser.py`, `xxtest_data_reconstruction.py`, `xxtest_phrase_mapper_helpers.py`, `xxtest_token_fuser.py`, `test_stage_2_process_target_tiers.py` (tests stale stage — should be xx-prefixed)

---

## Notes

- Python code is kept indefinitely as the proven reference until full book production is verified with the Rust implementation.
- `standardize.py` ported to `src/domain/standardize.rs` (3 functions, 20 tests).
- `validator.py` ported to `src/domain/validation.rs` (5 validators + 2 convenience functions, 21 tests). `validate_precomputed_word_counts` deferred until MappingEntry gains word-count fields.
- `stanza_segmenter.py` core logic has already been ported to `src/services/llm_segmenter.rs` and `src/services/tier_processor.rs`.
- `llm_prompts.py` functionality is already covered by `src/services/prompt_manager.rs`.
- `llm_logger.py` functionality is already covered by `src/services/llm_logger.rs`.
