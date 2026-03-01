# Rust Mapping Architecture

## Overview

The mapping system in Rust is designed to replace the Python-based `llm2books` pipeline. It processes LLM responses to create structured mappings between English source text and Target language translations, while simultaneously updating the token stream to reflect semantic groupings.

## Data Flow

### 1. Parsing (`parse_llm_mapping`)

The LLM returns text in a specific format:
```
S1:
MAPPINGS:
The -> El
black cat -> gato negro
{{Alice}} -> {{Alicia}}
```

The parser extracts this into `Vec<ParsedMapping>`:
- **Struct:** `ParsedMapping { source_text, target_text, is_proper_noun }`
- **Proper Nouns:** `{{...}}` syntax is detected, braces are stripped, and `is_proper_noun` is set to true.
- **Validation:** Headers and `VALIDATION:` sections are skipped.

### 2. Token Fusion (`fuse_tokens_from_groups`)

This is the core logic that aligns the static LLM output with the dynamic Token Stream.

**Goal:** Transform the stream so that multi-word concepts (atoms) become single tokens.
*Example:* `["black", "-", "cat"]` + Group "black cat" -> `["black-cat"]` (One token, ID inherited from "black").

**Algorithm:**
1.  **Normalization:** Both LLM groups and stream tokens are normalized (lowercase, alphanumeric only).
2.  **Lookahead Window:** For each LLM group, the algorithm looks ahead up to 15 tokens in the stream.
3.  **Fuzzy Matching (Levenshtein):**
    - It attempts to fuse 1 token, then 2 tokens, then 3...
    - It calculates the Levenshtein distance between the *group text* and the *fused candidate text*.
    - **Threshold:** It accepts a match if the edit distance is small (<= 2 edits or 20%).
    - This handles:
        - **Punctuation:** "bad-looking" matches "bad looking".
        - **Contractions:** "don't" matches "do n't".
        - **Minor Typos:** "color" matches "colour".
4.  **Fusion:**
    - The identified tokens (Backgrounds + Words) are consumed.
    - A new `Token::Word` is created containing the full original text.
    - **ID Inheritance:** The new token keeps the `WordId` of the *first* word in the sequence.
    - **Lemma Merging:** Lemmas from all fused words are collected and deduped.

### 3. Mapping Creation (`apply_llm_mapping`)

This orchestrator ties everything together:

1.  **Parse:** Get the target phrases from the LLM output.
2.  **Fuse:** Update the `TokenStream` to match the source phrases.
3.  **Validate:** Ensure the number of word tokens in the stream now equals the number of parsed mappings.
4.  **Map:**
    - Iterate through the Stream and Mappings in lockstep.
    - Create `MappingEntry` objects linking `WordId` -> `Target Text`.
    - Create a `TierMapping` container.

### 4. Normalization (`normalization.rs`)

A direct port of the Python `normalize_spanish_lemma` function.
- **Goal:** Ensure lemma strings are consistent across the application (Dictionary vs Mapping).
- **Process:** Lowercase, strip accents, remove punctuation, validate.
- **Usage:** Will be used by the future Lemmatizer Service to sanitize strings returned from the Python Bridge.

## Key Differences from Python

| Feature | Python (`llm2books`) | Rust (`domain/mapping_logic.rs`) |
| :--- | :--- | :--- |
| **Parsing** | Regex-based | Regex-based (Parity reached) |
| **Proper Nouns** | `{{...}}` extraction | `{{...}}` extraction (Parity reached) |
| **Matching** | `rapidfuzz` (Levenshtein) | Custom Levenshtein implementation |
| **Fusion** | `refactor_token_stream` | `fuse_tokens_from_groups` |
| **Validation** | Reconstructs text | Implicit via Stream preservation |
| **Output** | JSON Blocks | `TierMapping` / `TokenStream` Structs |

## Future Work

- **Inverse Mapping:** The current logic supports Forward Mapping. Inverse mapping logic is identical but directions are swapped (Target -> Source).
- **Lemma Generation:** Currently target lemmas are empty; Python uses spaCy. We need to integrate a lemmatizer or rely on the LLM for lemmas in the future.
