# stage2llm_async.py

import google.generativeai as genai
import os
from pathlib import Path
import re
import time # Still used for synchronous delays
import argparse
import sys
from typing import Optional, List, Dict, Any, Tuple
from dotenv import load_dotenv
import asyncio

# --- Import from the validator module ---
try:
    # Assuming your validator provides these lists, if not, define them or import appropriately
    from llm_output_validator import validate_llm_block #, REQUIRED_SECTION_MARKERS, OPTIONAL_SECTION_MARKERS
    # ALL_POSSIBLE_MARKERS_FOR_VALIDATOR = REQUIRED_SECTION_MARKERS + OPTIONAL_SECTION_MARKERS
except ImportError:
    print("ERROR: Could not import 'validate_llm_block' from 'llm_output_validator.py'.")
    print("Please ensure 'llm_output_validator.py' is in the same directory or accessible in PYTHONPATH.")
    sys.exit(1)

# --- Configuration ---
MODEL_NAME = "models/gemini-2.5-pro-preview-05-06" # Verified from your list_models output
#MODEL_NAME = "models/gemini-2.0-flash-lite" # Verified from your list_models output
#MODEL_NAME = "gemini-2.0-flash" # Verified from your list_models output
DEFAULT_STAGED_DIR_NAME = "Staged"
DEFAULT_LLM_OUTPUT_DIR_NAME = "stage" # This is where .llm.txt files go
DEFAULT_MAX_API_RETRIES = 3
DEFAULT_MAX_VALIDATION_RETRIES = 5 # 1 initial try + 1 corrective retry
DEFAULT_RETRY_DELAY_SECONDS = 7
DEFAULT_CONCURRENT_REQUESTS = 20   # Max parallel LLM calls

# Regex to parse input lines from Staged/*.txt files
SENTENCE_LINE_REGEX = re.compile(r"^{S\d+:\s*(.*)}$")
CHAPTER_MARKER_REGEX = re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")

# --- Markers and Sentinels ---
END_SENTENCE_MARKER_TEXT = "END_SENTENCE"
FATAL_QUOTA_ERROR_SENTINEL = "__FATAL_QUOTA_ERROR_DETECTED_STOP_BOOK__"
COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX = "// LLM_COPYRIGHT_DETECTED_REPHRASE_ATTEMPT_FOR_SOURCE: "
RESUME_MARKER_PREFIX = "// --- PARTIAL_FILE_RESUME_NEXT_ITEM_INDEX: "
OUTPUT_LIMITED_MARKER_PREFIX = "// --- OUTPUT_LIMITED_TO_FIRST_"
COMPLETION_MARKER_TEXT = "// --- BOOK_FULLY_PROCESSED --- //"


