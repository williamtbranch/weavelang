# stage2llm.py

import google.generativeai as genai
import os
from pathlib import Path
import re
import time
import argparse
import sys
from typing import Optional, List # Added List for type hinting
from dotenv import load_dotenv

# --- Import from the validator module ---
try:
    from llm_output_validator import validate_llm_block
except ImportError:
    print("ERROR: Could not import 'validate_llm_block' from 'llm_output_validator.py'.")
    print("Please ensure 'llm_output_validator.py' is in the same directory or accessible in PYTHONPATH.")
    sys.exit(1)

# --- Configuration ---
MODEL_NAME = "gemini-2.5-pro-preview-05-06"
DEFAULT_STAGED_DIR_NAME = "Staged"
DEFAULT_LLM_OUTPUT_DIR_NAME = "stage"
DEFAULT_MAX_API_RETRIES = 3
DEFAULT_MAX_VALIDATION_RETRIES = 2 # 1 initial try + 1 corrective retry
DEFAULT_RETRY_DELAY_SECONDS = 5
API_CALL_DELAY_SECONDS = 1 # Base delay between successful API calls for sentences

SENTENCE_LINE_REGEX = re.compile(r"^{S\d+:\s*(.*)}$")
CHAPTER_MARKER_REGEX = re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")

# --- LLM_PROMPT_TEMPLATE ---
LLM_PROMPT_TEMPLATE = """
You are an expert linguist and data formatter.
Your primary task is to process the "TARGET SENTENCE" provided below.
Use the "PRECEDING CONTEXT" and "SUCCEEDING CONTEXT" (if available) ONLY for disambiguation, pronoun resolution, and to understand the narrative flow related to the TARGET SENTENCE.
DO NOT generate full output blocks for the context sentences.
The output block you generate MUST correspond ONLY to the "TARGET SENTENCE".

Overall Goal for Simplification:
For both the "SimE" (Simple English) and "SimS" (Simple Spanish) outputs, the primary goal is extreme simplicity suitable for an absolute beginner learner. Think of language appropriate for a **first-grade reading level (e.g., for a 6-7 year old child learning to read or learning a second language from scratch).** Prioritize very common, high-frequency words and simple sentence structures.
+IMPORTANT: Your generated output for a single TARGET SENTENCE should be one continuous block of text containing all the required '::' sections. DO NOT include the literal string 'END_SENTENCE' anywhere within this block of text you generate; the 'END_SENTENCE' marker is only used externally to separate distinct, fully-formed blocks in the final combined file.

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

AdvSL:: [flat_list_adv_lemma1 flat_list_adv_lemma2 ...] // Flat list: All AdvS lemmas for the TARGET SENTENCE. Strictly lemmatized per rules.

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
S1 :: [EngWord1_from_SimE_S1_phrase]->[SpaLemma1]([ExactSpaForm1])([ViabilityFlagY/N]) | [EngWord2_from_SimE_S1_phrase]->[SpaLemma2]([ExactSpaForm2])([ViabilityFlagY/N])
// ...

// LOCKED_PHRASE :: [Sx Sy ...] (Optional, only if certain SimS_Segments from TARGET SENTENCE should always stay together)

END_SENTENCE
---
Ensure all sections are present for the TARGET SENTENCE.

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
END_SENTENCE //This line should not be output by you. It will be added by an external tool.
// (The LLM's output for the TARGET SENTENCE block should conclude after the last section like DIGLOT_MAP or LOCKED_PHRASE. Do not add an "END_SENTENCE" line yourself.)
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

def process_sentence_with_llm(source_sentence_text: str, preceding_context: str, succeeding_context: str,
                              model, max_api_retries: int, max_validation_retries: int, 
                              retry_delay: int) -> str:
    
    original_prompt_text = LLM_PROMPT_TEMPLATE.format(
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
    # generation_config = genai.types.GenerationConfig(temperature=0.7) # Optional

    for validation_attempt in range(max_validation_retries):
        raw_llm_output_core = None # The LLM's response text before END_SENTENCE is managed
        
        for api_attempt in range(max_api_retries):
            try:
                # print(f"    DEBUG: Sending (API Attempt {api_attempt+1}, Val Attempt {validation_attempt+1}):\n{current_prompt_to_send[:350]}...")
                response = model.generate_content(current_prompt_to_send, safety_settings=safety_settings)
                
                if not response.parts:
                    reason = "Unknown reason, empty parts list in response."
                    if response.prompt_feedback and response.prompt_feedback.block_reason:
                        reason = response.prompt_feedback.block_reason_message or str(response.prompt_feedback.block_reason)
                    print(f"  LLM Warning (API Attempt {api_attempt+1}): Prompt for '{source_sentence_text[:50]}...' blocked or empty. Reason: {reason}", file=sys.stderr)
                    if response.prompt_feedback and response.prompt_feedback.block_reason:
                        raw_llm_output_core = f"// LLM_BLOCKED_OR_EMPTY_RESPONSE (Reason: {reason}) FOR_SOURCE: {source_sentence_text}"
                        break # Break API retry loop for this validation attempt
                
                if response.text:
                    raw_llm_output_core = response.text.strip()
                    break # Successful API call and got text
                else: # No .text attribute but response.parts was not empty
                    print(f"  LLM Warning (API Attempt {api_attempt+1}): Response for '{source_sentence_text[:50]}...' has no .text. Parts: {response.parts}", file=sys.stderr)
                
                if api_attempt + 1 < max_api_retries: time.sleep(retry_delay)

            except Exception as e:
                print(f"  LLM API Error (API Attempt {api_attempt + 1}/{max_api_retries}) for '{source_sentence_text[:50]}...': {e}", file=sys.stderr)
                if "429" in str(e) or "resource_exhausted" in str(e).lower() or "model_unavailable" in str(e).lower() or "quota" in str(e).lower():
                    effective_delay = retry_delay * (2**api_attempt)
                    print(f"  Retrying API call in {effective_delay} seconds...", file=sys.stderr)
                    time.sleep(effective_delay)
                elif api_attempt + 1 == max_api_retries:
                    raw_llm_output_core = f"// LLM_API_ERROR_MAX_RETRIES_FOR_SOURCE: {source_sentence_text}"
                    break 
                else:
                    time.sleep(retry_delay)
            
            if raw_llm_output_core and raw_llm_output_core.startswith("// LLM_BLOCKED"):
                break 
        # End of API retry loop for this validation attempt

        if not raw_llm_output_core: 
            raw_llm_output_core = f"// LLM_NO_OUTPUT_AFTER_API_RETRIES_FOR_SOURCE: {source_sentence_text}"
            return raw_llm_output_core + "\nEND_SENTENCE\n" 

        if raw_llm_output_core.startswith("// LLM_BLOCKED"): # Or other critical API failure placeholders
            return raw_llm_output_core + "\nEND_SENTENCE\n"

        validation_errors = validate_llm_block(raw_llm_output_core)
        if not validation_errors:
            return raw_llm_output_core # Script will append END_SENTENCE
        
        error_details = "; ".join(validation_errors)
        print(f"  LLM Output Validation Failed (Attempt {validation_attempt+1}/{max_validation_retries}) for '{source_sentence_text[:50]}...': {error_details}", file=sys.stderr)

        if validation_attempt + 1 < max_validation_retries:
            corrective_instruction = (
                "PREVIOUS ATTEMPT FAILED VALIDATION. PLEASE PAY EXTREME ATTENTION TO THE REQUIRED OUTPUT FORMAT. "
                f"Specifically, ensure all sections are present and correctly formatted. Previous errors: {error_details}\n---\n"
            )
            current_prompt_to_send = corrective_instruction + original_prompt_text
            print(f"  Retrying with corrective prompt (Validation Attempt {validation_attempt+2}).")
            time.sleep(retry_delay) 
        else: # Max validation retries reached
            return f"// LLM_OUTPUT_VALIDATION_FAILED_MAX_RETRIES (Errors: {error_details}) FOR_SOURCE: {source_sentence_text}"

    # Fallback if validation loop logic has an issue (should ideally not be reached)
    return f"// LLM_PROCESSING_FAILED_UNEXPECTEDLY_IN_VALIDATION_LOOP_FOR_SOURCE: {source_sentence_text}"


def process_book_file(staged_file_path: Path, llm_output_dir: Path, model, args,
                      num_context_sentences: int, item_limit: Optional[int]):
    book_name_stem = staged_file_path.stem
    output_llm_file_path = llm_output_dir / f"{book_name_stem}.llm.txt"

    if not args.force and output_llm_file_path.exists() and item_limit is None:
        print(f"Skipping '{staged_file_path.name}': Output file '{output_llm_file_path.name}' already exists (and not in limit/force mode).")
        return True, True 

    print(f"Processing '{staged_file_path.name}'...")
    
    try:
        with open(staged_file_path, 'r', encoding='utf-8') as f_in:
            raw_lines_from_staged_file = f_in.readlines()
    except Exception as e:
        print(f"Error reading input file '{staged_file_path.name}': {e}", file=sys.stderr)
        return False, False

    all_items: List[dict] = [] # Stores {"type": "marker"|"sentence", "text": "..."}
    for line_raw in raw_lines_from_staged_file:
        line = line_raw.strip()
        chapter_match = CHAPTER_MARKER_REGEX.match(line)
        if chapter_match:
            all_items.append({"type": "marker", "text": chapter_match.group(1).strip()})
        else:
            sentence_match = SENTENCE_LINE_REGEX.match(line)
            if sentence_match:
                all_items.append({"type": "sentence", "text": sentence_match.group(1).strip()})

    if not all_items:
        print(f"No processable items (%%CHAPTER_MARKER%% or {{S...}}) found in '{staged_file_path.name}'.", file=sys.stderr)
        if item_limit is None or item_limit >= 0 : # Write placeholder if we intended to process or limit=0
             try:
                llm_output_dir.mkdir(parents=True, exist_ok=True)
                with open(output_llm_file_path, 'w', encoding='utf-8') as f_out: f_out.write("// NO_PROCESSABLE_ITEMS_IN_SOURCE_FILE\nEND_SENTENCE\n")
                print(f"Wrote placeholder output file '{output_llm_file_path.name}'.")
             except Exception as e_write:
                print(f"Error writing placeholder output file: {e_write}", file=sys.stderr)
                return False, False
        return False, True 

    num_items_to_output_in_file = len(all_items)
    if item_limit is not None:
        num_items_to_output_in_file = min(item_limit, len(all_items))
        print(f"  Limiting output to the first {num_items_to_output_in_file} items (markers or sentences) for this run.")

    all_output_blocks_for_book: List[str] = []
    llm_calls_made_this_file = 0

    for current_item_idx in range(num_items_to_output_in_file):
        item_data = all_items[current_item_idx]
        item_type = item_data["type"]
        item_text = item_data["text"]

        if item_type == "marker":
            marker_block = f"CHAPTER_MARKER_DIRECT:: {item_text}\nEND_SENTENCE\n"
            all_output_blocks_for_book.append(marker_block)
            print(f"  Writing direct chapter marker ({current_item_idx + 1}/{num_items_to_output_in_file}): '{item_text[:60]}...'")
        
        elif item_type == "sentence":
            target_sentence_text = item_text
            
            preceding_sentences = []
            for k_offset in range(1, num_context_sentences + 1):
                prev_idx = current_item_idx - k_offset
                if prev_idx >= 0 and all_items[prev_idx]["type"] == "sentence":
                    preceding_sentences.insert(0, all_items[prev_idx]["text"])
                elif prev_idx < 0: break
            preceding_context_str = "\n".join(preceding_sentences) if preceding_sentences else "[NO PRECEDING CONTEXT]"

            succeeding_sentences = []
            for k_offset in range(1, num_context_sentences + 1):
                next_idx = current_item_idx + k_offset
                if next_idx < len(all_items) and all_items[next_idx]["type"] == "sentence":
                    succeeding_sentences.append(all_items[next_idx]["text"])
                elif next_idx >= len(all_items): break
            succeeding_context_str = "\n".join(succeeding_sentences) if succeeding_sentences else "[NO SUCCEEDING CONTEXT]"
            
            print(f"  Sending to LLM ({current_item_idx + 1}/{num_items_to_output_in_file}): '{target_sentence_text[:60]}...'")
            
            llm_output_core = process_sentence_with_llm(
                target_sentence_text, preceding_context_str, succeeding_context_str,
                model, args.max_api_retries, args.max_validation_retries,
                DEFAULT_RETRY_DELAY_SECONDS
            )
            llm_calls_made_this_file +=1
            
            all_output_blocks_for_book.append(llm_output_core.strip() + "\nEND_SENTENCE\n") # Append END_SENTENCE
            
            if num_items_to_output_in_file > 0 : 
                 time.sleep(API_CALL_DELAY_SECONDS)
    
    if item_limit is not None and num_items_to_output_in_file < len(all_items):
        all_output_blocks_for_book.append(f"// --- OUTPUT_LIMITED_TO_FIRST_{num_items_to_output_in_file}_ITEMS (markers or sentences) --- //\nEND_SENTENCE\n")
    elif item_limit == 0 and len(all_items) > 0 :
         all_output_blocks_for_book.append(f"// --- OUTPUT_LIMITED_TO_0_ITEMS (NO LLM CALLS MADE, NO MARKERS WRITTEN) --- //\nEND_SENTENCE\n")

    try:
        llm_output_dir.mkdir(parents=True, exist_ok=True)
        with open(output_llm_file_path, 'w', encoding='utf-8') as f_out:
            if not all_output_blocks_for_book :
                 if item_limit == 0 and len(all_items) > 0:
                     f_out.write(f"// --- OUTPUT_LIMITED_TO_0_ITEMS --- //\nEND_SENTENCE\n")
                 else:
                     f_out.write(f"// NO_OUTPUT_BLOCKS_COLLECTED_FOR_FILE (items to output: {num_items_to_output_in_file})\nEND_SENTENCE\n")
            else:
                f_out.write("\n".join(all_output_blocks_for_book))

        if item_limit == 0:
            print(f"Successfully wrote placeholder file (0 items processed) to '{output_llm_file_path.name}'")
        else:
            print(f"Successfully wrote {len(all_output_blocks_for_book)} blocks ({llm_calls_made_this_file} LLM calls) to '{output_llm_file_path.name}'")
        return False, True
    except Exception as e:
        print(f"Error writing output file '{output_llm_file_path.name}': {e}", file=sys.stderr)
        return False, False

def main():
    dotenv_path = Path('.') / '.env'
    if dotenv_path.is_file():
        load_dotenv(dotenv_path=dotenv_path)
        print(f"INFO: Attempted to load API key from '{dotenv_path.resolve()}'.")
    else:
        print(f"INFO: No .env file found at '{dotenv_path.resolve()}'.")

    parser = argparse.ArgumentParser(description="Process staged text files with an LLM to generate .llm.txt format.")
    parser.add_argument("--api_key", help="Google Gemini API Key. Overrides .env and GOOGLE_API_KEY env variable if provided.")
    parser.add_argument("--project_config", default="config.toml", help="Path to the project's main config.toml file.")
    parser.add_argument("--input_staged_subdir", default=DEFAULT_STAGED_DIR_NAME, help=f"Subdirectory for input .txt files (default: {DEFAULT_STAGED_DIR_NAME})")
    parser.add_argument("--output_llm_subdir", default=DEFAULT_LLM_OUTPUT_DIR_NAME, help=f"Subdirectory for output .llm.txt files (default: {DEFAULT_LLM_OUTPUT_DIR_NAME})")
    parser.add_argument("--force", action="store_true", help="Force reprocessing even if output .llm.txt files exist.")
    parser.add_argument("--max_api_retries", type=int, default=DEFAULT_MAX_API_RETRIES, help=f"Max retries for basic API calls (default: {DEFAULT_MAX_API_RETRIES}).")
    parser.add_argument("--max_validation_retries", type=int, default=DEFAULT_MAX_VALIDATION_RETRIES, help=f"Max retries with corrective prompts if LLM output fails validation (default: {DEFAULT_MAX_VALIDATION_RETRIES}). Set to 1 for no corrective retries.")
    parser.add_argument("--context_sents", type=int, default=2, help="Number of preceding/succeeding sentences for context (default: 2).")
    parser.add_argument("--limit_items", type=int, default=None, help="Output only the first N items (markers or sentences) from each book. Default: process all. Use 0 to write placeholder files without LLM calls.")
    
    args = parser.parse_args()

    api_key_to_use = None
    if args.api_key:
        api_key_to_use = args.api_key
        print("INFO: Using API key from --api_key command-line argument.")
    elif os.getenv("GOOGLE_API_KEY"):
        api_key_to_use = os.getenv("GOOGLE_API_KEY")
        print("INFO: Using API key from GOOGLE_API_KEY environment variable (could be from shell or .env).")
    
    if not api_key_to_use:
        print("ERROR: API Key not provided. Set GOOGLE_API_KEY in your shell environment, in a .env file, or use the --api_key argument.", file=sys.stderr)
        sys.exit(1)

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
    
    print(f"Found {len(staged_files_to_process)} files to process in '{staged_input_dir}'.")
    print(f"Output will be written to '{llm_output_dir}'.")
    if args.limit_items is not None:
        print(f"PROCESSING MODE: Output limited to --limit_items={args.limit_items} per file.")
    print("---")

    total_successful_ops = 0
    total_skipped_ops = 0
    total_error_ops = 0

    for staged_file in staged_files_to_process:
        was_skipped, op_successful = process_book_file(
            staged_file, llm_output_dir, model, args,
            num_context_sentences=args.context_sents,
            item_limit=args.limit_items
        )
        if was_skipped:
            total_skipped_ops += 1
        elif op_successful:
            total_successful_ops += 1
        else:
            total_error_ops += 1
        print("---")

    print("\nProcessing Complete.")
    print(f"Successfully processed/wrote: {total_successful_ops} book(s) / file operation(s).")
    print(f"Skipped (output existed and not in limit/force mode): {total_skipped_ops} book(s).")
    if total_error_ops > 0:
        print(f"Encountered errors during file operations for: {total_error_ops} book(s).")
    else:
        print("No file operation errors encountered.")

if __name__ == "__main__":
    main()