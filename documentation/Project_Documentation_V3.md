# Project Documentation: Comprehensible Input Spanish Learning Application (Static Generation Model) - Version 3

**Document Version: 3.0**
**Last Updated:** (Date of this session)

**Note for LLM (Context for Future Sessions):**
This document, `Project_Documentation_V3.md`, is the primary specification for the "Biweave" project. It may be provided as part of a larger monolithic bundle (`mono.out`) for context. The bundling format uses plain text markers on their own lines: `//*** START FILE: path/to/filename.ext ***//` before file content and `//*** END FILE: path/to/filename.ext ***//` after file content, with a blank line separating file blocks.

## 1. Project Overview & Goal

*   **Name:** weavelang - Spanish CI Learning Application
*   **Goal:** To facilitate Spanish language acquisition for learners starting from zero knowledge, using a Comprehensible Input (CI) methodology based on Stephen Krashen's i+1 hypothesis. This project initially focuses on creating a **prototype for self-testing** by the developer to validate the core mechanics and parameters.
*   **Methodology:** The application provides learners with extensive listening input derived from public domain literary works. Input is carefully scaffolded using a static generation model, starting with mostly Simple English and gradually incorporating Spanish vocabulary and structures at a rate designed to remain comprehensible (targeting ~98% known vocabulary). Learning content is pre-generated in large batches or stages.
*   **Content Source:** Public domain literary texts (e.g., Pride and Prejudice, Oliver Twist, etc.), ordered by calculated difficulty based on readability and vocabulary novelty.
*   **Target Audience:** English speakers learning Spanish (initially the developer for self-testing), aiming to build foundational listening comprehension sufficient to transition to authentic Spanish media (podcasts, movies). The initial 300-hour prototype aims primarily to progress the learner off explicit English language support ("crutches") within the generated content, rather than achieve full proficiency.
*   **Primary Medium:** Audio-based learning (TTS generated), intended for sequential listening of pre-generated audio files (e.g., per chapter/book).
*   **Development Context:** Solo developer, limited hours, leveraging LLM heavily for pre-processing, accepting minor imperfections in prototype output. Development environment is Windows, using Zig for the core application (parser, generator) and Python for helper/API scripts (LLM interaction, TTS interaction, bundling).

## 2. Core Learning Methodology

*   **Comprehensible Input (CI):** Learners acquire language naturally by understanding messages slightly above their current level of competence (i+1).
*   **Scaffolding & Progression Levels:** Content for each sentence is generated *offline* based on the state of a *hypothetical learner profile* representing a stage of acquisition. The generation algorithm checks level viability in this order:
    1.  **Level 1 (AdvS):** Full Advanced Spanish sentence (natural Spanish phrase order). Requires all constituent Spanish lemmas (from the flat `AdvSL` list for the sentence) to be 'Known' or 'Active' in the hypothetical profile.
    2.  **Level 2 (SimS):** Full Simple Spanish sentence (natural Spanish phrase order). Requires all constituent Spanish lemmas (from the phrasal `SimSL` list for the sentence) to be 'Known' or 'Active'.
    3.  **Level 3 (Woven SimS/SimE):** Simple Spanish structure/order, but with `SimS_Segments` phrases containing *any* 'New' lemmas (checked via `SimSL` for that phrase) replaced entirely by their aligned Simple English phrase counterparts (text obtained from `PHRASE_ALIGN`).
    4.  **Level 4 (Diglot SimE/Spa):** Simple English sentence base (`SimE` text) with limited, targeted Spanish word substitutions. Primarily used when the hypothetical learner's vocabulary is very low (`TotalKnownOrActiveLemmas < Threshold_Low`). Substitutes *one* 'Active' (lowest exposure count), `viable:true` Spanish word (`ExactSpaForm` from `DIGLOT_MAP`) per original `SimS_Segments` phrase boundary.
    5.  **Level 5 (SimE):** Full Simple English sentence (`SimE` text). Ultimate fallback.
    *   **No Woven AdvS/SimS Level:** The prototype does not include an intermediate level that mixes AdvS and SimS phrases. The transition is directly from SimS to AdvS when vocabulary allows.
