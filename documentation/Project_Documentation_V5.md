# Project Documentation: WeaveLang - Spanish CI Learning Application - Version 5

**Document Version: 5.0**
**Last Updated:** (Date of this session)

**Note for LLM (Context for Future Sessions):**
This document, `Project_Documentation_V5.md`, is the primary specification for the "WeaveLang" project. It outlines a hybrid data pipeline using both LLMs for creative tasks and SpaCy for deterministic linguistic analysis.

## 1. Project Overview & Goal

*   **Name:** WeaveLang - Spanish CI Learning Application
*   **Goal:** To facilitate Spanish language acquisition for learners, using a Comprehensible Input (CI) methodology. This project focuses on creating robust, pre-processed learning content from literary works.
*   **Methodology:** A hybrid data pipeline is used for pre-processing. LLMs are leveraged for creative translation and simplification, while the **SpaCy NLP library** is used for deterministic, high-quality linguistic tasks like tokenization and lemmatization. The final structured data is then processed by a Rust application which simulates a learner's progress and generates scaffolded audio script files.
*   **Content Source:** Public domain literary texts.
*   **Target Audience:** English speakers learning Spanish.

## 2. Core Learning Methodology & Generation Levels (Rust Application)

The Rust application uses the pre-processed JSON data to generate output text for a learner based on a hypothetical profile. The generation strategy involves these levels, attempted in order:

*   **L0: Full Advanced Spanish (`AdvS`)**
    *   Uses the complete `AdvS` text.
    *   Condition: All lemmas in `AdvSL_Overall` are Known/Active (K/A) in the learner's profile.

*   **L1: Woven Advanced Spanish (`AdvS_Segments` / `SimplerAdvS_Segments`)** (Bridging Level)
    *   Uses `AdvS_Segments` and their corresponding simpler vocabulary versions.
    *   For each segment, uses the `AdvS` version if its lemmas are K/A; otherwise, attempts to use the `SimplerAdvS` version. If neither is viable, this level fails.

*   **L2: Full Simpler Advanced Spanish (`SimplerAdvS_Full`)**
    *   Uses the complete `SimplerAdvS_Full` text.
    *   Condition: All lemmas in its corresponding list are K/A.

*   **L3: Full Simple Spanish (`SimS_L3_Full`)**
    *   Uses the `SimS_L3_Text`, a direct, simple Spanish translation of English.
    *   Condition: All lemmas in `L3_SimSL` are K/A.

*   **L4: Hybrid Woven Simple Spanish / Diglot English** **(Redefined)**
    *   This is the primary transitional level, providing a smooth progression.
    *   It iterates through the phrases of a sentence (`SimS_L3_Segments` and their aligned `Eng_Spans`).
        *   If the Spanish phrase's lemmas are K/A, the **full Spanish phrase** is used.
        *   If the Spanish phrase is not fully known, the system falls back to the **English phrase** and then attempts to perform a **single word-level diglot substitution** within that phrase, using a known word from the `DIGLOT_MAP`.
        *   If no diglot substitution is possible, the plain English phrase is used.
    *   Condition: The resulting sentence must contain at least one Spanish word or phrase.

*   **L5: Simple Diglot (Fallback)**
    *   A minimal fallback level. Uses the original `Eng` text as a base.
    *   Finds the **first possible word** in the entire sentence that can be substituted with a K/A Spanish word from the `DIGLOT_MAP`.
    *   Generates a sentence with only this single substitution.

*   **L6: Full English (`Eng`)**
    *   Uses the original `Eng` (source) text. The ultimate fallback.

## 3. Analysis & Tuning: The Level Distribution Log

To facilitate algorithm tuning, the Rust generator produces an analysis report for each book processed. This report is printed to the console and appended to a persistent log file (`corpus_analysis_log.txt`).

*   **Content:** The log details the percentage of sentences generated at each of the 7 levels (L0-L6) for that book instance.
*   **Purpose:** This data provides quantitative feedback on the learning progression. It is used to:
    *   Tune algorithm parameters (e.g., Comprehensibility Threshold, words activated per block).
    *   Identify content that is too easy or too difficult.
    *   Inform future heuristics, such as the "No Going Back" rule (disabling English fallbacks after a certain proficiency is reached).

## 4. The New Data Pre-processing Pipeline (LLM + SpaCy)

The data generation process is a multi-stage pipeline that strategically combines LLMs and the SpaCy library.

**Key Principle:** Use LLMs for subjective, creative tasks (translation, simplification). Use SpaCy for objective, deterministic linguistic analysis (lemmatization).

**Staged Workflow:**

1.  **Stage 1: English -> Advanced Spanish (LLM)**
    *   **Input:** Raw English text.
    *   **Tool:** LLM (e.g., Claude).
    *   **Output:** `adv_spanish_full.text`.

2.  **Stage 2: Lemmatize Advanced Spanish (SpaCy)**
    *   **Input:** `adv_spanish_full.text`.
    *   **Tool:** SpaCy (`es_core_news_lg` model).
    *   **Output:** `adv_spanish_full.lemmas`.

3.  **Stage 3: Advanced Spanish -> Simpler Spanish Segments (LLM)**
    *   **Input:** `adv_spanish_full.text`.
    *   **Tool:** LLM.
    *   **Output:** `adv_spanish_segments` (containing both `advanced_text` and `simpler_text`).

4.  **Stage 4: Lemmatize Simpler Spanish Segments (SpaCy)**
    *   **Input:** `advanced_text` and `simpler_text` from Stage 3.
    *   **Tool:** SpaCy.
    *   **Output:** `advanced_lemmas` and `simpler_lemmas` for each segment, and the aggregated `simpler_adv_spanish_full` object.

5.  **Stage 5: English -> Simple Spanish Alignments (LLM)**
    *   **Input:** Raw English text.
    *   **Tool:** LLM.
    *   **Output:** `phrase_alignments_l3_to_english` and `simple_spanish_l3_segments`.

6.  **Stage 6: Lemmatize Simple Spanish Segments (SpaCy)**
    *   **Input:** `simple_spanish_text` from Stage 5 alignments.
    *   **Tool:** SpaCy.
    *   **Output:** `simple_spanish_l3_lemmas_per_segment` and the aggregated `simple_spanish_l3_full` object.

7.  **Stage 7: Create Diglot Map (LLM)**
    *   **Input:** Aligned English and Spanish phrases from Stage 5.
    *   **Tool:** LLM.
    *   **Output:** `diglot_map_entries`.

This hybrid approach makes the pipeline more robust, cost-effective, and produces higher-quality, consistent data for the Rust simulation engine. The Python `single_stage_processor.py` script is being refactored to implement this new workflow.