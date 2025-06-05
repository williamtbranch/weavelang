# Filename: llm_prompts.py
# Purpose: To load and format prompts for the multi-call LLM processing pipeline.

import os
from pathlib import Path
from typing import List, Dict, Any, Optional
import sys

# --- Prompt Filenames (relative to the directory of this script or a configured prompt dir) ---
PROMPT_DIR = Path(__file__).parent / "llm_prompt_templates" 

PROMPT_CALL1_FILENAME = "prompt_call1_advs_advsl_batch.txt"
PROMPT_CALL2_FILENAME = "prompt_call2_simsL2_L2simsl_batch.txt"
PROMPT_CALL3_FILENAME = "prompt_call3_advs_segments_lemmas_batch.txt"
PROMPT_CALL4_FILENAME = "prompt_call4_simpler_advs_segments_lemmas_batch.txt"
PROMPT_CALL5_FILENAME = "prompt_call5_simsL3_align_lemmas_batch.txt"
PROMPT_CALL6_FILENAME = "prompt_call6_diglotmap_batch.txt"

def load_prompt_template(filename: str) -> Optional[str]:
    """Loads a prompt template from the specified file in the PROMPT_DIR."""
    file_path = PROMPT_DIR / filename
    if not file_path.exists():
        print(f"ERROR: Prompt template file not found: {file_path}")
        return None
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            return f.read()
    except Exception as e:
        print(f"ERROR: Could not read prompt template file {file_path}: {e}")
        return None

# --- Formatting Functions for Each Call ---

def format_call1_advs_advsl_prompt(
    sentences_batch: List[Dict[str, str]], 
    prompt_template: str
) -> Optional[str]:
    if not prompt_template: return None
    formatted_input_lines = []
    for sentence_data in sentences_batch:
        # Corrected: Simple concatenation for {{text}}
        delimited_text = "{{" + sentence_data['eng_text'] + "}}"
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        formatted_input_lines.append(f"TEXT: {delimited_text}")
        formatted_input_lines.append("") 
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace("{batched_input_sentences_with_ids_and_delimited_text}", batched_input_str)

def format_call2_simsL2_L2simsl_prompt(
    sentences_batch: List[Dict[str, str]], 
    prompt_template: str
) -> Optional[str]:
    return format_call1_advs_advsl_prompt(sentences_batch, prompt_template)

def format_call3_advs_segments_lemmas_prompt(
    sentences_batch_data: List[Dict[str, Any]], 
    prompt_template: str
) -> Optional[str]:
    if not prompt_template: return None
    formatted_input_lines = []
    for sentence_data in sentences_batch_data:
        # Corrected: Simple concatenation for {{text}}
        eng_text_delimited = "{{" + sentence_data['eng_text'] + "}}"
        advs_text_delimited = "{{" + sentence_data['advs_text'] + "}}"
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        formatted_input_lines.append(f"ORIGINAL_ENGLISH_TEXT: {eng_text_delimited}")
        formatted_input_lines.append(f"AdvS_TEXT_TO_SEGMENT: {advs_text_delimited}")
        formatted_input_lines.append("")
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace("{batched_input_advs_sentences_with_ids_and_eng_context}", batched_input_str)

def format_call4_simpler_advs_segments_lemmas_prompt(
    sentences_batch_data: List[Dict[str, Any]], 
    prompt_template: str
) -> Optional[str]:
    if not prompt_template: return None
    formatted_input_lines = []
    for sentence_data in sentences_batch_data:
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        if sentence_data.get('eng_text'):
            # Corrected: Simple concatenation for {{text}}
            eng_text_val = "{{" + sentence_data['eng_text'] + "}}"
            formatted_input_lines.append(f"ORIGINAL_ENGLISH_TEXT: {eng_text_val}")
        if sentence_data.get('advs_text_full'):
            # Corrected: Simple concatenation for {{text}}
            advs_text_full_val = "{{" + sentence_data['advs_text_full'] + "}}"
            formatted_input_lines.append(f"ORIGINAL_AdvS_TEXT: {advs_text_full_val}")
        
        formatted_input_lines.append("AdvS_SEGMENTS_TO_SIMPLIFY_START::")
        for seg_data in sentence_data['advs_segments']:
            # Corrected: Simple concatenation for {{text}}
            advs_segment_text_delimited = "{{" + seg_data['text'] + "}}"
            formatted_input_lines.append(f"{seg_data['id']}::{advs_segment_text_delimited}")
        formatted_input_lines.append("AdvS_SEGMENTS_TO_SIMPLIFY_END::")
        formatted_input_lines.append("")
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace("{batched_input_for_call4}", batched_input_str)

