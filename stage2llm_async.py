# stage2llm_async.py
# Script Version: 1.0.0 (Added Claude 3 Haiku support, dual provider)

import os
from pathlib import Path
import re
import time # Still used for synchronous delays
import argparse
import sys
from typing import Optional, List, Dict, Any, Tuple
from dotenv import load_dotenv
import asyncio
import importlib.metadata # For getting package version (for SDKs)
import json # For potential future use, not directly in this script version

# --- Enhanced Debug Logging Setup ---
# (If you want more verbose logging from libraries, uncomment and adapt set_library_log_levels)
# logging.basicConfig(format='%(asctime)s - %(levelname)s - %(name)s - %(message)s')
# def set_library_log_levels(script_log_level_str: str):
#     level = logging.DEBUG if script_log_level_str.upper() == "DEBUG" else logging.WARNING
#     libraries_to_set = [
#         "httpx",
#         "google.generativeai._base_client",
#         "google.genai._base_client",
#         "google.genai.client",
#         "anthropic", # Add Anthropic library
#     ]
#     for lib_name in libraries_to_set:
#         logging.getLogger(lib_name).setLevel(level)

SCRIPT_VERSION = "1.0.0"

# Attempt to import necessary libraries
try:
    import google.generativeai as genai
except ImportError:
    print("ERROR: Google GenAI (Gemini) library not found. `pip install google-generativeai`")
    genai = None

try:
    import anthropic
except ImportError:
    print("ERROR: Anthropic (Claude) library not found. `pip install anthropic`")
    anthropic = None

# --- Import from the validator module ---
try:
    from llm_output_validator import validate_llm_block
except ImportError:
    print("ERROR: Could not import 'validate_llm_block' from 'llm_output_validator.py'.")
    print("Please ensure 'llm_output_validator.py' is in the same directory or accessible in PYTHONPATH.")
    sys.exit(1)

# --- Configuration ---
DEFAULT_LLM_PROVIDER = "gemini" # "gemini" or "claude"

# Gemini Defaults
DEFAULT_GEMINI_MODEL_NAME = "models/gemini-1.5-pro-preview-05-06" # Or your preferred Gemini text model

# Claude Defaults
DEFAULT_CLAUDE_MODEL_NAME = "claude-3-haiku-20240307"
DEFAULT_CLAUDE_MAX_TOKENS_OUTPUT = 4000 # Max tokens for Claude's output

DEFAULT_STAGED_DIR_NAME = "Staged"
DEFAULT_LLM_OUTPUT_DIR_NAME = "stage"
DEFAULT_MAX_API_RETRIES = 3
DEFAULT_MAX_VALIDATION_RETRIES = 5
DEFAULT_RETRY_DELAY_SECONDS = 7
DEFAULT_CONCURRENT_REQUESTS = 20

# Regex to parse input lines
SENTENCE_LINE_REGEX = re.compile(r"^{S\d+:\s*(.*)}$")
CHAPTER_MARKER_REGEX = re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")

# --- Markers and Sentinels ---
END_SENTENCE_MARKER_TEXT = "END_SENTENCE"
FATAL_QUOTA_ERROR_SENTINEL = "__FATAL_QUOTA_ERROR_DETECTED_STOP_BOOK__"
COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX = "// LLM_COPYRIGHT_DETECTED_REPHRASE_ATTEMPT_FOR_SOURCE: "
RESUME_MARKER_PREFIX = "// --- PARTIAL_FILE_RESUME_NEXT_ITEM_INDEX: "
OUTPUT_LIMITED_MARKER_PREFIX = "// --- OUTPUT_LIMITED_TO_FIRST_"
COMPLETION_MARKER_TEXT = "// --- BOOK_FULLY_PROCESSED --- //"