# --- LLM_PROMPT_TEMPLATE ---
LLM_PROMPT_TEMPLATE = """
You are an expert linguist and data formatter.
Your primary task is to process the "TARGET SENTENCE" provided below.
Use the "PRECEDING CONTEXT" and "SUCCEEDING CONTEXT" (if available) ONLY for disambiguation, pronoun resolution, and to understand the narrative flow related to the TARGET SENTENCE.
DO NOT generate full output blocks for the context sentences.
The output block you generate MUST correspond ONLY to the "TARGET SENTENCE".

Overall Goal for Simplification:
For both the "SimE" (Simple English) and "SimS" (Simple Spanish) outputs, the primary goal is extreme simplicity suitable for an absolute beginner learner. Think of language appropriate for a **first-grade reading level (e.g., for a 6-7 year old child learning to read or learning a second language from scratch).** Prioritize very common, high-frequency words and simple sentence structures.

---
PRECEDING CONTEXT:
{preceding_context}
---
TARGET SENTENCE (This is the English sentence from the source text you need to process):
"{source_sentence}"
---
SUCCEEDING CONTEXT:
{succeeding_context}
---

The output block for the TARGET SENTENCE MUST strictly follow this format and include all sections, even if some are empty or placeholders.
Pay EXTREME attention to the lemmatization rules for SimSL and AdvSL.

Lemmatization Rules:
- Verbs -> Infinitive.
- Nouns/Adjectives -> Singular, Masculine (where applicable).
- Clitic pronouns (me, te, se, lo, etc.) -> Kept as their own lemma form.
- Proper Nouns (Names, Places) -> Excluded from SimSL and AdvSL. Included in DIGLOT_MAP only with ViabilityFlag(N).
- Articles (el, la, un, a, the, etc.) and common prepositions (de, en, por, to, by, in etc.) ARE INCLUDED in SimSL and AdvSL as their own lemma form.
- Lemmas should be single canonical words. For reflexive verbs (e.g., levantarse), the verb infinitive (levantar) and the reflexive pronoun (se) should be listed as separate lemmas in SimSL/AdvSL. For phrasal prepositions (e.g. cerca de), list the core word (cerca) and the preposition (de) as separate lemmas if they are distinct vocabulary items.

Output Format (for the TARGET SENTENCE only):
AdvS:: [Advanced Spanish translation of the TARGET SENTENCE. Natural Spanish phrase order. This can be complex and match the reading level of the TARGET SENTENCE.]
SimS:: [Simple Spanish translation of the TARGET SENTENCE, adhering to the **first-grade reading level** guidelines. Use very common, high-frequency Spanish vocabulary and simple sentence structures. This should be a direct, simple translation of the SimE below.]
SimE:: [Simple English version of the TARGET SENTENCE, adhering to the **first-grade reading level** guidelines. Paraphrase the TARGET SENTENCE extensively to use very common, high-frequency English words (e.g., from basic sight word lists) and simple sentence structures, even if this makes the sentence very basic. Retain the core original meaning. This SimE version will be the basis for SimS_Segments and DIGLOT_MAP word selection.]

SimS_Segments::
S1([SimS Phrase 1 Text derived from SimS above, which was based on SimE])
S2([SimS Phrase 2 Text derived from SimS above, which was based on SimE])
// ... (Segment SimS into meaningful phrases)

PHRASE_ALIGN::
S1 ~ [AdvS Span corresponding to S1 from AdvS above] ~ [SimE Phrase 1 text that corresponds to SimS_Segments S1]
S2 ~ [AdvS Span corresponding to S2 from AdvS above] ~ [SimE Phrase 2 text that corresponds to SimS_Segments S2]
// ... (Align SimS_Segments to AdvS and SimE spans)

SimSL:: // Phrasal: Lemmas for SimS segments of the TARGET SENTENCE. Strictly lemmatized per rules.
S1 :: [lemma1 lemma2 ...]
S2 :: [lemmaA lemmaB ...]
// ...

AdvSL:: [flat_list_adv_lemma1 flat_list_adv_lemma2 ...] // Flat list: **Ensure ALL** non-proper-noun AdvS lemmas for the TARGET SENTENCE are included. Strictly lemmatized per rules.

DIGLOT_MAP:: // For Level 4 generation. Map words from the generated SimE phrases to their Spanish equivalents.
// EngWord: Use the base English word from the corresponding SimE phrase (e.g., 'Alice' if SimE contains 'Alice's' or 'Alice'; 'cat' if SimE contains 'cat's' or 'cat'). If SimE uses a compound best treated as a single unit (e.g., 'no one'), represent it as a single EngWord (e.g., 'noone' or if spaces are problematic for parsing the map, 'no_one' is acceptable).
// SpaLemma: The Spanish lemma. For reflexive verbs, use the main verb infinitive (e.g., for 'levantarse', SpaLemma is 'levantar').
// ExactSpaForm: The exact Spanish word form that would be used in a substitution.
// ViabilityFlag (Y/N):
//   - (Y): This EngWord->ExactSpaForm is a good candidate for direct substitution in an English sentence (Level 4).
//          This includes common content words (nouns, verbs, adjectives, adverbs) AND ALSO foundational function words
//          (like articles 'a', 'the'; prepositions 'to', 'by', 'in'; common conjunctions 'and', 'but')
//          that are beneficial for early exposure.
//   - (N): This EngWord->ExactSpaForm is NOT a good candidate for direct substitution in Level 4.
//          This typically applies to Proper Nouns (e.g., Alice, London), or English words/phrases that don't have a
//          simple, direct, single-word Spanish equivalent suitable for beginner substitution (e.g., complex idioms,
//          or if an English compound word like 'rabbit-hole' maps to a multi-word Spanish phrase not ideal for single substitution).
//          The goal of (Y) is to introduce learnable Spanish vocabulary (content or foundational function words)
//          into an English sentence structure.
// For English auxiliary verbs (e.g., 'is', 'will', 'did', 'has', 'would', 'can', 'may', 'should'):
//   - If it has a simple, direct, common Spanish equivalent suitable for Viability(Y) substitution (e.g., 'is' -> 'ser(es)(Y)'), provide it.
//   - If its meaning is best captured by the main Spanish verb's conjugation (e.g., 'will go' -> 'ir(irá)(Y)'), then map the English *main verb* to the *conjugated Spanish verb*.
//   - If the auxiliary is difficult to map directly for a beginner with Viability(Y), or if it is purely grammatical and does not make sense for substitution, you MAY OMIT it from the DIGLOT_MAP for that segment, or use a Viability(N) entry like 'will->FUTURO(FUTURO)(N)'. AVOID outputting empty SpaLemma or ExactSpaForm like 'EngWord->( )(N)'.
S1 :: [EngWord1_from_SimE_S1_phrase]->[SpaLemma1]([ExactSpaForm1])([ViabilityFlagY/N]) | [EngWord2_from_SimE_S1_phrase]->[SpaLemma2]([ExactSpaForm2])([ViabilityFlagY/N])
// ...

// LOCKED_PHRASE :: [Sx Sy ...] (Optional, only if certain SimS_Segments from TARGET SENTENCE should always stay together)
// (The LLM's output for the TARGET SENTENCE block should conclude after the last section like DIGLOT_MAP or LOCKED_PHRASE. Do not add an "END_SENTENCE" line yourself.)
---
Ensure all sections are present for the TARGET SENTENCE.
IMPORTANT: Your generated output for a single TARGET SENTENCE should be one continuous block of text containing all the required '::' sections. DO NOT include the literal string '{END_SENTENCE_MARKER_TEXT}' anywhere within this block of text you generate; the '{END_SENTENCE_MARKER_TEXT}' marker is only used externally by other scripts to separate distinct, fully-formed blocks in the final combined file.

Example for Regular Sentence:
PRECEDING CONTEXT:
"The house stood on a slight eminence, and its windows commanded an extensive view."
---
TARGET SENTENCE (This is the English sentence from the source text you need to process):
"It is a truth universally acknowledged, that a single man in possession of a good fortune, must be in want of a wife."
---
SUCCEEDING CONTEXT:
"Though Mr. Bennet was not insensible to the advantages of his marriage, he had long since given up the hope of any companionable solace in it."
---
EXPECTED OUTPUT (for "It is a truth universally acknowledged..."):
AdvS:: Es una verdad universalmente reconocida que un hombre soltero, poseedor de una gran fortuna, necesita una esposa.
SimS:: Es una cosa muy sabida. Un hombre solo con mucho dinero necesita una esposa.
SimE:: It is a very known thing. A single man with much money needs a wife.
SimS_Segments::
S1(Es una cosa muy sabida.)
S2(Un hombre solo con mucho dinero)
S3(necesita una esposa.)
PHRASE_ALIGN::
S1 ~ Es una verdad universalmente reconocida ~ It is a very known thing.
S2 ~ que un hombre soltero, poseedor de una gran fortuna ~ A single man with much money
S3 ~ necesita una esposa. ~ needs a wife.
SimSL::
S1 :: ser uno cosa muy sabido
S2 :: uno hombre solo con mucho dinero
S3 :: necesitar uno esposa
AdvSL:: ser uno verdad universalmente reconocido que uno hombre soltero poseedor de uno grande fortuna necesitar uno esposa
DIGLOT_MAP::
S1 :: It->él(él)(N) | is->ser(es)(Y) | a->uno(una)(Y) | very->mucho(muy)(Y) | known->sabido(sabida)(Y) | thing->cosa(cosa)(Y)
S2 :: A->uno(Un)(Y) | single->solo(solo)(Y) | man->hombre(hombre)(Y) | with->con(con)(Y) | much->mucho(mucho)(Y) | money->dinero(dinero)(Y)
S3 :: needs->necesitar(necesita)(Y) | a->uno(una)(Y) | wife->esposa(esposa)(Y)
// (The LLM's output for the TARGET SENTENCE block for this example would conclude here.)
//          into an English sentence structure.
// For English auxiliary verbs ... AVOID outputting empty SpaLemma or ExactSpaForm like 'EngWord->( )(N)'.
//
// For Proper Nouns (e.g., John, London): these MUST be included in DIGLOT_MAP with ViabilityFlag(N). The SpaLemma and ExactSpaForm should be the Proper Noun itself. Example: `John->John(John)(N)`.
//
// Ensure every entry strictly follows the `EngWord->SpaLemma(ExactSpaForm)(ViabilityFlag)` pattern. For simple words like 'and', the SpaLemma and ExactSpaForm might be the same. Example: `and->y(y)(Y)`.
//
// If a specific SimS_Segment S<n> (from SimS_Segments section) results in no words that can or should be mapped in DIGLOT_MAP for that S<n>, output its line as `S<n> ::` (i.e., the S-ID followed by `::` and nothing else on that line).
//
// Example:
// S1 :: [EngWord1_from_SimE_S1_phrase]->[SpaLemma1]([ExactSpaForm1])([ViabilityFlagY/N]) | [EngWord2_from_SimE_S1_phrase]->[SpaLemma2]([ExactSpaForm2])([ViabilityFlagY/N])
// S2 :: // This segment S2 had no mappable words
// S3 :: [EngWordA_from_SimE_S3_phrase]->[SpaLemmaA]([ExactSpaFormA])([ViabilityFlagY/N])
// ...

// LOCKED_PHRASE :: [Sx Sy ...] (Optional, only if certain SimS_Segments from TARGET SENTENCE should always stay together)
// (IMPORTANT INSTRUCTION TO LLM ASSISTANT: Your output for the TARGET SENTENCE block stops after the last section. Do not add '{END_SENTENCE_MARKER_TEXT}' yourself.)
"""