def format_call5_simsL3_align_lemmas_prompt(
    sentences_batch: List[Dict[str, str]], 
    prompt_template: str
) -> Optional[str]:
    if not prompt_template: return None
    formatted_input_lines = []
    for sentence_data in sentences_batch:
        # Corrected: Simple concatenation for {{text}}
        eng_text_delimited = "{{" + sentence_data['eng_text'] + "}}"
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        formatted_input_lines.append(f"ENG_TEXT: {eng_text_delimited}")
        formatted_input_lines.append("") 
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace("{batched_input_eng_sentences_with_ids}", batched_input_str)

def format_call6_diglotmap_prompt(
    sentences_batch_data: List[Dict[str, Any]],
    prompt_template: str
) -> Optional[str]:
    if not prompt_template: return None
    formatted_input_lines = []
    for sentence_data in sentences_batch_data:
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        # Corrected: Simple concatenation for {{text}}
        eng_text_val = "{{" + sentence_data['eng_text'] + "}}"
        formatted_input_lines.append(f"ENG_TEXT: {eng_text_val}")
        
        formatted_input_lines.append("SimS_L3_SEGMENTS_START::")
        for seg_data in sentence_data['sims_l3_segments']:
            # Corrected: Simple concatenation for {{text}}
            seg_text_val = "{{" + seg_data['text'] + "}}"
            formatted_input_lines.append(f"{seg_data['id']}::{seg_text_val}")
        formatted_input_lines.append("SimS_L3_SEGMENTS_END::")

        formatted_input_lines.append("PHRASE_ALIGNMENTS_START::")
        for align_data in sentence_data['phrase_alignments_l3_to_eng']: 
            # ---- MORE AGGRESSIVE DEBUG BLOCK ----
            print(f"DEBUG C6 FORMATTER: Processing align_data for sentence {sentence_data.get('id', 'UNKNOWN_ID')}: {align_data}", file=sys.stderr) # Print every align_data
            sys.stderr.flush() # Ensure it prints immediately

            if not isinstance(align_data, dict):
                print(f"DEBUG C6 ERROR: align_data for sentence ID {sentence_data.get('id', 'UNKNOWN_ID')} is NOT A DICT. Value: {align_data}", file=sys.stderr)
                sys.stderr.flush()
                # To prevent crash and see if other items are okay, we could skip this problematic item
                # Or, to ensure we catch the error source, let it try and fail, but the print above helps.
                # For now, let it try to access keys to see where it fails if it's not a dict.
                # If it is not a dict, the key access below will raise a TypeError or AttributeError.
                # To be super safe for now and avoid crashing the whole batch formatter:
                if not isinstance(align_data, dict):
                    formatted_input_lines.append(f"S_ERR ~ {{MALFORMED_ALIGN_DATA_NOT_DICT}} ~ {{SKIPPED}}") # Add placeholder
                    continue


            error_in_align_data_keys = False
            required_keys_for_align = ['id', 'sims_l3_segment_text', 'eng_span_text']
            for req_key in required_keys_for_align:
                if req_key not in align_data:
                    print(f"DEBUG C6 ERROR: align_data for sentence ID {sentence_data.get('id', 'UNKNOWN_ID')} MISSING KEY '{req_key}'.", file=sys.stderr)
                    print(f"DEBUG C6 ERROR: Present keys: {list(align_data.keys())}", file=sys.stderr)
                    print(f"DEBUG C6 ERROR: Full align_data item: {align_data}", file=sys.stderr)
                    sys.stderr.flush()
                    error_in_align_data_keys = True
            
            if error_in_align_data_keys:
                # Add a placeholder to the prompt being built so we know which one failed
                # but don't crash the whole prompt formatting.
                # The LLM will likely ignore this malformed line.
                # The main script will later fail to parse this if the LLM echoes it or similar.
                err_id = align_data.get('id', 'UNKNOWN_ALIGN_ID')
                formatted_input_lines.append(f"{err_id} ~ {{ERROR_IN_ALIGN_DATA_KEYS}} ~ {{SKIPPED}}")
                continue # Skip to the next align_data item
            # ---- END MORE AGGRESSIVE DEBUG BLOCK ----
            
            # Original lines causing the error:
            sims_seg_text_delimited = "{{" + align_data['sims_l3_segment_text'] + "}}" 
            eng_span_text_delimited = "{{" + align_data['eng_span_text'] + "}}" 
            formatted_input_lines.append(f"{align_data['id']} ~ {sims_seg_text_delimited} ~ {eng_span_text_delimited}")
        formatted_input_lines.append("PHRASE_ALIGNMENTS_END::")

        formatted_input_lines.append("L3_SimSL_PER_SEGMENT_START::")
        sorted_segment_ids = sorted(sentence_data['l3_simsl_per_segment'].keys(), 
                                    key=lambda x: int(x[1:]) if x.startswith('S') and x[1:].isdigit() else float('inf'))
        for seg_id in sorted_segment_ids:
            lemmas_str = sentence_data['l3_simsl_per_segment'][seg_id]
            formatted_input_lines.append(f"{seg_id}::{lemmas_str}")
        formatted_input_lines.append("L3_SimSL_PER_SEGMENT_END::")
        formatted_input_lines.append("")
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace("{batched_input_for_call6}", batched_input_str)