*   **Vocabulary Tracking (Hypothetical Learner Profile):**
    *   Tracks exposure to Spanish lemmas (as defined by LLM during pre-processing, excluding proper nouns).
    *   **States:** New -> Active (1 to T-1 exposures) -> Known (>= T exposures).
    *   **Exposure Threshold (T):** Baseline 20. For high-frequency function words (determined by corpus analysis), T is adaptively calculated based on expected exposures in a "consolidation window" (e.g., 50,000 lemmas / ~5 hours). T value is stored per lemma.
*   **Pacing & Comprehensibility Threshold (CT):**
    *   Content generated in batches (~10,000 Spanish lemmas source / ~1 hour audio target).
    *   `Actual_CT = (% of Known Spanish Lemma instances)` in the final generated batch text (calculated using the generated text and the hypothetical profile state *after* potential new word activation within the batch).
    *   **New Word Introduction Loop:** If `Actual_CT >= Target_CT` (e.g., 98%) after initial batch generation attempt, the batch is "too easy." The highest-frequency 'New' lemma available *in the source sentences for that batch* is marked 'Active' (in a temporary profile state), and the **entire batch text is regenerated**. This loop repeats (activating the next highest frequency available 'New' word) until `Actual_CT < Target_CT` or no more 'New' words are available in the batch source.
*   **"No Going Back" Heuristic (Future Refinement V2):** Consider implementing logic that once a hypothetical profile has processed a significant amount of content purely in Spanish (L1/L2), the system might prioritize outputting L2 (SimS) even if it contains a rare 'New' lemma, rather than reverting to L3 (Woven SimS/SimE). This aims to maintain immersion, potentially allowing the `Actual_CT` for specific sentences/batches to dip slightly below `Target_CT` under this condition.

## 3. Core System Components

*   **Learner Profile Database (Hypothetical):** Stores state ('Known'/'Active'/'New'), current exposure count, and specific `Required_Exposure_Threshold` (T) for every Spanish lemma. Used *during static generation only*. Not used by the end listener.
*   **Content Data Structure Database (JSON):** Stores the pre-processed JSON data for each sentence, outputted by the Helper Tool.
*   **Pre-computation Module:** Calculates book difficulty scores (Readability proxy, Vocabulary Novelty), determines global sentence processing order, calculates adaptive 'T' thresholds. Run once before generation.
*   **Static Generation Engine (Zig):** Core algorithm processing JSON data and hypothetical Learner Profile state to generate final concatenated batch text according to the Adaptive Strategy.
*   **TTS Generation Module (Python Script):** Takes final batch text, calls TTS API (e.g., Google Cloud TTS) to generate audio file.
*   **Helper Tool (Parser/Validator/JSON Encoder - Zig):** Converts LLM-generated intermediate text format to final JSON database structure, performs *structural* validation checks (e.g., delimiters, sections), assigns sequential Sentence IDs based on pre-computed book order. Relies on LLM for semantic correctness.
*   **LLM (External API, e.g., Gemini):** Used *offline* during pre-processing via Python scripts for translation, simplification, segmentation, alignment, lemmatization, and diglot viability flagging.
*   **Bundler/Unbundler Scripts (Python):** `bundler.py` creates `mono.out` from project files using `//*** START/END FILE ***//` markers for providing context to LLM assistant during development. `unbundler.py` processes `mono.in` (LLM assistant output) back into files.

## 4. Data Pre-processing (LLM Role & Intermediate Format)