# --- LLM_PROMPT_TEMPLATE (User-facing part for Claude, full prompt for Gemini) ---
LLM_PROMPT_TEMPLATE = """
You are an expert linguist and data formatter.
Your primary task is to process the "TARGET SENTENCE" provided below.
Use the "PRECEDING CONTEXT" and "SUCCEEDING CONTEXT" (if available) ONLY for disambiguation, pronoun resolution, and to understand the narrative flow related to the TARGET SENTENCE.
DO NOT generate full output blocks for the context sentences.
The output block you generate MUST correspond ONLY to the "TARGET SENTENCE".

Overall Goal for Simplification of SimE and SimS:
The primary goal for both "SimE" (Simple English) and "SimS" (Simple Spanish) is to use vocabulary and sentence structures appropriate for an **absolute beginner learner (e.g., a first-grade reading level, or a 6-7 year old child learning to read or learning a second language from scratch).**
- For **SimE**: Extensively paraphrase the TARGET SENTENCE using very common, high-frequency English words and simple sentence structures. Retain the core original meaning. SimE may consist of one or more simple sentences if needed to simplify the TARGET SENTENCE.
- For **SimS**: Provide a direct, simple Spanish translation of the generated SimE. SimS may also consist of one or more simple sentences, mirroring SimE.
This SimE version will be the basis for SimS_Segments and DIGLOT_MAP word selection.

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
SimS:: [Simple Spanish translation of the SimE below. This SimS block may contain one or more simple sentences.]
SimE:: [Simple English version of the TARGET SENTENCE, adhering to the first-grade reading level guidelines as described above. This SimE block may contain one or more simple sentences.]

SimS_Segments:: // Critically important: Segment the SimS text above into **multiple smaller, natural phrases, clauses, or very short sentences**.
// If SimS contains multiple sentences, apply this segmentation logic to each of those sentences.
// The S-numbering (S1, S2, S3...) should be continuous across all segments derived from the entire SimS block.
//
// The primary goal of segmentation is to create meaningful chunks for pedagogical purposes, such as targeted vocabulary substitution or temporary replacement of a Spanish chunk with its English equivalent if it contains new vocabulary for the learner.
//
// Segmentation Guidelines:
//   - Aim to break each sentence within SimS into 2-5 meaningful chunks. Very long SimS sentences could naturally have more segments originating from them.
//   - Very short SimS sentences (e.g., one, two, or three words like "Run!" or "Yes, I did.") generally do not need to be broken up further and can remain a single segment.
//   - Think about how the sentence would be spoken with natural pauses. Good breaking points often occur:
//       - Separating the main subject (e.g., "The cat") from its verb/action (e.g., "ran fast").
//       - Before or after prepositional phrases (e.g., "under the table", "in the house", "with her friend").
//       - Around coordinating conjunctions (e.g., "and", "but", "or", "so", "yet", "for"), separating the connected independent clauses or major ideas.
//           - If a conjunction connects two parts that could stand as simpler sentences, each of those parts can often be a segment or be further segmented if they are complex enough.
//       - After introductory phrases or clauses that set time, place, or condition (e.g., "In the morning,", "Later,", "If it rains,").
//   - Each segment should correspond to a similar conceptual phrase in the SimE text for later alignment in PHRASE_ALIGN.
//
S1([First phrase from first SimS sentence])
S2([Second phrase from first SimS sentence])
// ... (Continue for all sentences in SimS, numbering segments S3, S4, etc. continuously)

PHRASE_ALIGN:: // Align each SimS_Segment to its corresponding span in AdvS and its corresponding phrase/clause in SimE.
// The SimE Phrase here MUST be the complete English text chunk from the full SimE:: block that corresponds to the SimS_Segment.
// **Crucially, if the SimE chunk represents a new sentence or a clause that would naturally start with a subject pronoun (e.g., He, She, It, They) in the full SimE:: text, that subject pronoun MUST be included in this SimE Phrase.**
// The goal is that if all SimS_Segments were replaced by their aligned SimE Phrases from this section, they would seamlessly reconstruct the full SimE:: text (with correct punctuation and capitalization to be handled by the generator tool later).
// Ensure the SimE phrase here matches the conceptual chunk of the SimS_Segment.
S1 ~ [AdvS Span corresponding to S1 from AdvS above] ~ [Complete SimE Phrase/Clause 1 text, including leading subject pronoun if applicable]
S2 ~ [AdvS Span corresponding to S2 from AdvS above] ~ [Complete SimE Phrase/Clause 2 text, including leading subject pronoun if applicable]
// ... (Align all SimS_Segments S3, S4, etc.)

SimSL:: // Phrasal: Lemmas for SimS segments of the TARGET SENTENCE. Strictly lemmatized per rules.
// Provide lemmas for each SimS_Segment defined above (S1, S2, S3, S4, etc.).
S1 :: [lemma1 lemma2 ...]
S2 :: [lemmaA lemmaB ...]
// ...

AdvSL:: [flat_list_adv_lemma1 flat_list_adv_lemma2 ...] // Flat list: **Ensure ALL** non-proper-noun AdvS lemmas for the TARGET SENTENCE are included. Strictly lemmatized per rules.

DIGLOT_MAP:: // For Level 4 generation. Map words from the generated SimE phrases to their Spanish equivalents.
// Map words for each segment S1, S2, S3, S4, etc.
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
// For Proper Nouns (e.g., John, London): these MUST be included in DIGLOT_MAP with ViabilityFlag(N). The SpaLemma and ExactSpaForm should be the Proper Noun itself. Example: `John->John(John)(N)`.
// Ensure every entry strictly follows the `EngWord->SpaLemma(ExactSpaForm)(ViabilityFlag)` pattern. For simple words like 'and', the SpaLemma and ExactSpaForm might be the same. Example: `and->y(y)(Y)`.
// If a specific SimS_Segment S<n> (from SimS_Segments section) results in no words that can or should be mapped in DIGLOT_MAP for that S<n>, output its line as `S<n> ::` (i.e., the S-ID followed by `::` and nothing else on that line).
S1 :: [EngWord1_from_SimE_S1_phrase]->[SpaLemma1]([ExactSpaForm1])([ViabilityFlagY/N]) | [EngWord2_from_SimE_S1_phrase]->[SpaLemma2]([ExactSpaForm2])([ViabilityFlagY/N])
S2 :: [EngWordA_from_SimE_S2_phrase]->[SpaLemmaA]([ExactSpaFormA])([ViabilityFlagY/N])
// ... (Continue for S3, S4, etc.)

// LOCKED_PHRASE :: [Sx Sy ...] (Optional, only if certain SimS_Segments from TARGET SENTENCE should always stay together)
// (The LLM's output for the TARGET SENTENCE block should conclude after the last section like DIGLOT_MAP or LOCKED_PHRASE. Do not add an "END_SENTENCE" line yourself.)
---
Ensure all sections are present for the TARGET SENTENCE.
IMPORTANT: Your generated output for a single TARGET SENTENCE should be one continuous block of text containing all the required '::' sections. DO NOT include the literal string '{END_SENTENCE_MARKER_TEXT}' anywhere within this block of text you generate; the '{END_SENTENCE_MARKER_TEXT}' marker is only used externally by other scripts to separate distinct, fully-formed blocks in the final combined file.
---
EXAMPLES:

---
EXAMPLE 1: Standard sentence processing with detailed segmentation.
SimE and SimS here contain two conceptual simple sentences. Segments S1 is from the first, S2-S4 are from the second.
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
S2(Un hombre solo)
S3(con mucho dinero)
S4(necesita una esposa.)
PHRASE_ALIGN::
S1 ~ Es una verdad universalmente reconocida ~ It is a very known thing.
S2 ~ que un hombre soltero ~ A single man
S3 ~ poseedor de una gran fortuna ~ with much money
S4 ~ necesita una esposa. ~ needs a wife.
SimSL::
S1 :: ser uno cosa muy sabido
S2 :: uno hombre solo
S3 :: con mucho dinero
S4 :: necesitar uno esposa
AdvSL:: ser uno verdad universalmente reconocido que uno hombre soltero poseedor de uno grande fortuna necesitar uno esposa
DIGLOT_MAP::
S1 :: It->él(él)(N) | is->ser(es)(Y) | a->uno(una)(Y) | very->mucho(muy)(Y) | known->sabido(sabida)(Y) | thing->cosa(cosa)(Y)
S2 :: A->uno(Un)(Y) | single->solo(solo)(Y) | man->hombre(hombre)(Y)
S3 :: with->con(con)(Y) | much->mucho(mucho)(Y) | money->dinero(dinero)(Y)
S4 :: needs->necesitar(necesita)(Y) | a->uno(una)(Y) | wife->esposa(esposa)(Y)

---
EXAMPLE 2: Complex sentence becoming multiple simpler sentences in SimE/SimS, then segmented.
SimE and SimS here contain three conceptual simple sentences.
PRECEDING CONTEXT:
The dog barked loudly.
---
TARGET SENTENCE (This is the English sentence from the source text you need to process):
"Fearing for its life, the small cat quickly ran beneath the large ornate table."
---
SUCCEEDING CONTEXT:
It stayed there for a long time.
---
EXPECTED OUTPUT (for "Fearing for its life..."):
AdvS:: Temiendo por su vida, el pequeño gato corrió rápidamente debajo de la gran mesa ornamentada.
SimS:: El gato tuvo miedo. Corrió rápido. Fue debajo de la mesa.
SimE:: The cat was afraid. It ran fast. It went under the table.
SimS_Segments::
S1(El gato)
S2(tuvo miedo.)
S3(Corrió rápido.)
S4(Fue debajo de la mesa.)
PHRASE_ALIGN::
S1 ~ El pequeño gato ~ The cat was afraid. 
S2 ~ temiendo por su vida ~ // This AdvS phrase contributes to "The cat was afraid" concept.
S3 ~ corrió rápidamente ~ It ran fast.
S4 ~ debajo de la gran mesa ornamentada ~ It went under the table.
// Note: For S1 & S2 in PHRASE_ALIGN, the SimE "The cat was afraid." covers both SimS_Segments S1("El gato") and S2("tuvo miedo.").
// The alignment might sometimes group SimS_Segments if they collectively map to a single SimE sentence.
// A more granular PHRASE_ALIGN for S1/S2 could be:
// S1 ~ El pequeño gato ~ The cat
// S2 ~ temiendo por su vida ~ was afraid.
// For this example, we'll show the slightly grouped version, but ensure the LLM understands the SimE phrase in PHRASE_ALIGN should be reconstructable.
// Let's refine this example for maximum clarity on PHRASE_ALIGN for this case:
// SimE "The cat was afraid." corresponds to S1+S2. SimE "It ran fast." to S3. SimE "It went under the table." to S4.
// The PHRASE_ALIGN needs to reflect this.
// Corrected PHRASE_ALIGN for Example 2:
PHRASE_ALIGN::
S1 ~ El pequeño gato ~ The cat 
S2 ~ temiendo por su vida ~ was afraid. 
// S1+S2 SimE Phrase: "The cat was afraid."
S3 ~ corrió rápidamente ~ It ran fast.
S4 ~ debajo de la gran mesa ornamentada ~ It went under the table.
SimSL::
S1 :: el gato
S2 :: tener miedo
S3 :: correr rápido
S4 :: ir debajo de el mesa
AdvSL:: temer por su vida el pequeño gato correr rápidamente debajo de el grande mesa ornamentado
DIGLOT_MAP::
S1 :: The->el(el)(Y) | cat->gato(gato)(Y)
S2 :: was->tener(tuvo)(Y) | afraid->miedo(miedo)(Y)
S3 :: It->él(él)(N) | ran->correr(corrió)(Y) | fast->rápido(rápido)(Y)
S4 :: It->él(él)(N) | went->ir(fue)(Y) | under->debajo(debajo)(Y) | the->el(la)(Y) | table->mesa(mesa)(Y)
// LOCKED_PHRASE :: [S1 S2] // Example: "El gato tuvo miedo." should stick together.

---
EXAMPLE 3: Very short sentence requiring minimal further segmentation.
SimE and SimS here contain two conceptual simple sentences.
PRECEDING CONTEXT:
Someone asked a question.
---
TARGET SENTENCE (This is the English sentence from the source text you need to process):
"No, I didn't!"
---
SUCCEEDING CONTEXT:
She looked surprised.
---
EXPECTED OUTPUT (for "No, I didn't!"):
AdvS:: ¡No, no lo hice!
SimS:: No. Yo no lo hice.
SimE:: No. I did not do it.
SimS_Segments::
S1(No.)
S2(Yo no lo hice.)
PHRASE_ALIGN::
S1 ~ ¡No, ~ No.
S2 ~ no lo hice! ~ I did not do it.
SimSL::
S1 :: no
S2 :: yo no lo hacer
AdvSL:: no no lo hacer
DIGLOT_MAP::
S1 :: No->no(No)(Y)
S2 :: I->yo(Yo)(Y) | did->hacer(hice)(Y) | not->no(no)(Y) | do->hacer(lo)(Y) | it->él(él)(N)

---
EXAMPLE 4: Single imperative verb (minimal segmentation).
SimE and SimS here contain one conceptual simple sentence.
PRECEDING CONTEXT:
There was danger.
---
TARGET SENTENCE (This is the English sentence from the source text you need to process):
"Run!"
---
SUCCEEDING CONTEXT:
He ran as fast as he could.
---
EXPECTED OUTPUT (for "Run!"):
AdvS:: ¡Corre!
SimS:: ¡Corre!
SimE:: Run!
SimS_Segments::
S1(¡Corre!)
PHRASE_ALIGN::
S1 ~ ¡Corre! ~ Run!
SimSL::
S1 :: correr
AdvSL:: correr
DIGLOT_MAP::
S1 :: Run->correr(Corre)(Y)
// (The LLM's output for the TARGET SENTENCE block for this example would conclude here.)
// (IMPORTANT INSTRUCTION TO LLM ASSISTANT: Your output for the TARGET SENTENCE block stops after the last section. Do not add '{END_SENTENCE_MARKER_TEXT}' yourself.)
"""