def load_project_config(config_path_str="config.toml"):
    try:
        import tomllib # Python 3.11+
        TOML_LOAD_MODE = "rb"
        def load_toml_file(f): return tomllib.load(f)
    except ImportError:
        try:
            import toml # Fallback: pip install toml
            TOML_LOAD_MODE = "r"
            def load_toml_file(f): return toml.load(f)
            print("Using 'toml' library for config. Python 3.11+ with 'tomllib' is preferred.", file=sys.stderr)
        except ImportError:
            print("TOML library not found. Please install 'toml' (pip install toml) or use Python 3.11+.", file=sys.stderr)
            return None

    config_path = Path(config_path_str)
    if not config_path.is_file():
        print(f"Error: Project configuration file '{config_path}' not found.", file=sys.stderr)
        return None
    try:
        with open(config_path, TOML_LOAD_MODE) as f:
            config_data = load_toml_file(f)
        return config_data.get("content_project_dir")
    except Exception as e:
        print(f"Error parsing TOML file '{config_path}': {e}", file=sys.stderr)
        return None


async def process_sentence_with_llm_async(
    source_sentence_text: str,
    preceding_context: str, # Will be used to format the prompt
    succeeding_context: str, # Will be used to format the prompt
    model_to_use: genai.GenerativeModel, # Specific model object
    model_name_for_log: str, # For logging
    max_api_retries: int,
    max_validation_retries: int,
    retry_delay_seconds: int,
    semaphore: asyncio.Semaphore,
    item_idx_for_log: int, # 1-based index for logging
    # LLM_PROMPT_TEMPLATE is now passed in, not a global
    llm_prompt_template_str: str,
    # Flag to indicate if this is the very first attempt for this sentence (used for primary model)
    # OR if this is a retry specifically due to copyright for this sentence.
    is_copyright_retry_attempt: bool = False
) -> str: # Returns core LLM output, or placeholder, or FATAL_QUOTA_SENTINEL

    # Ensure END_SENTENCE_MARKER_TEXT in the prompt is correctly substituted
    # This template is specific to this function call, formatted from the global one.
    current_original_prompt_text_for_sentence = llm_prompt_template_str.format(
        preceding_context=preceding_context,
        source_sentence=source_sentence_text,
        succeeding_context=succeeding_context,
        END_SENTENCE_MARKER_TEXT=END_SENTENCE_MARKER_TEXT # Ensure this is in your f-string
    )
    current_prompt_to_send = current_original_prompt_text_for_sentence
    
    last_validation_error_details_str = ""

    safety_settings = [
        {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE"},
    ]
    generation_config_params = {"temperature": 0.75}

    async with semaphore:
        for validation_attempt in range(max_validation_retries):
            if validation_attempt > 0: # This is a retry
                if is_copyright_retry_attempt and validation_attempt == 1: # First retry *after* a copyright flag
                    # This prompt is only used if the *initial* call to this function
                    # was flagged as a copyright retry by the orchestrator.
                    # Or, more simply, if the previous raw_llm_output_core was the copyright placeholder.
                    # Let's refine: the corrective prompt is based on the *previous output of this function's loop*.
                    # The `is_copyright_retry_attempt` flag is now less important here as we check last_raw_output.
                    # For simplicity, let's keep the corrective prompt construction based on `last_validation_error_details_str`
                    # and specific placeholders.

                    # If the *previous output that failed validation* was our copyright placeholder,
                    # then we construct the copyright-specific retry.
                    # We need to get that `raw_llm_output_core` from the previous iteration.
                    # This means `raw_llm_output_core` should be defined outside the API loop.
                    # Let's assume `last_raw_api_output_before_validation` is available from previous iteration.
                    # This is getting complex. Simpler: if the *reason* for retry is copyright, the orchestrator calls this
                    # function with `is_copyright_retry_attempt = True` for the *first* call of this model.
                    # The prompt construction needs to handle this.
                    # The version I provided before had this logic for constructing the prompt:
                    # if is_copyright_retry_prompt and validation_attempt == 0 (first attempt of *this* model call)
                    # OR if general validation failure (validation_attempt > 0)

                    # Let's simplify the prompt construction logic here:
                    # The `current_prompt_to_send` will be set based on `is_copyright_retry_attempt`
                    # by the orchestrator if it's a fallback call.
                    # If it's a validation retry within the same model call:
                    corrective_instruction = (
                        "PREVIOUS ATTEMPT FAILED VALIDATION. PLEASE PAY EXTREME ATTENTION TO THE REQUIRED OUTPUT FORMAT. "
                        f"Specifically, ensure all sections are present and correctly formatted. Previous errors: {last_validation_error_details_str}\n"
                        "Re-generate the entire block for the TARGET SENTENCE ensuring adherence to the format.\n"
                        "ADDITIONALLY, before the 'AdvS::' line, please include a few lines starting with '// DEBUG:' "
                        "explaining any specific difficulties or ambiguities you encountered with the previous attempt or the source sentence "
                        "that might have led to the errors.\n"
                        "---\n"
                    )
                    current_prompt_to_send = corrective_instruction + current_original_prompt_text_for_sentence


            # Special handling for the very first attempt if it's a copyright retry
            if is_copyright_retry_attempt and validation_attempt == 0:
                corrective_instruction_copyright = (
                    "THE SYSTEM BELIEVES A PREVIOUS ATTEMPT (POSSIBLY WITH A DIFFERENT MODEL) " # Clarify context
                    "WAS BLOCKED DUE TO POTENTIAL COPYRIGHT/RECITATION FOR THE TARGET SENTENCE. "
                    "PLEASE REPHRASE THE SimE, SimS, AND AdvS OUTPUTS SIGNIFICANTLY TO AVOID RESEMBLANCE TO COPYRIGHTED MATERIAL, "
                    "WHILE MAINTAINING THE CORE MEANING OF THE TARGET SENTENCE. THEN, REGENERATE ALL OTHER SECTIONS BASED ON THE REPHRASED CONTENT. "
                    "PAY EXTREME ATTENTION TO THE REQUIRED OUTPUT FORMAT.\n"
                    "ADDITIONALLY, before the 'AdvS::' line, please include a few lines starting with '// DEBUG:' "
                    "explaining what part of the original sentence might have triggered the copyright/recitation flag, if you can identify it. "
                    "Then proceed with the rephrased full block.\n"
                    "---\n"
                )
                current_prompt_to_send = corrective_instruction_copyright + current_original_prompt_text_for_sentence


            # This `raw_llm_output_core` needs to be accessible for the copyright check after the loop
            # Initialize it here for each validation attempt.
            _raw_llm_output_core_this_api_cycle = None # Use a distinct name
            api_call_successful_flag = False

            for api_attempt in range(max_api_retries):
                try:
                    response = await model_to_use.generate_content_async(
                        current_prompt_to_send,
                        safety_settings=safety_settings,
                        generation_config=generation_config_params
                    )
                    if response.parts:
                        _raw_llm_output_core_this_api_cycle = response.text.strip()
                        api_call_successful_flag = True
                        if response.prompt_feedback and response.prompt_feedback.block_reason:
                            reason_str = str(response.prompt_feedback.block_reason).lower()
                            reason_msg = response.prompt_feedback.block_reason_message or reason_str
                            if "recitation" in reason_str or response.prompt_feedback.block_reason == 4:
                                print(f"  LLM API Warning ({model_name_for_log}, Item {item_idx_for_log}): Potential copyright/recitation block. Reason: {reason_msg}", file=sys.stderr)
                                _raw_llm_output_core_this_api_cycle = f"{COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX}{source_sentence_text}"
                        break
                    else:
                        reason = "Unknown reason, empty parts list in response."
                        is_fatal_quota = False
                        if response.prompt_feedback and response.prompt_feedback.block_reason:
                            reason_str = str(response.prompt_feedback.block_reason).lower()
                            reason_msg = response.prompt_feedback.block_reason_message or reason_str
                            reason = reason_msg
                            if "quota" in reason_str or "limit" in reason_str or "billing" in reason_str or "exceeded" in reason_str:
                                is_fatal_quota = True
                            elif "recitation" in reason_str or response.prompt_feedback.block_reason == 4 :
                                print(f"  LLM API Warning ({model_name_for_log}, Item {item_idx_for_log}): Potential copyright/recitation block (no parts). Reason: {reason}", file=sys.stderr)
                                _raw_llm_output_core_this_api_cycle = f"{COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX}{source_sentence_text}"
                                api_call_successful_flag = True; break
                        print(f"  LLM Warning ({model_name_for_log}, API Attempt {api_attempt+1}) for item {item_idx_for_log}: Blocked or empty parts. Reason: {reason}", file=sys.stderr)
                        if is_fatal_quota: return FATAL_QUOTA_ERROR_SENTINEL
                        if not api_call_successful_flag:
                            _raw_llm_output_core_this_api_cycle = f"// LLM_BLOCKED_NO_PARTS ({model_name_for_log}, Reason: {reason}) FOR_SOURCE: {source_sentence_text}"
                        break
                except Exception as e:
                    error_str = str(e).lower()
                    # ... (same exception handling as before: FATAL_QUOTA, retries, placeholder on max retries) ...
                    print(f"  LLM API Error ({model_name_for_log}, API Attempt {api_attempt + 1}/{max_api_retries}) for item {item_idx_for_log}: {e}", file=sys.stderr)
                    if "429" in error_str or "resource_exhausted" in error_str or "quota" in error_str or "rate_limit_exceeded" in error_str or "model_unavailable" in error_str:
                        if api_attempt + 1 == max_api_retries:
                            print(f"    FATAL QUOTA LIKELY ({model_name_for_log}, persisted API error): Item {item_idx_for_log}. Error: {e}", file=sys.stderr)
                            return FATAL_QUOTA_ERROR_SENTINEL
                        effective_delay = retry_delay_seconds * (2**api_attempt)
                        print(f"    Item {item_idx_for_log} ({model_name_for_log}): Retrying API call (potential rate limit) in {effective_delay} seconds...", file=sys.stderr)
                        await asyncio.sleep(effective_delay)
                    elif api_attempt + 1 == max_api_retries:
                        _raw_llm_output_core_this_api_cycle = f"// LLM_API_ERROR_MAX_RETRIES ({model_name_for_log}, {type(e).__name__}) FOR_SOURCE: {source_sentence_text}"
                        break
                    else: await asyncio.sleep(retry_delay_seconds)

            # Use the result from this API cycle
            raw_llm_output_core_from_api = _raw_llm_output_core_this_api_cycle

            if raw_llm_output_core_from_api == FATAL_QUOTA_ERROR_SENTINEL: return FATAL_QUOTA_ERROR_SENTINEL
            if not api_call_successful_flag and not raw_llm_output_core_from_api :
                 raw_llm_output_core_from_api = f"// LLM_NO_OUTPUT_AFTER_API_RETRIES ({model_name_for_log}) FOR_SOURCE: {source_sentence_text}"

            if END_SENTENCE_MARKER_TEXT in raw_llm_output_core_from_api:
                lines_cleaned = [line for line in raw_llm_output_core_from_api.splitlines() if line.strip() != END_SENTENCE_MARKER_TEXT]
                raw_llm_output_core_from_api = "\n".join(lines_cleaned).strip()

            raw_output_for_validation = raw_llm_output_core_from_api
            debug_lines_from_llm = []
            if "// DEBUG:" in raw_llm_output_core_from_api:
                # ... (same debug line parsing logic as before, using raw_llm_output_core_from_api) ...
                potential_block_lines = []
                seen_advs_or_first_content_marker = False
                possible_first_markers = ["AdvS::", "SimS::", "SimE::", "CHAPTER_MARKER_DIRECT::"] # Add other markers if needed
                for line_content in raw_llm_output_core_from_api.splitlines():
                    stripped_line = line_content.strip()
                    is_debug_line = stripped_line.startswith("// DEBUG:")
                    if not seen_advs_or_first_content_marker:
                        for marker_val in possible_first_markers: # Renamed marker to marker_val
                            if stripped_line.startswith(marker_val):
                                seen_advs_or_first_content_marker = True; break
                    if is_debug_line: debug_lines_from_llm.append(line_content)
                    elif seen_advs_or_first_content_marker: potential_block_lines.append(line_content)
                    elif not stripped_line.startswith("//"): potential_block_lines.append(line_content)
                if debug_lines_from_llm:
                    print(f"  Item {item_idx_for_log} ({model_name_for_log}) - LLM Debug Info (Validation Attempt {validation_attempt+1}):")
                    for d_line in debug_lines_from_llm: print(f"    {d_line.strip()}")
                if not potential_block_lines and debug_lines_from_llm : raw_output_for_validation = "\n".join(debug_lines_from_llm)
                elif potential_block_lines: raw_output_for_validation = "\n".join(potential_block_lines).strip()

            validation_errors = validate_llm_block(raw_output_for_validation)
            last_validation_error_details_str = "; ".join(validation_errors)

            if not validation_errors: return raw_output_for_validation # Success

            print(f"  Item {item_idx_for_log} ({model_name_for_log}): Validation FAILED (Attempt {validation_attempt+1}/{max_validation_retries}) for '{source_sentence_text[:30]}...': {last_validation_error_details_str}", file=sys.stderr)
            
            # If the raw output (before stripping debug lines) was a copyright placeholder,
            # it implies the *next* attempt (if any) should be a copyright-specific retry.
            is_copyright_retry_attempt = raw_llm_output_core_from_api.startswith(COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX)
            
            if validation_attempt + 1 < max_validation_retries:
                await asyncio.sleep(retry_delay_seconds)
                # The prompt for the next validation_attempt will be constructed at the top of the loop.
                # If is_copyright_retry_attempt is now true, it will use that specific prompt.
                continue
            else: # Max validation retries for this model invocation
                # Determine if failure was due to original copyright flag or general validation
                if is_copyright_retry_attempt : # It was a copyright issue that couldn't be fixed
                     return f"// {model_name_for_log}_COPYRIGHT_VALIDATION_FAILED_MAX_RETRIES (Errors: {last_validation_error_details_str}) FOR_SOURCE: {source_sentence_text}"
                else: # General validation failure
                    return f"// {model_name_for_log}_VALIDATION_FAILED_MAX_RETRIES (Errors: {last_validation_error_details_str}) FOR_SOURCE: {source_sentence_text}"
        
        return f"// {model_name_for_log}_VALIDATION_LOOP_UNEXPECTED_EXIT_FOR_SOURCE: {source_sentence_text}"
########
async def process_book_file_async(
    staged_file_path: Path, llm_output_dir: Path, 
    model_obj: genai.GenerativeModel, # Single model object
    args: argparse.Namespace,
    num_context_sentences: int, item_limit: Optional[int],
    semaphore: asyncio.Semaphore
) -> Tuple[bool, bool, bool]: # (was_skipped, operation_successful, daily_limit_hit_flag)

    book_name_stem = staged_file_path.stem
    output_llm_file_path = llm_output_dir / f"{book_name_stem}.llm.txt"

    start_item_idx = 0
    is_resuming = False
    existing_content_blocks: List[str] = []

    # --- Resume Logic (same as before, ensure it's correct) ---
    if not args.force and output_llm_file_path.exists():
        try:
            with open(output_llm_file_path, 'r', encoding='utf-8') as f_exist:
                full_existing_content = f_exist.read().strip()
            raw_blocks_with_delimiters = re.findall(f"(.*?{END_SENTENCE_MARKER_TEXT}\n?)", full_existing_content, re.DOTALL)
            if raw_blocks_with_delimiters:
                last_block_full_content = raw_blocks_with_delimiters[-1]
                lines_in_last_block = last_block_full_content.strip().splitlines()
                penultimate_line_of_last_block = ""
                if len(lines_in_last_block) >= 2 and lines_in_last_block[-1].strip() == END_SENTENCE_MARKER_TEXT:
                    penultimate_line_of_last_block = lines_in_last_block[-2].strip()
                elif len(lines_in_last_block) == 1: penultimate_line_of_last_block = lines_in_last_block[0].strip()

                if penultimate_line_of_last_block == COMPLETION_MARKER_TEXT:
                    print(f"Skipping '{staged_file_path.name}': Found completion marker.")
                    return True, True, False
                elif penultimate_line_of_last_block.startswith(RESUME_MARKER_PREFIX) and penultimate_line_of_last_block.endswith("--- //"):
                    try:
                        index_str = penultimate_line_of_last_block[len(RESUME_MARKER_PREFIX):].split(" ")[0]
                        start_item_idx = int(index_str)
                        is_resuming = True
                        existing_content_blocks = raw_blocks_with_delimiters[:-1]
                        print(f"Resuming '{staged_file_path.name}' from source item index {start_item_idx}.")
                    except ValueError: start_item_idx = 0; is_resuming = False; existing_content_blocks = [] # Reprocess
                elif penultimate_line_of_last_block.startswith(OUTPUT_LIMITED_MARKER_PREFIX) and penultimate_line_of_last_block.endswith("--- //"):
                    try:
                        limit_in_marker_str = penultimate_line_of_last_block[len(OUTPUT_LIMITED_MARKER_PREFIX):].split("_ITEMS")[0]
                        limit_in_marker = int(limit_in_marker_str)
                        can_process_more = args.limit_items is None or args.limit_items > limit_in_marker
                        if can_process_more:
                            start_item_idx = len(raw_blocks_with_delimiters) -1 
                            is_resuming = True; existing_content_blocks = raw_blocks_with_delimiters[:-1]
                        else: return True, True, False # Skip
                    except ValueError: start_item_idx = 0; is_resuming = False; existing_content_blocks = [] # Reprocess
                elif args.limit_items is None: start_item_idx = 0; is_resuming = False; existing_content_blocks = [] # Reprocess ambiguous
        except Exception as e_read:
            print(f"Warning: Error reading existing output file '{output_llm_file_path.name}': {e_read}. Reprocessing from start.", file=sys.stderr)
            start_item_idx = 0; is_resuming = False; existing_content_blocks = []
    
    # --- Load source Staged/*.txt items ---
    print(f"Processing '{staged_file_path.name}' (effective start source item index: {start_item_idx})...")
    # ... (same logic to load all_items) ...
    try:
        with open(staged_file_path, 'r', encoding='utf-8') as f_in: raw_lines_from_staged_file = f_in.readlines()
    except Exception as e: return False, False, False
    all_items: List[Dict[str, Any]] = []
    for orig_idx, line_raw in enumerate(raw_lines_from_staged_file):
        line = line_raw.strip()
        if CHAPTER_MARKER_REGEX.match(line): all_items.append({"type": "marker", "text": CHAPTER_MARKER_REGEX.match(line).group(1).strip(), "original_idx_in_file": orig_idx})
        elif SENTENCE_LINE_REGEX.match(line): all_items.append({"type": "sentence", "text": SENTENCE_LINE_REGEX.match(line).group(1).strip(), "original_idx_in_file": orig_idx})
    if not all_items: # ... (handle no processable items) ...
        try: # Write placeholder
            llm_output_dir.mkdir(parents=True, exist_ok=True)
            with open(output_llm_file_path, 'w', encoding='utf-8') as f_out: f_out.write(f"// NO_PROCESSABLE_ITEMS_IN_SOURCE_FILE\n{END_SENTENCE_MARKER_TEXT}\n")
            print(f"Wrote placeholder: {output_llm_file_path.name}.")
        except Exception: return False, False, False
        return False, True, False

    # --- Determine items to process ---
    # ... (same logic for items_to_process_this_run_slice, target_total_items_in_output_file, num_existing_items_count, etc.) ...
    items_to_process_this_run_slice: List[Dict[str, Any]] = []
    target_total_items_in_output_file = len(all_items)
    if args.limit_items is not None: target_total_items_in_output_file = min(args.limit_items, len(all_items))
    num_existing_items_count = "".join(existing_content_blocks).count(END_SENTENCE_MARKER_TEXT)
    if num_existing_items_count >= target_total_items_in_output_file: # Finalize if limit met by existing
        if is_resuming:
            try:
                with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
                    f_out.write("".join(existing_content_blocks))
                    if num_existing_items_count >= len(all_items): f_out.write(f"{COMPLETION_MARKER_TEXT}\n{END_SENTENCE_MARKER_TEXT}\n")
                    elif args.limit_items is not None and num_existing_items_count >= args.limit_items: f_out.write(f"{OUTPUT_LIMITED_MARKER_PREFIX}{args.limit_items}_ITEMS --- //\n{END_SENTENCE_MARKER_TEXT}\n")
                print(f"Finalized '{output_llm_file_path.name}'.")
            except Exception as e_fin: print(f"Error finalizing {output_llm_file_path.name}: {e_fin}")
        return False, True, False
    num_new_items_to_process_this_run = target_total_items_in_output_file - num_existing_items_count
    end_idx_for_slice_in_all_items = min(len(all_items), start_item_idx + num_new_items_to_process_this_run)
    if start_item_idx < end_idx_for_slice_in_all_items: items_to_process_this_run_slice = all_items[start_item_idx:end_idx_for_slice_in_all_items]
    if not items_to_process_this_run_slice: # Finalize if no new items needed
        if is_resuming:
            try:
                with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
                    f_out.write("".join(existing_content_blocks))
                    if num_existing_items_count >= len(all_items): f_out.write(f"{COMPLETION_MARKER_TEXT}\n{END_SENTENCE_MARKER_TEXT}\n")
                    elif args.limit_items is not None and num_existing_items_count >= args.limit_items: f_out.write(f"{OUTPUT_LIMITED_MARKER_PREFIX}{args.limit_items}_ITEMS --- //\n{END_SENTENCE_MARKER_TEXT}\n")
                print(f"Finalized '{output_llm_file_path.name}' as no new items needed.")
            except Exception as e_fin: print(f"Error finalizing {output_llm_file_path.name}: {e_fin}")
        return False, True, False

    # --- Prepare and Execute Tasks (Simplified for single model, sequential calls with semaphore inside) ---
    newly_processed_blocks_this_run: List[str] = []
    daily_limit_hit_for_this_book = False
    first_failed_item_original_idx = -1
    llm_calls_made_this_run = 0
    num_successfully_processed_items_this_run = 0

    for run_slice_idx, item_data in enumerate(items_to_process_this_run_slice):
        original_item_idx_in_all_items = start_item_idx + run_slice_idx

        if daily_limit_hit_for_this_book and first_failed_item_original_idx != -1:
            print(f"  Skipping further processing for source item {original_item_idx_in_all_items+1} in '{book_name_stem}' due to earlier fatal quota error.")
            break # Stop processing more items for THIS BOOK in this run

        item_output_block_content = None
        current_item_is_copyright_retry = False # This would be set if a previous attempt for this item was copyright

        if item_data["type"] == "marker":
            item_output_block_content = f"CHAPTER_MARKER_DIRECT:: {item_data['text']}"
            # No LLM call for markers, directly count as "processed" for block counting
            # This num_successfully_processed_items_this_run is for actual items added to output
            num_successfully_processed_items_this_run +=1 
        
        elif item_data["type"] == "sentence":
            source_sentence_text = item_data["text"]
            preceding_context_str = "[NO PRECEDING CONTEXT]"
            pre_sent_texts = [all_items[i]["text"] for i in range(max(0, original_item_idx_in_all_items - num_context_sentences), original_item_idx_in_all_items) if all_items[i]["type"] == "sentence"]
            if pre_sent_texts: preceding_context_str = "\n".join(pre_sent_texts)
            succeeding_context_str = "[NO SUCCEEDING CONTEXT]"
            suc_sent_texts = [all_items[i]["text"] for i in range(original_item_idx_in_all_items + 1, min(len(all_items), original_item_idx_in_all_items + 1 + num_context_sentences)) if all_items[i]["type"] == "sentence"]
            if suc_sent_texts: succeeding_context_str = "\n".join(suc_sent_texts)

            print(f"  Processing item {original_item_idx_in_all_items+1} ('{source_sentence_text[:30]}...') with model {MODEL_NAME}.")
            
            # We assume `model_obj` passed to process_book_file_async IS the one from MODEL_NAME
            # The `is_copyright_retry_attempt` flag is for *this specific call* to process_sentence_with_llm_async
            # If we were implementing a retry *within process_book_file_async* for copyright, this flag would change.
            # For now, let's assume the first call is not a specific copyright retry unless prior state indicated it.
            # This part might need refinement if we want `process_book_file_async` to orchestrate copyright retries
            # for the *same* model before trying a fallback (which we removed).
            # The current `process_sentence_with_llm_async` handles its *own* copyright retries internally based on its output.

            item_result_str = await process_sentence_with_llm_async(
                source_sentence_text, preceding_context_str, succeeding_context_str,
                model_obj, MODEL_NAME, # Use the global MODEL_NAME for logging inside
                args.max_api_retries, args.max_validation_retries, DEFAULT_RETRY_DELAY_SECONDS,
                semaphore, original_item_idx_in_all_items + 1,
                LLM_PROMPT_TEMPLATE, # Pass the global template string
                is_copyright_retry_attempt=False # Initial call is not a specific copyright retry from orchestrator
            )
            llm_calls_made_this_run += 1

            if item_result_str == FATAL_QUOTA_ERROR_SENTINEL:
                daily_limit_hit_for_this_book = True
                if first_failed_item_original_idx == -1:
                    first_failed_item_original_idx = original_item_idx_in_all_items
                # Do not add this item's "result" to output
            elif not item_result_str.startswith("//") and not item_result_str.startswith(COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX):
                item_output_block_content = item_result_str
                num_successfully_processed_items_this_run +=1
            else: # It's a placeholder for a failure (validation, copyright, API error)
                item_output_block_content = item_result_str # Store the error placeholder
                # We still count this as an "attempted block" for writing, but not "successfully processed item"
        
        if item_output_block_content: # True for markers, successful sentences, or error placeholders
            newly_processed_blocks_this_run.append(item_output_block_content.strip() + f"\n{END_SENTENCE_MARKER_TEXT}\n")
        
        if daily_limit_hit_for_this_book and first_failed_item_original_idx != -1:
            break # Exit loop over items_to_process_this_run_slice for this book

    # --- Assemble final output string & markers (logic same as before) ---
    # ... using `num_successfully_processed_items_this_run` for counts ...
    final_output_content = "".join(existing_content_blocks) + "".join(newly_processed_blocks_this_run)
    # total_items_now_in_file needs to be calculated based on actual successful items + existing
    # This counts all blocks added, including error placeholders or markers.
    # For marker logic, we care about successfully processed items.
    # `num_existing_items_count` was based on END_SENTENCE, so let's use that for consistency.
    total_end_sentence_markers_in_final = final_output_content.count(END_SENTENCE_MARKER_TEXT)


    if daily_limit_hit_for_this_book:
        if first_failed_item_original_idx != -1 and first_failed_item_original_idx < len(all_items):
            final_output_content += f"{RESUME_MARKER_PREFIX}{first_failed_item_original_idx} --- //\n{END_SENTENCE_MARKER_TEXT}\n"
            print(f"  Partial file for '{book_name_stem}' will be saved. Resume from source item index {first_failed_item_original_idx} next time.")
    elif total_end_sentence_markers_in_final >= len(all_items): # All source items represented
        final_output_content += f"{COMPLETION_MARKER_TEXT}\n{END_SENTENCE_MARKER_TEXT}\n"
        print(f"  Marking '{book_name_stem}' as fully processed.")
    elif args.limit_items is not None and total_end_sentence_markers_in_final >= args.limit_items:
        final_output_content += f"{OUTPUT_LIMITED_MARKER_PREFIX}{args.limit_items}_ITEMS --- //\n{END_SENTENCE_MARKER_TEXT}\n"
        print(f"  Marking '{book_name_stem}' as limited to {args.limit_items} items.")
    
    try:
        llm_output_dir.mkdir(parents=True, exist_ok=True)
        with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
            f_out.write(final_output_content)
        
        log_msg_blocks = f"Wrote {len(newly_processed_blocks_this_run)} blocks from this run " \
                         f"(approx {num_successfully_processed_items_this_run} source items/markers processed; " \
                         f"{llm_calls_made_this_run} LLM calls attempted) to '{output_llm_file_path.name}'"
        print(log_msg_blocks)
        return False, True, daily_limit_hit_for_this_book
    except Exception as e:
        print(f"Error writing output file '{output_llm_file_path.name}': {e}", file=sys.stderr)

        return False, False, daily_limit_hit_for_this_book
###############
async def main_async():
    dotenv_path = Path('.') / '.env'
    if dotenv_path.is_file():
        load_dotenv(dotenv_path=dotenv_path)
        print(f"INFO: Attempted to load API key from '{dotenv_path.resolve()}'.")
    else:
        print(f"INFO: No .env file found at '{dotenv_path.resolve()}'. Consider creating one for GOOGLE_API_KEY.")

    parser = argparse.ArgumentParser(formatter_class=argparse.ArgumentDefaultsHelpFormatter, description="Process staged text files with an LLM (async) to generate .llm.txt format.")
    parser.add_argument("--api_key", help="Google Gemini API Key. Overrides .env and GOOGLE_API_KEY env variable if provided.")
    parser.add_argument("--project_config", default="config.toml", help="Path to the project's main config.toml file.")
    parser.add_argument("--input_staged_subdir", default=DEFAULT_STAGED_DIR_NAME, help="Subdirectory within content_project_dir for input .txt files.")
    parser.add_argument("--output_llm_subdir", default=DEFAULT_LLM_OUTPUT_DIR_NAME, help="Subdirectory within content_project_dir for output .llm.txt files.")
    parser.add_argument("--force", action="store_true", help="Force reprocessing of files from scratch, ignoring existing partial or complete .llm.txt files.")
    parser.add_argument("--max_api_retries", type=int, default=DEFAULT_MAX_API_RETRIES, help="Max retries for basic API calls.")
    parser.add_argument("--max_validation_retries", type=int, default=DEFAULT_MAX_VALIDATION_RETRIES, help="Max retries with corrective prompts if LLM output fails validation. Set to 1 for no corrective retries on validation failure.")
    parser.add_argument("--context_sents", type=int, default=2, help="Number of preceding/succeeding sentences for context.")
    parser.add_argument("--limit_items", type=int, default=None, help="Output only the first N items (markers or sentences) in total for each book. Default: process all. Use 0 to write placeholder files without LLM calls if file doesn't exist.")
    parser.add_argument("--concurrent_requests", type=int, default=DEFAULT_CONCURRENT_REQUESTS, help="Max concurrent LLM API requests.")
    
    args = parser.parse_args()

    api_key_to_use = args.api_key or os.getenv("GOOGLE_API_KEY")
    if not api_key_to_use:
        print("ERROR: API Key not found. Set GOOGLE_API_KEY in your shell environment, in a .env file, or use the --api_key argument.", file=sys.stderr)
        sys.exit(1)
    print(f"INFO: Using API key (source: {'command-line argument' if args.api_key else ('environment variable' if os.getenv('GOOGLE_API_KEY') else 'unknown/not set')}).")


    content_project_root_str = load_project_config(args.project_config)
    if not content_project_root_str: sys.exit(1)
    
    content_project_root = Path(content_project_root_str)
    staged_input_dir = content_project_root / args.input_staged_subdir
    llm_output_dir = content_project_root / args.output_llm_subdir

    if not staged_input_dir.is_dir():
        print(f"ERROR: Input directory '{staged_input_dir}' not found.", file=sys.stderr)
        sys.exit(1)
    try:
        genai.configure(api_key=api_key_to_use)
        # The `model` object here is the single model instance we'll use
        model_obj_for_processing = genai.GenerativeModel(MODEL_NAME) 
        print(f"Successfully configured Gemini model: {MODEL_NAME}")
    except Exception as e:
        print(f"ERROR configuring Gemini SDK: {e}", file=sys.stderr)
        sys.exit(1) 

    staged_files_to_process = sorted([f for f in staged_input_dir.glob('*.txt') if not f.name.endswith('.junk.txt')])
    if not staged_files_to_process:
        print(f"INFO: No suitable .txt files found in '{staged_input_dir}'.", file=sys.stderr)
        sys.exit(0)
    
    print(f"\nFound {len(staged_files_to_process)} Staged files to process from '{staged_input_dir}'.")
    print(f"LLM output (.llm.txt) will be written to '{llm_output_dir}'.")
    if args.force: print("PROCESSING MODE: --force enabled, will reprocess all files from scratch.")
    if args.limit_items is not None: print(f"PROCESSING MODE: Output limited to --limit_items={args.limit_items} total items per file.")
    print(f"Max concurrent LLM requests: {args.concurrent_requests}")
    print("---")

    total_successful_ops, total_skipped_ops, total_error_ops = 0, 0, 0
    semaphore = asyncio.Semaphore(args.concurrent_requests)
    overall_daily_limit_hit_flag = False

    for staged_file in staged_files_to_process:
        if overall_daily_limit_hit_flag:
            print(f"Daily rate limit was hit earlier. Skipping further processing of '{staged_file.name}' and subsequent files in this run.")
            total_skipped_ops +=1
            continue

        was_skipped, op_successful, book_hit_daily_limit = await process_book_file_async(
            staged_file, llm_output_dir, 
            model_obj_for_processing, 
            args,
            num_context_sentences=args.context_sents,
            item_limit=args.limit_items,
            semaphore=semaphore
        )

        if was_skipped: total_skipped_ops += 1
        elif op_successful: total_successful_ops += 1
        else: total_error_ops += 1
        
        if book_hit_daily_limit:
            overall_daily_limit_hit_flag = True
            print(f"--- Daily rate limit hit while processing '{staged_file.name}'. Will stop processing new books after this. ---")
        
        print("---")

    print("\nProcessing Complete.")
    print(f"Successfully processed/wrote: {total_successful_ops} book(s)/file operation(s).")
    print(f"Skipped (already complete, or due to prior rate limit): {total_skipped_ops} book(s).")
    if total_error_ops > 0:
        print(f"Encountered errors during file operations for: {total_error_ops} book(s).")
    else:
        print("No file operation errors encountered.")
    
    if overall_daily_limit_hit_flag:
        print("\nNOTE: Daily rate limit was encountered. Some files may be partially processed.")
        print("You can re-run the script (e.g., tomorrow) to continue processing from where it left off.")

if __name__ == "__main__":
    try:
        asyncio.run(main_async())
    except KeyboardInterrupt:
        print("\nProcessing interrupted by user. Partial progress for the current book might not be saved unless its write cycle completed.")
    except Exception as e:
        print(f"\nAn unexpected error occurred in main execution: {e}", file=sys.stderr)
        import traceback
        traceback.print_exc()