*   **Goal:** Leverage LLM for complex language tasks upfront. Output a structured, human-readable intermediate text format.
*   **Intermediate Text Format Specification (V2.1 - Final):**
    ```text
    AdvS:: [Advanced Spanish text for the sentence]
    SimS:: [Simple Spanish text for the sentence]
    SimE:: [Simple English text for the sentence]

    SimS_Segments::
    S1([SimS Phrase 1 Text]) // Canonical SimS phrase text
    S2([SimS Phrase 2 Text])
    // ...

    PHRASE_ALIGN:: // Defines semantic equivalents
    S1 ~ [AdvS Span corresponding to S1] ~ [SimE Span corresponding to S1] // AdvS Span kept for completeness/future use
    S2 ~ [AdvS Span corresponding to S2] ~ [SimE Span corresponding to S2]
    // ...

    SimSL:: // Phrasal: Lemmas for SimS segments, no proper nouns, strictly lemmatized
    S1 :: [lemma1 lemma2 ...]
    S2 :: [lemmaA lemmaB ...]
    // ...

    AdvSL:: [flat_list_adv_lemma1 flat_list_adv_lemma2 ...] // Flat list: All AdvS lemmas for sentence, no proper nouns, strictly lemmatized

    DIGLOT_MAP:: // For Level 4 generation
    S1 :: [EngWord1]->[SpaLemma1]([ExactSpaForm1])([ViabilityFlagY/N]) | ... // SpaLemma1 is lemmatized
    S2 :: [EngWordA]->[SpaLemmaA]([ExactSpaFormA])([ViabilityFlagY/N]) | ...
    // ...

    // LOCKED_PHRASE :: [Sx Sy ...] (Optional)

    END_SENTENCE
    ```
*   **LLM Tasking:** Ideally a single LLM call per source sentence generates the full block above. Chained calls might be needed for quality/reliability. Initial prototype data generated by assistant simulation.
*   **Lemmatization Rules (Strict):**
    *   Verbs -> Infinitive.
    *   Nouns/Adjectives -> Singular, Masculine (where applicable).
    *   Clitic pronouns (`me`, `te`, `se`, `lo`, etc.) -> Kept as their own lemma form in `SimSL`/`AdvSL`.
    *   Proper Nouns (Names, Places) -> Excluded from `SimSL` and `AdvSL`. Included in `DIGLOT_MAP` only with `ViabilityFlag(N)`.
*   **Text Normalization:** Source text pre-cleaned or LLM instructed to handle/remove artifacts like `_emphasis_`, `[illustration]`.

## 5. Content Sequencing & Difficulty Ordering

*   **Rationale:** Process easier content first for smoother progression.
*   **Pre-computation:** Entire corpus analyzed once:
    1.  Calculate **Readability Score Proxy** (ASL + AWL) for each book's original text.
    2.  Calculate **Vocabulary Novelty** (% low-frequency global lemmas) for each book's original text.
*   **Sorting:** Books ordered: 1st by Readability (easier first), 2nd by Novelty (more common words first).
*   **Global Sentence IDs:** Assigned sequentially across the entire corpus based on this final book order.

## 6. Static Generation Algorithm (V1 Prototype - Linear Corpus)

*   **Batch Processing:** Processes sentences sequentially by `sentenceId` in batches (~10k SimS lemmas source).
*   **Hypothetical Profile:** Maintains state for one hypothetical learner across batches.
*   **Adaptive Strategy (Per Sentence):** Determines output level (L1-L5) based on profile state and rules in Section 2.
*   **Batch Accumulation & CT Refinement:** Collects sentence text, calculates `Actual_CT`, regenerates batch if needed by activating new words (as per Section 2).
*   **Finalize Batch & Profile Update:** Stores final batch text, triggers TTS script, updates hypothetical profile for the next batch.

## 7. Vocabulary Saturation & End Game Handling

*   **Context:** True saturation unlikely within 300h prototype. Focus is progression L5->L1.
*   **Detection:** System flags if N consecutive batches generated >= `Target_CT` with no new words introduced.
*   **Prototype V1 Action:** Log warning, recommend external materials.
*   **Future Optimization:** "Scan Ahead & Jump" logic.

## 8. Output Generation (TTS & Final Format)

