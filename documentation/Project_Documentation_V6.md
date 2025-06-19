//*** START FILE: documentation/Project_Documentation_V5.md ***//
# Project Documentation: WeaveLang - Spanish CI Learning Application - Version 5

**Document Version: 5.1 (Orchestrator Complete)**
**Last Updated:** (Date of this session)

**Note for LLM (Context for Future Sessions):**
This document is the primary specification for the WeaveLang project. It describes the completed Python data generation pipeline, which uses a unified orchestrator (`llm2books/orchestrate_pipeline.py`) to manage a 7-stage hybrid LLM/SpaCy workflow. The previous standalone stage scripts are now deprecated.

## 1. Project Overview & Goal

*   **Name:** WeaveLang - Spanish CI Learning Application
*   **Goal:** To facilitate Spanish language acquisition for learners, using a Comprehensible Input (CI) methodology. This project focuses on creating robust, pre-processed learning content from literary works.
*   **Methodology:** A hybrid data pipeline is used for pre-processing. LLMs are leveraged for creative translation and simplification, while the **SpaCy NLP library** is used for deterministic, high-quality linguistic tasks like tokenization and lemmatization. The final structured data is then processed by a Rust application which simulates a learner's progress and generates scaffolded audio script files.

## 2. Core Learning Methodology & Generation Levels (Rust Application)

The Rust application uses the pre-processed JSON data to generate output text for a learner based on a hypothetical profile. The generation strategy has been simplified to a more elegant and powerful hierarchy.

### Deprecation of L5 and L6

The previous learning levels L5 (Simple Diglot) and L6 (Full English) are now **defunct and obsolete**. Their functionality is fully and more effectively encompassed by the new, robust L4 level. This change greatly simplifies the generation logic in the Rust application without any loss of functionality.

### The New Generation Hierarchy

The Rust application should attempt to generate text for a sentence in the following order:

*   **L0: Full Advanced Spanish (`adv_spanish_full`)**
    *   Condition: All lemmas are Known/Active.

*   **L1: Woven Advanced / Simpler Advanced Spanish (`adv_spanish_segments`)**
    *   Condition: All segments can be constructed using either their `advanced_text` or `simpler_text` based on known lemmas.

*   **L2: Full Simpler Advanced Spanish (`simpler_adv_spanish_full`)**
    *   Condition: All lemmas in the aggregated simpler text are Known/Active.

*   **L3: Full Simple Spanish (`simple_spanish_l3_full`)**
    *   Condition: All lemmas in the L3 simple text are Known/Active.

*   **L4: The Woven Hybrid (The Ultimate Fallback)**
    *   This is the primary transitional level and the final fallback.
    *   The algorithm iterates through the `phrase_alignments_l3_to_english` for a sentence.
        *   **If** the Spanish phrase (`simple_spanish_text`) is fully known (by checking its lemmas in `simple_spanish_l3_lemmas_per_segment`), the **full Spanish phrase** is used.
        *   **Else**, the system falls back to the corresponding `english_span_text`. It then attempts to perform **word-level diglot substitutions** within that English phrase, using viable words from the `diglot_map_entries` for that segment.
    *   This single level now covers every remaining possibility, from a mostly Spanish sentence with one English phrase, all the way down to a full English sentence with one or two Spanish words, and finally, a plain English sentence if no substitutions are possible.

## 3. The Unified Data Pre-processing Pipeline (Python Orchestrator)

The data generation process is a multi-stage pipeline managed by `llm2books/orchestrate_pipeline.py`. It strategically combines LLMs and the SpaCy library.

**Key Principle:** Use LLMs for subjective, creative tasks (translation, simplification, contextual mapping). Use SpaCy for objective, deterministic linguistic analysis (lemmatization, segmentation).

**Staged Workflow:**

1.  **Stage 1: English -> Advanced Spanish (LLM)**
    *   **Tool:** LLM (e.g., Claude)
    *   **Input:** `Staged/{book}.txt`
    *   **Output:** Populates `adv_spanish_full.text` in `stage1/{book}.stage1.json`.

2.  **Stage 2: Lemmatize Advanced Spanish (SpaCy)**
    *   **Tool:** SpaCy (`es_core_news_lg`)
    *   **Input:** `stage1/{book}.stage1.json`
    *   **Output:** Populates `adv_spanish_full.lemmas`.

3.  **Stage 3a: Segment Advanced Spanish (SpaCy)**
    *   **Tool:** SpaCy (`es_core_news_lg`)
    *   **Input:** `stage2/{book}.stage2.json`
    *   **Output:** Creates the `adv_spanish_segments` list, populating `segment_id` and `advanced_text`.

4.  **Stage 3b: Simplify Adv. Spanish Segments (LLM)**
    *   **Tool:** LLM
    *   **Input:** The partial `stage3/{book}.stage3.json` from 3a.
    *   **Output:** Populates the `simpler_text` field for each entry in `adv_spanish_segments`.

5.  **Stage 4: Lemmatize Adv. & Simpler Segments (SpaCy)**
    *   **Tool:** SpaCy (`es_core_news_lg`)
    *   **Input:** `stage3/{book}.stage3.json`
    *   **Output:** Populates `advanced_lemmas` and `simpler_lemmas` in `adv_spanish_segments`. Also creates the aggregated `simpler_adv_spanish_full` object.

6.  **Stage 5a: Segment English (SpaCy)**
    *   **Tool:** SpaCy (`en_core_web_lg`)
    *   **Input:** `stage4/{book}.stage4.json`
    *   **Output:** Creates `phrase_alignments_l3_to_english` (populating `english_span_text`) and `simple_spanish_l3_segments`.

7.  **Stage 5b: Translate English Segments (LLM)**
    *   **Tool:** LLM
    *   **Input:** Partial `stage5/{book}.stage5.json` from 5a.
    *   **Output:** Populates `simple_spanish_text` in alignments and `simple_text` in L3 segments.

8.  **Stage 6: Lemmatize Simple Spanish (SpaCy)**
    *   **Tool:** SpaCy (`es_core_news_lg`)
    *   **Input:** `stage5/{book}.stage5.json`
    *   **Output:** Populates `simple_spanish_l3_lemmas_per_segment` and the aggregated `simple_spanish_l3_full` object.

9.  **Stage 7: Create Diglot Map (LLM & SpaCy)**
    *   **Tool:** Hybrid. LLM provides `EngWord -> SpaForm` mapping. The script uses SpaCy to derive the `SpaLemma` from the `SpaForm`.
    *   **Input:** `stage6/{book}.stage6.json`
    *   **Output:** Populates the `diglot_map_entries` list.

## 4. Next Steps for Development

The Python data pipeline is now considered complete and robust. The next logical steps for the project are:
1.  **Run the Rust Corpus Generator:** Use `run_corpus_gen.ps1` to feed the high-quality `stage7` JSON files into the Rust application.
2.  **Refactor Rust Generator:** Update the Rust generation algorithm to use the new, simplified L0-L4 hierarchy, removing the now-redundant L5 and L6 logic.
3.  **Analyze & Evaluate:** Examine the generated TTS text and the `corpus_analysis_log.txt` to tune the learning progression.
//*** END FILE: documentation/Project_Documentation_V5.md ***//