if __name__ == '__main__':
    # (Test code remains the same, it will now use the corrected formatters)
    print(f"Attempting to load prompts from: {PROMPT_DIR.resolve()}")
    PROMPT_DIR.mkdir(parents=True, exist_ok=True)
    dummy_prompt_content_call1 = "Call 1 Template\nInput:\n{batched_input_sentences_with_ids_and_delimited_text}\nEnd Template"
    dummy_prompt_content_call3 = "Call 3 Template\nInput:\n{batched_input_advs_sentences_with_ids_and_eng_context}\nEnd Template"
    dummy_prompt_content_call4 = "Call 4 Template\nInput:\n{batched_input_for_call4}\nEnd Template"
    dummy_prompt_content_call5 = "Call 5 Template\nInput:\n{batched_input_eng_sentences_with_ids}\nEnd Template"
    dummy_prompt_content_call6 = "Call 6 Template\nInput:\n{batched_input_for_call6}\nEnd Template"

    if not (PROMPT_DIR / PROMPT_CALL1_FILENAME).exists():
        with open(PROMPT_DIR / PROMPT_CALL1_FILENAME, 'w') as f: f.write(dummy_prompt_content_call1)
    if not (PROMPT_DIR / PROMPT_CALL2_FILENAME).exists(): 
        with open(PROMPT_DIR / PROMPT_CALL2_FILENAME, 'w') as f: f.write(dummy_prompt_content_call1) 
    if not (PROMPT_DIR / PROMPT_CALL3_FILENAME).exists():
        with open(PROMPT_DIR / PROMPT_CALL3_FILENAME, 'w') as f: f.write(dummy_prompt_content_call3)
    if not (PROMPT_DIR / PROMPT_CALL4_FILENAME).exists():
        with open(PROMPT_DIR / PROMPT_CALL4_FILENAME, 'w') as f: f.write(dummy_prompt_content_call4)
    if not (PROMPT_DIR / PROMPT_CALL5_FILENAME).exists():
        with open(PROMPT_DIR / PROMPT_CALL5_FILENAME, 'w') as f: f.write(dummy_prompt_content_call5)
    if not (PROMPT_DIR / PROMPT_CALL6_FILENAME).exists():
        with open(PROMPT_DIR / PROMPT_CALL6_FILENAME, 'w') as f: f.write(dummy_prompt_content_call6)

    print("\n--- Testing Call 1 Formatter ---")
    sentences1 = [{"id": "bk1_s1", "eng_text": "First sentence."}, {"id": "bk1_s2", "eng_text": "Second sentence with \"quotes\"."}]
    template1 = load_prompt_template(PROMPT_CALL1_FILENAME)
    if template1: print(format_call1_advs_advsl_prompt(sentences1, template1))
    print("\n--- Testing Call 3 Formatter ---")
    sentences3_data = [{"id": "bk1_s1", "eng_text": "First eng.", "advs_text": "First advs."}, {"id": "bk1_s2", "eng_text": "Second eng.", "advs_text": "Second advs."}]
    template3 = load_prompt_template(PROMPT_CALL3_FILENAME)
    if template3: print(format_call3_advs_segments_lemmas_prompt(sentences3_data, template3))
    print("\n--- Testing Call 4 Formatter ---")
    sentences4_data = [{"id": "bk1_s1", "eng_text": "Full eng text for context.", "advs_text_full": "Full advs text for context.", "advs_segments": [{"id": "A1", "text": "AdvS Seg 1-1 text"}, {"id": "A2", "text": "AdvS Seg 1-2 text"}]}]
    template4 = load_prompt_template(PROMPT_CALL4_FILENAME)
    if template4: print(format_call4_simpler_advs_segments_lemmas_prompt(sentences4_data, template4))
    print("\n--- Testing Call 5 Formatter ---")
    template5 = load_prompt_template(PROMPT_CALL5_FILENAME)
    if template5: print(format_call5_simsL3_align_lemmas_prompt(sentences1, template5))
    print("\n--- Testing Call 6 Formatter ---")
    sentences6_data = [{"id": "bk1_s1","eng_text": "The quick brown fox.","sims_l3_segments": [{"id": "S1", "text": "El rápido zorro"},{"id": "S2", "text": "marrón."}],"phrase_alignments_l3_to_eng": [{"id": "S1", "sims_segment_text": "El rápido zorro", "eng_span_text": "The quick brown"},{"id": "S2", "sims_segment_text": "marrón.", "eng_span_text": "fox."}],"l3_simsl_per_segment": {"S1": "el rápido zorro","S2": "marrón"}}]
    template6 = load_prompt_template(PROMPT_CALL6_FILENAME)
    if template6: print(format_call6_diglotmap_prompt(sentences6_data, template6))