# --- CLAUDE_SYSTEM_PROMPT ---
CLAUDE_SYSTEM_PROMPT = """You are an expert linguist and data formatter.
Your primary task is to process the "TARGET SENTENCE" provided in the user message.
Use the "PRECEDING CONTEXT" and "SUCCEEDING CONTEXT" (if available, also in the user message) ONLY for disambiguation, pronoun resolution, and to understand the narrative flow related to the TARGET SENTENCE.
DO NOT generate full output blocks for the context sentences.
The output block you generate MUST correspond ONLY to the "TARGET SENTENCE".

Overall Goal for Simplification of SimE and SimS:
For both the "SimE" (Simple English) and "SimS" (Simple Spanish) outputs, the primary goal is to use vocabulary and sentence structures appropriate for an **absolute beginner learner (e.g., a first-grade reading level, or a 6-7 year old child learning to read or learning a second language from scratch).** Prioritize very common, high-frequency words and simple sentence structures. The SimE and SimS may themselves consist of one or more simple sentences if that aids simplification of the original TARGET SENTENCE. You will be given detailed formatting instructions for all output sections in the user message.
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
    preceding_context: str,
    succeeding_context: str,
    llm_client_or_model_obj: Any, # Gemini: GenerativeModel instance; Claude: AsyncAnthropic instance
    llm_provider: str, # "gemini" or "claude"
    model_name_to_use_in_api_call: str, # Actual model name string for the API
    max_api_retries: int,
    max_validation_retries: int,
    retry_delay_seconds: int,
    semaphore: asyncio.Semaphore,
    item_idx_for_log: int,
    llm_prompt_template_str: str, # The main template (becomes user prompt for Claude)
    claude_system_prompt_str: str, # System prompt for Claude
    claude_max_tokens: int,
    is_copyright_retry_attempt: bool = False
) -> str:

    # This is the base "user-facing" part of the prompt for both providers
    current_original_prompt_text_for_sentence = llm_prompt_template_str.format(
        preceding_context=preceding_context,
        source_sentence=source_sentence_text,
        succeeding_context=succeeding_context,
        END_SENTENCE_MARKER_TEXT=END_SENTENCE_MARKER_TEXT
    )
    
    # Initialize current_prompt_to_send based on provider for the first API call attempt in the validation loop
    if llm_provider == "claude":
        # For Claude, corrective instructions prepend the user message part
        current_prompt_user_message_part = current_original_prompt_text_for_sentence
    else: # gemini
        # For Gemini, corrective instructions prepend the whole prompt
        current_prompt_to_send_to_api = current_original_prompt_text_for_sentence


    last_validation_error_details_str = ""
    # Gemini specific settings
    gemini_safety_settings = [
        {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE"},
        {"category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE"},
    ]
    # Common generation config (temperature can be used by both)
    generation_config_params = {"temperature": 0.75}


    async with semaphore:
        for validation_attempt in range(max_validation_retries):
            # Construct corrective prompts if this is a retry
            corrective_instruction_general = ""
            corrective_instruction_copyright = ""

            if validation_attempt > 0: # This is a validation retry
                corrective_instruction_general = (
                    "PREVIOUS ATTEMPT FAILED VALIDATION. PLEASE PAY EXTREME ATTENTION TO THE REQUIRED OUTPUT FORMAT. "
                    f"Specifically, ensure all sections are present and correctly formatted. Previous errors: {last_validation_error_details_str}\n"
                    "Re-generate the entire block for the TARGET SENTENCE ensuring adherence to the format.\n"
                    "ADDITIONALLY, before the 'AdvS::' line, please include a few lines starting with '// DEBUG:' "
                    "explaining any specific difficulties or ambiguities you encountered with the previous attempt or the source sentence "
                    "that might have led to the errors.\n"
                    "---\n"
                )
            
            if is_copyright_retry_attempt and validation_attempt == 0: # First attempt of this model *because* of copyright
                 corrective_instruction_copyright = (
                    "THE SYSTEM BELIEVES A PREVIOUS ATTEMPT (POSSIBLY WITH A DIFFERENT MODEL) "
                    "WAS BLOCKED DUE TO POTENTIAL COPYRIGHT/RECITATION FOR THE TARGET SENTENCE. "
                    "PLEASE REPHRASE THE SimE, SimS, AND AdvS OUTPUTS SIGNIFICANTLY TO AVOID RESEMBLANCE TO COPYRIGHTED MATERIAL, "
                    "WHILE MAINTAINING THE CORE MEANING OF THE TARGET SENTENCE. THEN, REGENERATE ALL OTHER SECTIONS BASED ON THE REPHRASED CONTENT. "
                    "PAY EXTREME ATTENTION TO THE REQUIRED OUTPUT FORMAT.\n"
                    "ADDITIONALLY, before the 'AdvS::' line, please include a few lines starting with '// DEBUG:' "
                    "explaining what part of the original sentence might have triggered the copyright/recitation flag, if you can identify it. "
                    "Then proceed with the rephrased full block.\n"
                    "---\n"
                )

            # Prepare the actual prompt to send for this validation attempt
            if llm_provider == "claude":
                current_prompt_user_message_part = current_original_prompt_text_for_sentence
                if corrective_instruction_copyright: # Takes precedence for the first attempt if it's a copyright retry
                    current_prompt_user_message_part = corrective_instruction_copyright + current_prompt_user_message_part
                elif corrective_instruction_general: # For subsequent validation retries
                    current_prompt_user_message_part = corrective_instruction_general + current_prompt_user_message_part
                # system_prompt_for_claude remains claude_system_prompt_str
            else: # gemini
                current_prompt_to_send_to_api = current_original_prompt_text_for_sentence
                if corrective_instruction_copyright:
                    current_prompt_to_send_to_api = corrective_instruction_copyright + current_prompt_to_send_to_api
                elif corrective_instruction_general:
                    current_prompt_to_send_to_api = corrective_instruction_general + current_prompt_to_send_to_api
            
            _raw_llm_output_core_this_api_cycle = None
            api_call_successful_flag = False

            for api_attempt in range(max_api_retries):
                try:
                    if llm_provider == "gemini":
                        response = await llm_client_or_model_obj.generate_content_async(
                            current_prompt_to_send_to_api,
                            safety_settings=gemini_safety_settings,
                            generation_config=genai.types.GenerationConfig(**generation_config_params)
                        )
                        if response.parts:
                            _raw_llm_output_core_this_api_cycle = response.text.strip()
                            api_call_successful_flag = True
                            if response.prompt_feedback and response.prompt_feedback.block_reason:
                                reason_str = str(response.prompt_feedback.block_reason).lower()
                                reason_msg = response.prompt_feedback.block_reason_message or reason_str
                                if "recitation" in reason_str or response.prompt_feedback.block_reason == 4: # 4 is BlockReason.SAFETY (often for recitation)
                                    print(f"  LLM API Warning (Gemini, Item {item_idx_for_log}): Potential copyright/recitation block. Reason: {reason_msg}", file=sys.stderr)
                                    _raw_llm_output_core_this_api_cycle = f"{COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX}{source_sentence_text}"
                            break # API call success
                        else: # No parts, but response object exists (likely blocked)
                            reason = "Unknown reason, empty parts list in response."
                            is_fatal_quota = False
                            if response.prompt_feedback and response.prompt_feedback.block_reason:
                                reason_str = str(response.prompt_feedback.block_reason).lower()
                                reason_msg = response.prompt_feedback.block_reason_message or reason_str
                                reason = reason_msg
                                if any(kw in reason_str for kw in ["quota", "limit", "billing", "exceeded", "rate_limit_exceeded"]): # Gemini specific check
                                    is_fatal_quota = True
                                elif "recitation" in reason_str or response.prompt_feedback.block_reason == 4:
                                    print(f"  LLM API Warning (Gemini, Item {item_idx_for_log}): Potential copyright/recitation block (no parts). Reason: {reason}", file=sys.stderr)
                                    _raw_llm_output_core_this_api_cycle = f"{COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX}{source_sentence_text}"
                                    api_call_successful_flag = True; break
                            print(f"  LLM Warning (Gemini, API Attempt {api_attempt+1}) for item {item_idx_for_log}: Blocked or empty parts. Reason: {reason}", file=sys.stderr)
                            if is_fatal_quota: return FATAL_QUOTA_ERROR_SENTINEL
                            if not api_call_successful_flag: # If not already set to copyright placeholder
                                _raw_llm_output_core_this_api_cycle = f"// LLM_BLOCKED_NO_PARTS (Gemini, Reason: {reason}) FOR_SOURCE: {source_sentence_text}"
                            break # Handled block/empty parts

                    elif llm_provider == "claude":
                        messages_for_claude = [{"role": "user", "content": current_prompt_user_message_part}]
                        api_response = await llm_client_or_model_obj.messages.create(
                            model=model_name_to_use_in_api_call,
                            max_tokens=claude_max_tokens,
                            system=claude_system_prompt_str,
                            messages=messages_for_claude,
                            temperature=generation_config_params.get("temperature", 0.7)
                        )
                        if api_response.content and api_response.content[0].text:
                            _raw_llm_output_core_this_api_cycle = api_response.content[0].text.strip()
                            api_call_successful_flag = True
                            if api_response.stop_reason == "max_tokens":
                                print(f"  LLM API Warning (Claude, Item {item_idx_for_log}): Output truncated due to max_tokens ({claude_max_tokens}).", file=sys.stderr)
                            # Claude's copyright/safety is usually via 400 error, handled in exceptions
                        else: # Should not happen if no exception
                            _raw_llm_output_core_this_api_cycle = f"// LLM_EMPTY_CONTENT_UNEXPECTED (Claude) FOR_SOURCE: {source_sentence_text}"
                        break # API call success or handled empty

                except (anthropic.APIConnectionError, anthropic.InternalServerError) if llm_provider == "claude" else Exception as e_generic_retry: # Temp server issues
                    # This generic Exception for Gemini is broad, might need refinement if specific Gemini transient errors arise
                    print(f"  LLM API Error ({llm_provider.capitalize()}, API Attempt {api_attempt + 1}/{max_api_retries}) for item {item_idx_for_log}: Temporary issue - {type(e_generic_retry).__name__} {e_generic_retry}", file=sys.stderr)
                    if api_attempt + 1 == max_api_retries:
                         _raw_llm_output_core_this_api_cycle = f"// LLM_API_TEMP_ERROR_MAX_RETRIES ({llm_provider.capitalize()}, {type(e_generic_retry).__name__}) FOR_SOURCE: {source_sentence_text}"
                         break
                    await asyncio.sleep(retry_delay_seconds * (2**api_attempt)) # Exponential backoff for server issues

                except anthropic.RateLimitError if llm_provider == "claude" else Exception as e_rate_limit: # Gemini uses general Exception for 429
                    # For Gemini, check error string for 429
                    is_gemini_rate_limit = llm_provider == "gemini" and ("429" in str(e_rate_limit).lower() or "resource_exhausted" in str(e_rate_limit).lower())
                    
                    if llm_provider == "claude" or is_gemini_rate_limit:
                        print(f"  LLM API Error ({llm_provider.capitalize()}, API Attempt {api_attempt + 1}/{max_api_retries}) for item {item_idx_for_log}: Rate limit / Quota - {e_rate_limit}", file=sys.stderr)
                        if api_attempt + 1 == max_api_retries:
                            print(f"    FATAL QUOTA LIKELY ({llm_provider.capitalize()}, persisted API error): Item {item_idx_for_log}. Error: {e_rate_limit}", file=sys.stderr)
                            return FATAL_QUOTA_ERROR_SENTINEL
                        effective_delay = retry_delay_seconds * (2**api_attempt)
                        print(f"    Item {item_idx_for_log} ({llm_provider.capitalize()}): Retrying API call (rate limit/quota) in {effective_delay} seconds...", file=sys.stderr)
                        await asyncio.sleep(effective_delay)
                    else: # General Gemini exception not identified as rate limit
                         raise e_rate_limit # Re-raise if not a Gemini rate limit

                except anthropic.APIStatusError as e_claude_status: # Claude specific for 4xx/5xx not covered above
                    print(f"  LLM API Error (Claude, API Attempt {api_attempt + 1}/{max_api_retries}) for item {item_idx_for_log}: Status {e_claude_status.status_code} - {e_claude_status.message}", file=sys.stderr)
                    if e_claude_status.status_code == 400 and e_claude_status.body and \
                       e_claude_status.body.get('error', {}).get('type') == 'invalid_request_error':
                        error_message_lower = e_claude_status.message.lower()
                        if "safety" in error_message_lower or "policy" in error_message_lower or "harmful" in error_message_lower:
                            print(f"    Potential copyright/safety block from Claude for item {item_idx_for_log}.", file=sys.stderr)
                            _raw_llm_output_core_this_api_cycle = f"{COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX}{source_sentence_text}"
                            api_call_successful_flag = True; break 
                    
                    # For other 4xx/5xx errors, treat as potentially retriable or fatal depending on attempts
                    if api_attempt + 1 == max_api_retries:
                        _raw_llm_output_core_this_api_cycle = f"// LLM_API_STATUS_ERROR_MAX_RETRIES (Claude, {e_claude_status.status_code}) FOR_SOURCE: {source_sentence_text}"
                        break
                    await asyncio.sleep(retry_delay_seconds)

                except anthropic.AuthenticationError as e_auth:
                    print(f"  FATAL LLM API Authentication Error ({llm_provider.capitalize()}): {e_auth}. Check API Key.", file=sys.stderr)
                    return FATAL_QUOTA_ERROR_SENTINEL # Treat as fatal for this run

                except Exception as e: # General catch-all, primarily for Gemini's varied exceptions
                    error_str = str(e).lower()
                    print(f"  LLM API Error ({llm_provider.capitalize()}, API Attempt {api_attempt + 1}/{max_api_retries}) for item {item_idx_for_log}: {type(e).__name__} - {e}", file=sys.stderr)
                    # Check for Gemini quota errors again if not caught by specific rate limit check
                    if llm_provider == "gemini" and any(kw in error_str for kw in ["quota", "limit", "billing", "exceeded", "model_unavailable", "resource_exhausted"]):
                         if api_attempt + 1 == max_api_retries:
                            print(f"    FATAL QUOTA LIKELY (Gemini, persisted API error): Item {item_idx_for_log}. Error: {e}", file=sys.stderr)
                            return FATAL_QUOTA_ERROR_SENTINEL
                         effective_delay = retry_delay_seconds * (2**api_attempt)
                         print(f"    Item {item_idx_for_log} (Gemini): Retrying API call (potential quota/availability) in {effective_delay} seconds...", file=sys.stderr)
                         await asyncio.sleep(effective_delay)
                         continue # continue to next API attempt

                    if api_attempt + 1 == max_api_retries:
                        _raw_llm_output_core_this_api_cycle = f"// LLM_API_ERROR_MAX_RETRIES ({llm_provider.capitalize()}, {type(e).__name__}) FOR_SOURCE: {source_sentence_text}"
                        break
                    await asyncio.sleep(retry_delay_seconds)

            raw_llm_output_core_from_api = _raw_llm_output_core_this_api_cycle

            if raw_llm_output_core_from_api == FATAL_QUOTA_ERROR_SENTINEL: return FATAL_QUOTA_ERROR_SENTINEL
            if not api_call_successful_flag and not raw_llm_output_core_from_api :
                 raw_llm_output_core_from_api = f"// LLM_NO_OUTPUT_AFTER_API_RETRIES ({llm_provider.capitalize()}) FOR_SOURCE: {source_sentence_text}"

            if END_SENTENCE_MARKER_TEXT in raw_llm_output_core_from_api:
                lines_cleaned = [line for line in raw_llm_output_core_from_api.splitlines() if line.strip() != END_SENTENCE_MARKER_TEXT]
                raw_llm_output_core_from_api = "\n".join(lines_cleaned).strip()

            raw_output_for_validation = raw_llm_output_core_from_api
            debug_lines_from_llm = []
            if "// DEBUG:" in raw_llm_output_core_from_api:
                potential_block_lines = []
                seen_advs_or_first_content_marker = False
                possible_first_markers = ["AdvS::", "SimS::", "SimE::", "CHAPTER_MARKER_DIRECT::"]
                for line_content in raw_llm_output_core_from_api.splitlines():
                    stripped_line = line_content.strip()
                    is_debug_line = stripped_line.startswith("// DEBUG:")
                    if not seen_advs_or_first_content_marker:
                        for marker_val in possible_first_markers:
                            if stripped_line.startswith(marker_val):
                                seen_advs_or_first_content_marker = True; break
                    if is_debug_line: debug_lines_from_llm.append(line_content)
                    elif seen_advs_or_first_content_marker: potential_block_lines.append(line_content)
                    elif not stripped_line.startswith("//"): potential_block_lines.append(line_content) # Capture non-comment lines before first marker too
                
                if debug_lines_from_llm:
                    print(f"  Item {item_idx_for_log} ({llm_provider.capitalize()}/{model_name_to_use_in_api_call}) - LLM Debug Info (Validation Attempt {validation_attempt+1}):")
                    for d_line in debug_lines_from_llm: print(f"    {d_line.strip()}")
                
                if not potential_block_lines and debug_lines_from_llm: # Only debug lines, likely an error
                    raw_output_for_validation = "\n".join(debug_lines_from_llm) # Validate the debug message itself if it's all we have
                elif potential_block_lines:
                    raw_output_for_validation = "\n".join(potential_block_lines).strip()
                # If neither, raw_output_for_validation remains raw_llm_output_core_from_api

            validation_errors = validate_llm_block(raw_output_for_validation)
            last_validation_error_details_str = "; ".join(validation_errors)

            if not validation_errors: return raw_output_for_validation

            print(f"  Item {item_idx_for_log} ({llm_provider.capitalize()}/{model_name_to_use_in_api_call}): Validation FAILED (Attempt {validation_attempt+1}/{max_validation_retries}) for '{source_sentence_text[:30]}...': {last_validation_error_details_str}", file=sys.stderr)
            
            # This flag gets updated for the *next* validation attempt's prompt construction
            is_copyright_retry_attempt = raw_llm_output_core_from_api.startswith(COPYRIGHT_BLOCK_PLACEHOLDER_PREFIX)
            
            if validation_attempt + 1 < max_validation_retries:
                await asyncio.sleep(retry_delay_seconds)
                continue
            else:
                if is_copyright_retry_attempt :
                     return f"// {llm_provider.upper()}_{model_name_to_use_in_api_call.replace('/', '_')}_COPYRIGHT_VALIDATION_FAILED_MAX_RETRIES (Errors: {last_validation_error_details_str}) FOR_SOURCE: {source_sentence_text}"
                else:
                    return f"// {llm_provider.upper()}_{model_name_to_use_in_api_call.replace('/', '_')}_VALIDATION_FAILED_MAX_RETRIES (Errors: {last_validation_error_details_str}) FOR_SOURCE: {source_sentence_text}"
        
        return f"// {llm_provider.upper()}_{model_name_to_use_in_api_call.replace('/', '_')}_VALIDATION_LOOP_UNEXPECTED_EXIT_FOR_SOURCE: {source_sentence_text}"

async def process_book_file_async(
    staged_file_path: Path, llm_output_dir: Path, 
    llm_client_or_model_obj: Any, # Actual client/model object
    llm_provider: str, # "gemini" or "claude"
    model_name_to_use: str, # Specific model name string for this provider
    args: argparse.Namespace,
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
            
            # Ensure END_SENTENCE_MARKER_TEXT ends with a newline for regex if not already
            effective_end_marker = END_SENTENCE_MARKER_TEXT
            if not effective_end_marker.endswith('\n'):
                 effective_end_marker += '\n'

            raw_blocks_with_delimiters = re.findall(f"(.*?{re.escape(effective_end_marker)})", full_existing_content, re.DOTALL)

            if raw_blocks_with_delimiters:
                last_block_full_content = raw_blocks_with_delimiters[-1]
                lines_in_last_block = last_block_full_content.strip().splitlines()
                
                penultimate_line_of_last_block = ""
                # Check if the last line is indeed the END_SENTENCE_MARKER_TEXT (possibly stripped of its newline by splitlines)
                if lines_in_last_block and lines_in_last_block[-1].strip() == END_SENTENCE_MARKER_TEXT.strip():
                    if len(lines_in_last_block) >= 2:
                        penultimate_line_of_last_block = lines_in_last_block[-2].strip()
                elif lines_in_last_block : # Only one line, and it's not the marker (or marker itself is the content)
                    penultimate_line_of_last_block = lines_in_last_block[0].strip()


                if penultimate_line_of_last_block == COMPLETION_MARKER_TEXT:
                    print(f"Skipping '{staged_file_path.name}': Found completion marker.")
                    return True, True, False
                elif penultimate_line_of_last_block.startswith(RESUME_MARKER_PREFIX) and penultimate_line_of_last_block.endswith("--- //"):
                    try:
                        index_str = penultimate_line_of_last_block[len(RESUME_MARKER_PREFIX):].split(" ")[0]
                        start_item_idx = int(index_str)
                        is_resuming = True
                        existing_content_blocks = raw_blocks_with_delimiters[:-1] # Exclude the block with the resume marker
                        print(f"Resuming '{staged_file_path.name}' from source item index {start_item_idx}.")
                    except ValueError: start_item_idx = 0; is_resuming = False; existing_content_blocks = []
                elif penultimate_line_of_last_block.startswith(OUTPUT_LIMITED_MARKER_PREFIX) and penultimate_line_of_last_block.endswith("--- //"):
                    try:
                        limit_in_marker_str = penultimate_line_of_last_block[len(OUTPUT_LIMITED_MARKER_PREFIX):].split("_ITEMS")[0]
                        limit_in_marker = int(limit_in_marker_str)
                        can_process_more = args.limit_items is None or args.limit_items > limit_in_marker
                        if can_process_more:
                            start_item_idx = len(raw_blocks_with_delimiters) -1 # Start after the limited block
                            is_resuming = True; existing_content_blocks = raw_blocks_with_delimiters[:-1] # Exclude limited marker block
                            print(f"Resuming '{staged_file_path.name}' after previous limit of {limit_in_marker} items.")
                        else: 
                            print(f"Skipping '{staged_file_path.name}': Limit of {limit_in_marker} (from file) meets or exceeds current --limit-items={args.limit_items}.")
                            return True, True, False
                    except ValueError: start_item_idx = 0; is_resuming = False; existing_content_blocks = []
                elif args.limit_items is None: # Ambiguous end, reprocess if no explicit limit
                    print(f"Warning: Could not determine resume point for '{output_llm_file_path.name}'. Reprocessing from start (or use --force).", file=sys.stderr)
                    start_item_idx = 0; is_resuming = False; existing_content_blocks = []

        except Exception as e_read:
            print(f"Warning: Error reading existing output file '{output_llm_file_path.name}': {e_read}. Reprocessing from start.", file=sys.stderr)
            start_item_idx = 0; is_resuming = False; existing_content_blocks = []
    
    print(f"Processing '{staged_file_path.name}' (LLM: {llm_provider.capitalize()}/{model_name_to_use}, effective start source item index: {start_item_idx})...")
    try:
        with open(staged_file_path, 'r', encoding='utf-8') as f_in: raw_lines_from_staged_file = f_in.readlines()
    except Exception as e: 
        print(f"FATAL: Could not read input staged file {staged_file_path}: {e}", file=sys.stderr)
        return False, False, False # was_skipped, operation_successful, daily_limit_hit

    all_items: List[Dict[str, Any]] = []
    for orig_idx, line_raw in enumerate(raw_lines_from_staged_file):
        line = line_raw.strip()
        if CHAPTER_MARKER_REGEX.match(line): all_items.append({"type": "marker", "text": CHAPTER_MARKER_REGEX.match(line).group(1).strip(), "original_idx_in_file": orig_idx})
        elif SENTENCE_LINE_REGEX.match(line): all_items.append({"type": "sentence", "text": SENTENCE_LINE_REGEX.match(line).group(1).strip(), "original_idx_in_file": orig_idx})
    
    if not all_items:
        try:
            llm_output_dir.mkdir(parents=True, exist_ok=True)
            with open(output_llm_file_path, 'w', encoding='utf-8') as f_out: f_out.write(f"// NO_PROCESSABLE_ITEMS_IN_SOURCE_FILE\n{END_SENTENCE_MARKER_TEXT}\n")
            print(f"Wrote placeholder: {output_llm_file_path.name} (no processable items).")
        except Exception as e_ph: print(f"Error writing placeholder for empty source {output_llm_file_path.name}: {e_ph}", file=sys.stderr)
        return False, True, False # Not skipped, op "successful" (wrote placeholder), no limit hit

    items_to_process_this_run_slice: List[Dict[str, Any]] = []
    target_total_items_in_output_file = len(all_items)
    if args.limit_items is not None: target_total_items_in_output_file = min(args.limit_items, len(all_items))
    
    num_existing_items_count = "".join(existing_content_blocks).count(END_SENTENCE_MARKER_TEXT)

    if num_existing_items_count >= target_total_items_in_output_file:
        if is_resuming: # Ensure file is correctly terminated if resuming led to this state
            try:
                with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
                    f_out.write("".join(existing_content_blocks)) # Write back the existing blocks
                    final_marker_to_add = ""
                    if num_existing_items_count >= len(all_items):
                        final_marker_to_add = f"{COMPLETION_MARKER_TEXT}\n{END_SENTENCE_MARKER_TEXT}\n"
                    elif args.limit_items is not None and num_existing_items_count >= args.limit_items:
                        final_marker_to_add = f"{OUTPUT_LIMITED_MARKER_PREFIX}{args.limit_items}_ITEMS --- //\n{END_SENTENCE_MARKER_TEXT}\n"
                    if final_marker_to_add: f_out.write(final_marker_to_add)
                print(f"Finalized '{output_llm_file_path.name}' as existing items meet target.")
            except Exception as e_fin: print(f"Error finalizing {output_llm_file_path.name}: {e_fin}", file=sys.stderr)
        return False, True, False # Not skipped, considered successful, no limit

    num_new_items_to_process_this_run = target_total_items_in_output_file - num_existing_items_count
    end_idx_for_slice_in_all_items = min(len(all_items), start_item_idx + num_new_items_to_process_this_run)
    
    if start_item_idx < end_idx_for_slice_in_all_items:
        items_to_process_this_run_slice = all_items[start_item_idx:end_idx_for_slice_in_all_items]
    
    if not items_to_process_this_run_slice:
        if is_resuming: # Similar finalization if no new items are needed after resume logic
             try:
                with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
                    f_out.write("".join(existing_content_blocks))
                    final_marker_to_add = ""
                    if num_existing_items_count >= len(all_items):
                        final_marker_to_add = f"{COMPLETION_MARKER_TEXT}\n{END_SENTENCE_MARKER_TEXT}\n"
                    elif args.limit_items is not None and num_existing_items_count >= args.limit_items:
                        final_marker_to_add = f"{OUTPUT_LIMITED_MARKER_PREFIX}{args.limit_items}_ITEMS --- //\n{END_SENTENCE_MARKER_TEXT}\n"
                    if final_marker_to_add: f_out.write(final_marker_to_add)
                print(f"Finalized '{output_llm_file_path.name}' as no new items needed.")
             except Exception as e_fin: print(f"Error finalizing {output_llm_file_path.name}: {e_fin}", file=sys.stderr)
        return False, True, False

    newly_processed_blocks_this_run: List[str] = []
    daily_limit_hit_for_this_book = False
    first_failed_item_original_idx = -1 # Store original index in all_items
    llm_calls_made_this_run = 0
    
    for run_slice_idx, item_data in enumerate(items_to_process_this_run_slice):
        original_item_idx_in_all_items = start_item_idx + run_slice_idx # This is the 0-based index in all_items

        if daily_limit_hit_for_this_book and first_failed_item_original_idx != -1:
            print(f"  Skipping further processing for source item {original_item_idx_in_all_items+1} in '{book_name_stem}' due to earlier fatal quota error.")
            break

        item_output_block_content = None
        
        if item_data["type"] == "marker":
            item_output_block_content = f"CHAPTER_MARKER_DIRECT:: {item_data['text']}"
        
        elif item_data["type"] == "sentence":
            source_sentence_text = item_data["text"]
            preceding_context_str = "[NO PRECEDING CONTEXT]"
            pre_sent_texts = [all_items[i]["text"] for i in range(max(0, original_item_idx_in_all_items - num_context_sentences), original_item_idx_in_all_items) if all_items[i]["type"] == "sentence"]
            if pre_sent_texts: preceding_context_str = "\n".join(pre_sent_texts)
            
            succeeding_context_str = "[NO SUCCEEDING CONTEXT]"
            suc_sent_texts = [all_items[i]["text"] for i in range(original_item_idx_in_all_items + 1, min(len(all_items), original_item_idx_in_all_items + 1 + num_context_sentences)) if all_items[i]["type"] == "sentence"]
            if suc_sent_texts: succeeding_context_str = "\n".join(suc_sent_texts)

            print(f"  Processing item {original_item_idx_in_all_items+1} ('{source_sentence_text[:30]}...') with {llm_provider.capitalize()}/{model_name_to_use}.")
            
            item_result_str = await process_sentence_with_llm_async(
                source_sentence_text, preceding_context_str, succeeding_context_str,
                llm_client_or_model_obj, llm_provider, model_name_to_use,
                args.max_api_retries, args.max_validation_retries, DEFAULT_RETRY_DELAY_SECONDS,
                semaphore, original_item_idx_in_all_items + 1, # 1-based for logging
                LLM_PROMPT_TEMPLATE, CLAUDE_SYSTEM_PROMPT, args.claude_max_tokens,
                is_copyright_retry_attempt=False # Initial call, not a copyright-specific retry from orchestrator
            )
            llm_calls_made_this_run += 1

            if item_result_str == FATAL_QUOTA_ERROR_SENTINEL:
                daily_limit_hit_for_this_book = True
                if first_failed_item_original_idx == -1:
                    first_failed_item_original_idx = original_item_idx_in_all_items
            # Store result whether it's good output or a placeholder error/copyright
            item_output_block_content = item_result_str
        
        if item_output_block_content is not None:
            newly_processed_blocks_this_run.append(item_output_block_content.strip() + f"\n{END_SENTENCE_MARKER_TEXT}\n")
        
        if daily_limit_hit_for_this_book and first_failed_item_original_idx != -1:
            break

    final_output_content = "".join(existing_content_blocks) + "".join(newly_processed_blocks_this_run)
    total_end_sentence_markers_in_final = final_output_content.count(END_SENTENCE_MARKER_TEXT)

    if daily_limit_hit_for_this_book:
        # first_failed_item_original_idx is the index of the item that *caused* the quota error.
        # We want to resume from this item next time.
        if first_failed_item_original_idx != -1 and first_failed_item_original_idx < len(all_items):
            final_output_content += f"{RESUME_MARKER_PREFIX}{first_failed_item_original_idx} --- //\n{END_SENTENCE_MARKER_TEXT}\n"
            print(f"  Partial file for '{book_name_stem}' will be saved. Resume from source item index {first_failed_item_original_idx} next time.")
    elif total_end_sentence_markers_in_final >= len(all_items):
        final_output_content += f"{COMPLETION_MARKER_TEXT}\n{END_SENTENCE_MARKER_TEXT}\n"
        print(f"  Marking '{book_name_stem}' as fully processed.")
    elif args.limit_items is not None and total_end_sentence_markers_in_final >= args.limit_items:
        final_output_content += f"{OUTPUT_LIMITED_MARKER_PREFIX}{args.limit_items}_ITEMS --- //\n{END_SENTENCE_MARKER_TEXT}\n"
        print(f"  Marking '{book_name_stem}' as limited to {args.limit_items} items.")
    elif total_end_sentence_markers_in_final > 0 : # Partial but not due to error or limit, means it ran out of items_to_process_this_run_slice
        # This case should ideally be covered by num_new_items_to_process_this_run logic
        # But as a fallback, if it's partial and not completed/limited/error, save resume marker
        # The resume index should be the count of *successfully processed blocks* in `all_items`
        # which is `total_end_sentence_markers_in_final`.
        if total_end_sentence_markers_in_final < len(all_items):
            final_output_content += f"{RESUME_MARKER_PREFIX}{total_end_sentence_markers_in_final} --- //\n{END_SENTENCE_MARKER_TEXT}\n"
            print(f"  Partial file for '{book_name_stem}' saved. Resume from source item index {total_end_sentence_markers_in_final} next time.")


    try:
        llm_output_dir.mkdir(parents=True, exist_ok=True)
        with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
            f_out.write(final_output_content)
        
        num_newly_processed_items = len(newly_processed_blocks_this_run)
        log_msg_blocks = f"Wrote {num_newly_processed_items} new blocks " \
                         f"({llm_calls_made_this_run} LLM calls attempted) to '{output_llm_file_path.name}'"
        print(log_msg_blocks)
        return False, True, daily_limit_hit_for_this_book
    except Exception as e:
        print(f"Error writing output file '{output_llm_file_path.name}': {e}", file=sys.stderr)
        return False, False, daily_limit_hit_for_this_book

async def main_async():
    print(f"--- stage2llm_async.py Version: {SCRIPT_VERSION} ---")
    dotenv_path = Path('.') / '.env'
    if dotenv_path.is_file():
        load_dotenv(dotenv_path=dotenv_path)
        print(f"INFO: Attempted to load API keys from '{dotenv_path.resolve()}'.")
    else:
        print(f"INFO: No .env file found at '{dotenv_path.resolve()}'. API keys must be provided via CLI or environment variables.")

    parser = argparse.ArgumentParser(formatter_class=argparse.ArgumentDefaultsHelpFormatter, description="Process staged text files with an LLM (async) to generate .llm.txt format.")
    
    # Provider and API Keys
    parser.add_argument("--llm_provider", default=DEFAULT_LLM_PROVIDER, choices=["gemini", "claude"], help="LLM provider to use.")
    parser.add_argument("--gemini_api_key", help="Google Gemini API Key. Overrides GOOGLE_API_KEY from .env/environment.")
    parser.add_argument("--anthropic_api_key", help="Anthropic Claude API Key. Overrides ANTHROPIC_API_KEY from .env/environment.")

    # Model Names
    parser.add_argument("--gemini_model_name", default=DEFAULT_GEMINI_MODEL_NAME, help="Gemini model name to use.")
    parser.add_argument("--claude_model_name", default=DEFAULT_CLAUDE_MODEL_NAME, help="Claude model name to use.")
    parser.add_argument("--claude_max_tokens", type=int, default=DEFAULT_CLAUDE_MAX_TOKENS_OUTPUT, help="Max tokens for Claude's output generation.")

    # File and Directory Paths
    parser.add_argument("--project_config", default="config.toml", help="Path to the project's main config.toml file.")
    parser.add_argument("--input_staged_subdir", default=DEFAULT_STAGED_DIR_NAME, help="Subdirectory within content_project_dir for input .txt files.")
    parser.add_argument("--output_llm_subdir", default=DEFAULT_LLM_OUTPUT_DIR_NAME, help="Subdirectory within content_project_dir for output .llm.txt files.")
    
    # Processing Control
    parser.add_argument("--force", action="store_true", help="Force reprocessing of files from scratch, ignoring existing partial or complete .llm.txt files.")
    parser.add_argument("--max_api_retries", type=int, default=DEFAULT_MAX_API_RETRIES, help="Max retries for basic API calls.")
    parser.add_argument("--max_validation_retries", type=int, default=DEFAULT_MAX_VALIDATION_RETRIES, help="Max retries with corrective prompts if LLM output fails validation. Set to 1 for no corrective retries.")
    parser.add_argument("--context_sents", type=int, default=2, help="Number of preceding/succeeding sentences for context.")
    parser.add_argument("--limit_items", type=int, default=None, help="Output only the first N items (markers or sentences) in total for each book. Default: process all.")
    parser.add_argument("--concurrent_requests", type=int, default=DEFAULT_CONCURRENT_REQUESTS, help="Max concurrent LLM API requests.")
    
    args = parser.parse_args()

    # Initialize LLM client/model object
    llm_client_or_model_obj = None
    actual_model_name_to_use = ""

    if args.llm_provider == "gemini":
        if not genai:
            print("ERROR: Gemini provider selected, but 'google-generativeai' library is not installed/imported.", file=sys.stderr)
            sys.exit(1)
        api_key_to_use = args.gemini_api_key or os.getenv("GOOGLE_API_KEY")
        if not api_key_to_use:
            print("ERROR: Gemini API Key not found. Set GOOGLE_API_KEY or use --gemini_api_key.", file=sys.stderr)
            sys.exit(1)
        try:
            genai.configure(api_key=api_key_to_use)
            llm_client_or_model_obj = genai.GenerativeModel(args.gemini_model_name)
            actual_model_name_to_use = args.gemini_model_name
            print(f"Successfully configured Gemini model: {actual_model_name_to_use}")
            try: print(f"  Gemini SDK Version: {importlib.metadata.version('google-generativeai')}")
            except: pass
        except Exception as e:
            print(f"ERROR configuring Gemini SDK: {e}", file=sys.stderr)
            sys.exit(1)
    elif args.llm_provider == "claude":
        if not anthropic:
            print("ERROR: Claude provider selected, but 'anthropic' library is not installed/imported.", file=sys.stderr)
            sys.exit(1)
        api_key_to_use = args.anthropic_api_key or os.getenv("ANTHROPIC_API_KEY")
        if not api_key_to_use:
            print("ERROR: Anthropic API Key not found. Set ANTHROPIC_API_KEY or use --anthropic_api_key.", file=sys.stderr)
            sys.exit(1)
        try:
            llm_client_or_model_obj = anthropic.AsyncAnthropic(api_key=api_key_to_use)
            actual_model_name_to_use = args.claude_model_name
            # Test call might be good here, but for now, assume client init is enough
            print(f"Successfully configured Anthropic client for model: {actual_model_name_to_use}")
            try: print(f"  Anthropic SDK Version: {importlib.metadata.version('anthropic')}")
            except: pass
        except Exception as e:
            print(f"ERROR configuring Anthropic SDK: {e}", file=sys.stderr)
            sys.exit(1)
    else:
        print(f"ERROR: Unknown LLM provider '{args.llm_provider}'.", file=sys.stderr)
        sys.exit(1)

    content_project_root_str = load_project_config(args.project_config)
    if not content_project_root_str: sys.exit(1)
    
    content_project_root = Path(content_project_root_str)
    staged_input_dir = content_project_root / args.input_staged_subdir
    llm_output_dir = content_project_root / args.output_llm_subdir

    if not staged_input_dir.is_dir():
        print(f"ERROR: Input directory '{staged_input_dir}' not found.", file=sys.stderr)
        sys.exit(1)

    staged_files_to_process = sorted([f for f in staged_input_dir.glob('*.txt') if not f.name.endswith('.junk.txt')])
    if not staged_files_to_process:
        print(f"INFO: No suitable .txt files found in '{staged_input_dir}'.", file=sys.stderr)
        sys.exit(0)
    
    print(f"\nFound {len(staged_files_to_process)} Staged files to process from '{staged_input_dir}'.")
    print(f"Using LLM Provider: {args.llm_provider.capitalize()} with model {actual_model_name_to_use}")
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
            llm_client_or_model_obj, 
            args.llm_provider,
            actual_model_name_to_use,
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
        print("You can re-run the script (e.g., when quota resets) to continue processing from where it left off.")

if __name__ == "__main__":
    try:
        asyncio.run(main_async())
    except KeyboardInterrupt:
        print("\nProcessing interrupted by user. Partial progress for the current book might not be saved unless its write cycle completed.")
    except Exception as e:
        print(f"\nAn unexpected error occurred in main execution: {type(e).__name__} - {e}", file=sys.stderr)
        import traceback
        traceback.print_exc()