*   **Process:** Static Generation Engine outputs one large concatenated text file per batch.
*   **TTS:** Python script calls TTS API (testing Google Cloud TTS) using the batch text file to generate one audio file (MP3) per batch/chunk (e.g., chapter).
*   **Voice Strategy:** Using a single high-quality Spanish voice from Google Cloud TTS that handles embedded English text with an acceptable Spanish accent.

## 9. Data Structures (Final JSON & Learner Profile)

*   **Final JSON:** Generated by Zig Helper Tool from intermediate format. Contains all necessary fields derived from the intermediate format (text levels, alignments, lemmas, etc.). Structure needs formal definition but maps from intermediate format.
*   **Learner Profile (Hypothetical):** Data structure (e.g., hash map or database) mapping `lemma_string` -> `{ state, exposure_count, required_threshold }`.

## 10. Development Workspace & Workflow

*   **Directory Structure:** Standard project layout (`src`, `scripts`, `corpus_source`, `stage` [LLM intermediate output], `data` [JSON output], `output_audio`, `profiles`, `config`, `docs`, `tests`). `stage`, `data`, `output_audio`, `mono.in`, `mono.out` ignored by bundler.
*   **Monolithic Bundling (`bundler.py`/`unbundler.py`):** Uses `//*** START/END FILE: path ***//` markers for providing code context to LLM assistant during Zig development.
*   **Environment:** Windows / VSCode / Zig / Python. WSL2 as a potential future step if native Windows proves problematic.

## 11. Future Exploration: Internally Leveled Books & Universal Rating (Post V1 Prototype)

*   **Concept:** Offer user choice by providing multiple books at distinct, universally defined difficulty levels.
*   **Two-Stage Generation:**
    1.  **Stage 1 (Data Collection - No TTS):** "Un-roll" each book independently, saving hypothetical profile states at each pass/quantum until book-specific saturation.
    2.  **Stage 2 (Define Golden Profiles & Generate Leveled Content - Selective TTS):** Analyze/cluster Stage 1 profiles to define K "Golden Universal Profiles". Selectively re-generate specific books entirely using each Golden Profile and apply TTS.
*   **Benefit:** Enables learner choice within standardized levels, supporting different learning paces and interests. Reserved for future exploration.

## 12. Key Design Decisions & Known Limitations (Summary)

*   **Static Generation:** Simplifies prototype, predictable output, batch TTS.
*   **LLM Pre-processing:** Leverages LLM for language tasks; Helper Tool for structure.
*   **Intermediate Format:** Human-readable bridge, defines explicit structure.
*   **SimS-centric Segmentation:** Base for alignment and Woven SimS/SimE.
*   **Flat `AdvSL`:** Sufficient for L1 check in current model.
*   **Strict Lemmatization:** Ensures consistent vocabulary tracking.
*   **Content Pre-ordering:** Smoother difficulty curve.
*   **Batch CT Loop:** Controlled vocabulary introduction.
*   **Single Voice TTS:** Simplifies TTS, potentially natural code-switch model.
*   **Limitations:** Reliance on LLM quality/consistency, potential minor lemma inconsistencies from LLM, basic saturation handling in V1.

Content Project Directory:
Your path: E:\Bill\Documents\development\audiolingual\
Contains:
audio\ (corresponds to our conceptual output_audio/)
data\ (corresponds to our conceptual data/ - for JSON)
source\ (corresponds to our conceptual corpus_source/)
stage\ (corresponds to our conceptual stage/ - for LLM intermediate format)
This is where you'll cd into when you want to run WeaveLang on the "audiolingual" content.
Tool Development Environment (Git Repo Root):
Your path: E:\Bill\development\weavelang\
This is where src/, build.zig, scripts/, docs/ etc. for the WeaveLang tool itself will live.
You'll run zig build install --prefix C:\Tools\weavelang from here.
Tool Installation Directory:
Your path: C:\Tools\weavelang\
This will contain bin\weavelang.exe after the install step.