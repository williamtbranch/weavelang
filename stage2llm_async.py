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
#MODEL_NAME = "models/gemini-2.5-pro-preview-05-06" # Verified from your list_models output
MODEL_NAME = "models/gemini-2.0-flash-lite" # Verified from your list_models output
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
    source_sentence_text: str, preceding_context: str, succeeding_context: str,
    model: genai.GenerativeModel,
    max_api_retries: int, max_validation_retries: int,
    retry_delay_seconds: int,
    semaphore: asyncio.Semaphore,
    item_idx_for_log: int # 1-based index for logging
) -> str: # Returns the core LLM output string (or a placeholder/sentinel)

    # Ensure END_SENTENCE_MARKER_TEXT in the prompt is correctly substituted
    formatted_prompt_template = LLM_PROMPT_TEMPLATE.replace("{END_SENTENCE_MARKER_TEXT}", END_SENTENCE_MARKER_TEXT)

    original_prompt_text = formatted_prompt_template.format(
        preceding_context=preceding_context,
        source_sentence=source_sentence_text,
        succeeding_context=succeeding_context
    )
    current_prompt_to_send = original_prompt_text

    safety_settings = [
        {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE"},
    ]
    generation_config_params = {"temperature": 0.75} # Example, adjust as needed

    async with semaphore:
        for validation_attempt in range(max_validation_retries):
            raw_llm_output_core = None
            api_call_successful_flag = False # To track if we got any meaningful response text or specific error placeholder

            # Construct corrective prompt if this is a retry due to validation failure (not first attempt)
            # Copyright retries will construct their own specific prompt before the API loop
            if validation_attempt > 0 and not current_prompt_to_send.startswith("PREVIOUS ATTEMPT WAS BLOCKED DUE TO POTENTIAL COPYRIGHT"): # Avoid double-prefixing
                # This error_details_str would ideally come from the previous validation_errors
                # For now, it's generic as passing it through async calls is complex.
                error_details_for_prompt = "Previous errors indicated structural issues with the output. "
                corrective_instruction_general = (
                    "PREVIOUS ATTEMPT FAILED VALIDATION. PLEASE PAY EXTREME ATTENTION TO THE REQUIRED OUTPUT FORMAT. "
                    f"Specifically, ensure all sections are present and correctly formatted. {error_details_for_prompt}"
                    "Re-generate the entire block for the TARGET SENTENCE ensuring adherence to the format.\n---\n"
                )
                current_prompt_to_send = corrective_instruction_general + original_prompt_text


            for api_attempt in range(max_api_retries):
                try:
                    response = await model.generate_content_async(
                        current_prompt_to_send,
                        safety_settings=safety_settings,
                        generation_config=generation_config_params
                    )

                    if response.parts:
                        raw_llm_output_core = response.text.strip()
                        api_call_successful_flag = True
                        
                        if response.prompt_feedback and response.prompt_feedback.block_reason:
                            reason_str = str(response.prompt_feedback.block_reason).lower()
                            reason_msg = response.prompt_feedback.block_reason_message or reason_str
                            if "recitation" in reason_str or response.prompt_feedback.block_reason == 4: # FinishReason 4 (RECITATION)
                                print(f"  LLM API Warning (Item {item_idx_for_log}): Potential copyright/recitation block. Reason: {reason_msg}", file=sys.stderr)
                                raw_llm_output_core = f"{COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX}{source_sentence_text}"
                                # This will now fail validation and trigger the copyright-specific retry logic below
                        break # Successful API call or copyright placeholder set

                    else: # No response.parts
                        reason = "Unknown reason, empty parts list in response."
                        is_fatal_quota = False
                        if response.prompt_feedback and response.prompt_feedback.block_reason:
                            reason_str = str(response.prompt_feedback.block_reason).lower()
                            reason_msg = response.prompt_feedback.block_reason_message or reason_str
                            reason = reason_msg
                            if "quota" in reason_str or "limit" in reason_str or "billing" in reason_str or "exceeded" in reason_str:
                                is_fatal_quota = True
                            elif "recitation" in reason_str or response.prompt_feedback.block_reason == 4 : # Copyright
                                print(f"  LLM API Warning (Item {item_idx_for_log}): Potential copyright/recitation block (no parts). Reason: {reason}", file=sys.stderr)
                                raw_llm_output_core = f"{COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX}{source_sentence_text}"
                                api_call_successful_flag = True
                                break # Break API loop, validation will handle the placeholder

                        print(f"  LLM Warning (API Attempt {api_attempt+1}) for item {item_idx_for_log}: Blocked or empty parts. Reason: {reason}", file=sys.stderr)
                        if is_fatal_quota:
                            print(f"    FATAL QUOTA LIKELY: Item {item_idx_for_log}. Reason: {reason}", file=sys.stderr)
                            return FATAL_QUOTA_ERROR_SENTINEL

                        if not api_call_successful_flag: # If not copyright and not fatal quota
                            raw_llm_output_core = f"// LLM_BLOCKED_NO_PARTS (Reason: {reason}) FOR_SOURCE: {source_sentence_text}"
                        # Break API loop for non-fatal blocks too; validation will handle the placeholder.
                        break


                except Exception as e:
                    error_str = str(e).lower()
                    print(f"  LLM API Error (API Attempt {api_attempt + 1}/{max_api_retries}) for item {item_idx_for_log}: {e}", file=sys.stderr)
                    if "429" in error_str or "resource_exhausted" in error_str or "quota" in error_str or "rate_limit_exceeded" in error_str or "model_unavailable" in error_str:
                        if api_attempt + 1 == max_api_retries:
                            print(f"    FATAL QUOTA LIKELY (persisted API error): Item {item_idx_for_log}. Error: {e}", file=sys.stderr)
                            return FATAL_QUOTA_ERROR_SENTINEL
                        effective_delay = retry_delay_seconds * (2**api_attempt)
                        print(f"    Item {item_idx_for_log}: Retrying API call (potential rate limit) in {effective_delay} seconds...", file=sys.stderr)
                        await asyncio.sleep(effective_delay)
                    elif api_attempt + 1 == max_api_retries: # Max retries for other errors
                        raw_llm_output_core = f"// LLM_API_ERROR_MAX_RETRIES ({type(e).__name__}) FOR_SOURCE: {source_sentence_text}"
                        break
                    else: # Other errors, retry with standard delay
                        await asyncio.sleep(retry_delay_seconds)
            
            # --- After API retry loop for this validation_attempt ---
            if raw_llm_output_core == FATAL_QUOTA_ERROR_SENTINEL: # Propagate if set
                return FATAL_QUOTA_ERROR_SENTINEL
            
            if not api_call_successful_flag and not raw_llm_output_core : # If all API attempts failed without setting a placeholder
                 raw_llm_output_core = f"// LLM_NO_OUTPUT_AFTER_API_RETRIES_FOR_SOURCE: {source_sentence_text}"

            # --- Clean END_SENTENCE if LLM accidentally included it ---
            if END_SENTENCE_MARKER_TEXT in raw_llm_output_core:
                # print(f"  DEBUG: Item {item_idx_for_log} - Raw LLM output contained '{END_SENTENCE_MARKER_TEXT}'. Cleaning before validation.")
                lines_cleaned = [line for line in raw_llm_output_core.splitlines() if line.strip() != END_SENTENCE_MARKER_TEXT]
                raw_llm_output_core = "\n".join(lines_cleaned).strip()

            # --- Validation ---
            validation_errors = validate_llm_block(raw_llm_output_core)

            if not validation_errors:
                # print(f"  Item {item_idx_for_log}: Validation PASSED (Attempt {validation_attempt+1}).")
                return raw_llm_output_core # Success!

            # --- Validation Failed ---
            error_details_str = "; ".join(validation_errors)
            print(f"  Item {item_idx_for_log}: Validation FAILED (Attempt {validation_attempt+1}/{max_validation_retries}) for '{source_sentence_text[:30]}...': {error_details_str}", file=sys.stderr)

            # Handle Copyright-specific failure for next validation attempt
            if raw_llm_output_core.startswith(COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX):
                if validation_attempt + 1 < max_validation_retries:
                    print(f"    Item {item_idx_for_log}: Retrying with copyright rephrase prompt.")
                    # Construct the specific copyright rephrase prompt for the next API call in the *next validation_attempt*
                    current_prompt_to_send = (
                        "PREVIOUS ATTEMPT WAS BLOCKED DUE TO POTENTIAL COPYRIGHT/RECITATION. "
                        "PLEASE REPHRASE THE SimE, SimS, AND AdvS OUTPUTS SIGNIFICANTLY TO AVOID RESEMBLANCE TO COPYRIGHTED MATERIAL, "
                        "WHILE MAINTAINING THE CORE MEANING OF THE TARGET SENTENCE. THEN, REGENERATE ALL OTHER SECTIONS BASED ON THE REPHRASED CONTENT. "
                        "PAY EXTREME ATTENTION TO THE REQUIRED OUTPUT FORMAT.\n---\n"
                     ) + original_prompt_text
                    await asyncio.sleep(retry_delay_seconds)
                    continue # Go to the next validation_attempt with the new copyright-specific prompt
                else: # Max validation retries reached for copyright
                    return f"// LLM_COPYRIGHT_VALIDATION_FAILED_MAX_RETRIES (Errors: {error_details_str}) FOR_SOURCE: {source_sentence_text}"
            
            # General validation failure (not copyright)
            if validation_attempt + 1 < max_validation_retries:
                # current_prompt_to_send for the next validation_attempt's API call is already set at the top of this loop
                # if validation_attempt > 0. If it was the first validation attempt (validation_attempt == 0),
                # the corrective prompt will be set at the start of the next iteration.
                await asyncio.sleep(retry_delay_seconds)
                continue # Go to the next validation_attempt
            else: # Max validation retries reached for general failure
                return f"// LLM_OUTPUT_VALIDATION_FAILED_MAX_RETRIES (Errors: {error_details_str}) FOR_SOURCE: {source_sentence_text}"
        
        # Should be unreachable if loop logic is correct
        return f"// LLM_VALIDATION_LOOP_UNEXPECTED_EXIT_FOR_SOURCE: {source_sentence_text}"


async def process_book_file_async(
    staged_file_path: Path, llm_output_dir: Path, model: genai.GenerativeModel, args: argparse.Namespace,
    num_context_sentences: int, item_limit: Optional[int],
    semaphore: asyncio.Semaphore
) -> Tuple[bool, bool, bool]: # (was_skipped, operation_successful, daily_limit_hit_flag)

    book_name_stem = staged_file_path.stem
    output_llm_file_path = llm_output_dir / f"{book_name_stem}.llm.txt"

    start_item_idx = 0
    is_resuming = False
    existing_content_blocks: List[str] = []

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
                elif len(lines_in_last_block) == 1:
                    penultimate_line_of_last_block = lines_in_last_block[0].strip()

                if penultimate_line_of_last_block == COMPLETION_MARKER_TEXT:
                    print(f"Skipping '{staged_file_path.name}': Found completion marker.")
                    return True, True, False
                elif penultimate_line_of_last_block.startswith(RESUME_MARKER_PREFIX) and \
                     penultimate_line_of_last_block.endswith("--- //"):
                    try:
                        index_str = penultimate_line_of_last_block[len(RESUME_MARKER_PREFIX):].split(" ")[0]
                        start_item_idx = int(index_str)
                        is_resuming = True
                        existing_content_blocks = raw_blocks_with_delimiters[:-1]
                        print(f"Resuming '{staged_file_path.name}' from source item index {start_item_idx}.")
                    except ValueError:
                        print(f"Warning: Could not parse resume index from '{output_llm_file_path.name}'. Reprocessing from start.", file=sys.stderr)
                        start_item_idx = 0; is_resuming = False; existing_content_blocks = []
                elif penultimate_line_of_last_block.startswith(OUTPUT_LIMITED_MARKER_PREFIX) and \
                     penultimate_line_of_last_block.endswith("--- //"):
                    try:
                        limit_in_marker_str = penultimate_line_of_last_block[len(OUTPUT_LIMITED_MARKER_PREFIX):].split("_ITEMS")[0]
                        limit_in_marker = int(limit_in_marker_str)
                        can_process_more = args.limit_items is None or args.limit_items > limit_in_marker
                        
                        if can_process_more:
                            print(f"Output limit marker found for '{staged_file_path.name}' (was {limit_in_marker}), but current limit ({args.limit_items}) allows more. Resuming.")
                            start_item_idx = len(raw_blocks_with_delimiters) -1 # Resume after the limited content + marker block
                            is_resuming = True
                            existing_content_blocks = raw_blocks_with_delimiters[:-1]
                        else:
                            print(f"Skipping '{staged_file_path.name}': Output previously limited to {limit_in_marker} items, current limit ({args.limit_items}) not greater.")
                            return True, True, False
                    except ValueError:
                        print(f"Warning: Could not parse limit from OUTPUT_LIMITED_MARKER in '{output_llm_file_path.name}'. Reprocessing.", file=sys.stderr)
                        start_item_idx = 0; is_resuming = False; existing_content_blocks = []
                elif args.limit_items is None: # No specific marker, no current item_limit, file exists
                     print(f"Warning: File '{output_llm_file_path.name}' exists without a clear completion/resume/limit marker. Reprocessing to ensure integrity.", file=sys.stderr)
                     start_item_idx = 0; is_resuming = False; existing_content_blocks = []
                # If args.limit_items is set and no marker, it will re-evaluate against the current limit from start_item_idx 0
        except Exception as e_read:
            print(f"Warning: Error reading or parsing existing output file '{output_llm_file_path.name}': {e_read}. Reprocessing from start.", file=sys.stderr)
            start_item_idx = 0; is_resuming = False; existing_content_blocks = []

    print(f"Processing '{staged_file_path.name}' (effective start source item index: {start_item_idx})...")
    try:
        with open(staged_file_path, 'r', encoding='utf-8') as f_in:
            raw_lines_from_staged_file = f_in.readlines()
    except Exception as e:
        print(f"Error reading input file '{staged_file_path.name}': {e}", file=sys.stderr)
        return False, False, False

    all_items: List[Dict[str, Any]] = []
    for orig_idx, line_raw in enumerate(raw_lines_from_staged_file):
        line = line_raw.strip()
        chapter_match = CHAPTER_MARKER_REGEX.match(line)
        if chapter_match:
            all_items.append({"type": "marker", "text": chapter_match.group(1).strip(), "original_idx_in_file": orig_idx})
        else:
            sentence_match = SENTENCE_LINE_REGEX.match(line)
            if sentence_match:
                all_items.append({"type": "sentence", "text": sentence_match.group(1).strip(), "original_idx_in_file": orig_idx})

    if not all_items:
        if args.limit_items is None or args.limit_items >= 0 :
             try:
                llm_output_dir.mkdir(parents=True, exist_ok=True)
                with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
                    f_out.write(f"// NO_PROCESSABLE_ITEMS_IN_SOURCE_FILE\n{END_SENTENCE_MARKER_TEXT}\n")
                print(f"Wrote placeholder output file '{output_llm_file_path.name}'.")
             except Exception as e_write: return False, False, False
        return False, True, False

    items_to_process_this_run_slice: List[Dict[str, Any]] = []
    target_total_items_in_output_file = len(all_items)
    if args.limit_items is not None:
        target_total_items_in_output_file = min(args.limit_items, len(all_items))

    num_existing_items_count = "".join(existing_content_blocks).count(END_SENTENCE_MARKER_TEXT)

    if num_existing_items_count >= target_total_items_in_output_file:
        print(f"File '{staged_file_path.name}' already has {num_existing_items_count} items, meeting target of {target_total_items_in_output_file}. Finalizing.")
        if is_resuming: # Finalize by removing previous resume/limit marker if it was partial
            try:
                with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
                    f_out.write("".join(existing_content_blocks))
                    # Add correct final marker based on why it's "complete now"
                    if num_existing_items_count >= len(all_items):
                        f_out.write(f"{COMPLETION_MARKER_TEXT}\n{END_SENTENCE_MARKER_TEXT}\n")
                    elif args.limit_items is not None and num_existing_items_count >= args.limit_items:
                         f_out.write(f"{OUTPUT_LIMITED_MARKER_PREFIX}{args.limit_items}_ITEMS --- //\n{END_SENTENCE_MARKER_TEXT}\n")
                print(f"Finalized '{output_llm_file_path.name}'.")
            except Exception as e_fin: print(f"Error finalizing {output_llm_file_path.name}: {e_fin}")
        return False, True, False

    num_new_items_to_process_this_run = target_total_items_in_output_file - num_existing_items_count
    end_idx_for_slice_in_all_items = min(len(all_items), start_item_idx + num_new_items_to_process_this_run)

    if start_item_idx < end_idx_for_slice_in_all_items:
        items_to_process_this_run_slice = all_items[start_item_idx:end_idx_for_slice_in_all_items]
    
    if not items_to_process_this_run_slice:
        print(f"No new items to process for '{staged_file_path.name}' (start_src_idx: {start_item_idx}, end_slice_idx: {end_idx_for_slice_in_all_items}, existing: {num_existing_items_count}, target_total: {target_total_items_in_output_file}).")
        if is_resuming: # Finalize if it was partial and now meets criteria
            try:
                with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
                    f_out.write("".join(existing_content_blocks))
                    if num_existing_items_count >= len(all_items):
                        f_out.write(f"{COMPLETION_MARKER_TEXT}\n{END_SENTENCE_MARKER_TEXT}\n")
                    elif args.limit_items is not None and num_existing_items_count >= args.limit_items:
                        f_out.write(f"{OUTPUT_LIMITED_MARKER_PREFIX}{args.limit_items}_ITEMS --- //\n{END_SENTENCE_MARKER_TEXT}\n")
                print(f"Finalized '{output_llm_file_path.name}' as no new items needed.")
            except Exception as e_fin: print(f"Error finalizing {output_llm_file_path.name}: {e_fin}")
        return False, True, False

    llm_tasks_coroutines: List[asyncio.Task] = []
    ordered_task_results_placeholders: List[Optional[str]] = [None] * len(items_to_process_this_run_slice)
    coroutine_to_placeholder_idx_map: Dict[int, int] = {}

    for run_slice_idx, item_data in enumerate(items_to_process_this_run_slice):
        original_item_idx_in_all_items = start_item_idx + run_slice_idx
        if item_data["type"] == "marker":
            ordered_task_results_placeholders[run_slice_idx] = f"CHAPTER_MARKER_DIRECT:: {item_data['text']}"
        elif item_data["type"] == "sentence":
            preceding_context_str = "[NO PRECEDING CONTEXT]"
            pre_sent_texts = [all_items[i]["text"] for i in range(max(0, original_item_idx_in_all_items - num_context_sentences), original_item_idx_in_all_items) if all_items[i]["type"] == "sentence"]
            if pre_sent_texts: preceding_context_str = "\n".join(pre_sent_texts)
            succeeding_context_str = "[NO SUCCEEDING CONTEXT]"
            suc_sent_texts = [all_items[i]["text"] for i in range(original_item_idx_in_all_items + 1, min(len(all_items), original_item_idx_in_all_items + 1 + num_context_sentences)) if all_items[i]["type"] == "sentence"]
            if suc_sent_texts: succeeding_context_str = "\n".join(suc_sent_texts)
            
            task_coro = process_sentence_with_llm_async(
                item_data["text"], preceding_context_str, succeeding_context_str,
                model, args.max_api_retries, args.max_validation_retries,
                DEFAULT_RETRY_DELAY_SECONDS, semaphore, original_item_idx_in_all_items + 1
            )
            llm_tasks_coroutines.append(asyncio.create_task(task_coro))
            coroutine_to_placeholder_idx_map[len(llm_tasks_coroutines)-1] = run_slice_idx

    daily_limit_hit_for_this_book = False
    first_failed_item_original_idx = -1
    llm_calls_made_this_run = 0

    if llm_tasks_coroutines:
        print(f"  Starting {len(llm_tasks_coroutines)} LLM tasks for '{book_name_stem}' (source items {start_item_idx} to {end_idx_for_slice_in_all_items-1})...")
        results_from_coroutines = await asyncio.gather(*llm_tasks_coroutines, return_exceptions=True)
        print(f"  Finished {len(llm_tasks_coroutines)} LLM tasks for '{book_name_stem}'.")
        llm_calls_made_this_run = len(llm_tasks_coroutines)

        for i, result_or_exc in enumerate(results_from_coroutines):
            placeholder_idx = coroutine_to_placeholder_idx_map[i]
            original_idx_of_item_in_all = start_item_idx + placeholder_idx
            
            if daily_limit_hit_for_this_book and first_failed_item_original_idx != -1 and original_idx_of_item_in_all >= first_failed_item_original_idx:
                ordered_task_results_placeholders[placeholder_idx] = None # Mark as not processed due to prior fatal error
                continue

            source_text_for_log = items_to_process_this_run_slice[placeholder_idx]["text"]
            if result_or_exc == FATAL_QUOTA_ERROR_SENTINEL:
                daily_limit_hit_for_this_book = True
                if first_failed_item_original_idx == -1:
                    first_failed_item_original_idx = original_idx_of_item_in_all
                ordered_task_results_placeholders[placeholder_idx] = None
                print(f"  FATAL QUOTA recorded for source item {original_idx_of_item_in_all+1} of '{book_name_stem}'.")
            elif isinstance(result_or_exc, Exception):
                ordered_task_results_placeholders[placeholder_idx] = f"// LLM_TASK_EXCEPTION ({type(result_or_exc).__name__}: {result_or_exc}) FOR_SOURCE: {source_text_for_log}"
            elif isinstance(result_or_exc, str):
                ordered_task_results_placeholders[placeholder_idx] = result_or_exc
            else:
                ordered_task_results_placeholders[placeholder_idx] = f"// LLM_TASK_UNEXPECTED_TYPE ({type(result_or_exc)}) FOR_SOURCE: {source_text_for_log}"

    newly_processed_blocks_this_run: List[str] = []
    num_successfully_processed_items_this_run = 0

    for run_slice_idx, content_or_placeholder in enumerate(ordered_task_results_placeholders):
        original_item_idx_overall = start_item_idx + run_slice_idx
        if daily_limit_hit_for_this_book and \
           first_failed_item_original_idx != -1 and \
           original_item_idx_overall >= first_failed_item_original_idx:
            break 

        if content_or_placeholder is not None:
            newly_processed_blocks_this_run.append(content_or_placeholder.strip() + f"\n{END_SENTENCE_MARKER_TEXT}\n")
            if not content_or_placeholder.startswith("//") and not content_or_placeholder.startswith("CHAPTER_MARKER_DIRECT::"): # Crude check for "successful" item
                 if not content_or_placeholder.startswith(COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX): # Copyright placeholder isn't a success
                    num_successfully_processed_items_this_run += 1
            elif content_or_placeholder.startswith("CHAPTER_MARKER_DIRECT::"): # Count markers
                num_successfully_processed_items_this_run +=1


    final_output_content = "".join(existing_content_blocks) + "".join(newly_processed_blocks_this_run)
    total_items_now_in_file = num_existing_items_count + num_successfully_processed_items_this_run # Use the refined success count

    if daily_limit_hit_for_this_book:
        if first_failed_item_original_idx != -1 and first_failed_item_original_idx < len(all_items):
            final_output_content += f"{RESUME_MARKER_PREFIX}{first_failed_item_original_idx} --- //\n{END_SENTENCE_MARKER_TEXT}\n"
            print(f"  Partial file for '{book_name_stem}' will be saved. Resume from source item {first_failed_item_original_idx} next time.")
    elif total_items_now_in_file >= len(all_items):
        final_output_content += f"{COMPLETION_MARKER_TEXT}\n{END_SENTENCE_MARKER_TEXT}\n"
        print(f"  Marking '{book_name_stem}' as fully processed.")
    elif args.limit_items is not None and total_items_now_in_file >= args.limit_items:
        final_output_content += f"{OUTPUT_LIMITED_MARKER_PREFIX}{args.limit_items}_ITEMS --- //\n{END_SENTENCE_MARKER_TEXT}\n"
        print(f"  Marking '{book_name_stem}' as limited to {args.limit_items} items.")
    
    try:
        llm_output_dir.mkdir(parents=True, exist_ok=True)
        with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
            f_out.write(final_output_content)
        
        log_msg_blocks = f"Wrote {len(newly_processed_blocks_this_run)} blocks from this run " \
                         f"(including {num_successfully_processed_items_this_run} successfully processed source items/markers; " \
                         f"{llm_calls_made_this_run} LLM calls attempted) to '{output_llm_file_path.name}'"
        print(log_msg_blocks)
        return False, True, daily_limit_hit_for_this_book
    except Exception as e:
        print(f"Error writing output file '{output_llm_file_path.name}': {e}", file=sys.stderr)
        return False, False, daily_limit_hit_for_this_book

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
        model = genai.GenerativeModel(MODEL_NAME)
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
            staged_file, llm_output_dir, model, args,
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