# Filename: llm_prompts.py
# Purpose: To load and format prompts for the multi-call LLM processing pipeline.

#import os
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


# --- Helper Function to Load Prompt Templates ---
def load_prompt_template(filename: str) -> Optional[str]:
    """Loads a prompt template from the specified file in the PROMPT_DIR."""
    file_path = PROMPT_DIR / filename
    if not file_path.exists():
        # Using print and sys.exit here because logging might not be fully configured yet
        # or we want a more immediate, critical failure if a prompt is missing.
        print(f"ERROR: Prompt template file not found: {file_path}", file=sys.stderr)
        return None  # Indicate failure
    try:
        with open(file_path, "r", encoding="utf-8") as f:
            return f.read()
    except Exception as e:
        print(
            f"ERROR: Could not read prompt template file {file_path}: {e}",
            file=sys.stderr,
        )
        return None


# --- Formatting Functions for Each Call ---


def format_call1_advs_advsl_prompt(
    sentences_batch: List[Dict[str, str]], prompt_template: str
) -> Optional[str]:
    if not prompt_template:
        return None
    formatted_input_lines = []
    for sentence_data in sentences_batch:
        delimited_text = "{{" + sentence_data["eng_text"] + "}}"
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        formatted_input_lines.append(f"TEXT: {delimited_text}")
        formatted_input_lines.append("")
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace(
        "{batched_input_sentences_with_ids_and_delimited_text}", batched_input_str
    )


def format_call2_simsL2_L2simsl_prompt(
    sentences_batch: List[Dict[str, str]], prompt_template: str
) -> Optional[str]:
    return format_call1_advs_advsl_prompt(sentences_batch, prompt_template)


def format_call3_advs_segments_lemmas_prompt(
    sentences_batch_data: List[Dict[str, Any]], prompt_template: str
) -> Optional[str]:
    if not prompt_template:
        return None
    formatted_input_lines = []
    for sentence_data in sentences_batch_data:
        eng_text_delimited = "{{" + sentence_data["eng_text"] + "}}"
        advs_text_delimited = "{{" + sentence_data["advs_text"] + "}}"
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        formatted_input_lines.append(f"ORIGINAL_ENGLISH_TEXT: {eng_text_delimited}")
        formatted_input_lines.append(f"AdvS_TEXT_TO_SEGMENT: {advs_text_delimited}")
        formatted_input_lines.append("")
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace(
        "{batched_input_advs_sentences_with_ids_and_eng_context}", batched_input_str
    )


def format_call4_simpler_advs_segments_lemmas_prompt(
    sentences_batch_data: List[Dict[str, Any]], prompt_template: str
) -> Optional[str]:
    if not prompt_template:
        return None
    formatted_input_lines = []
    for sentence_data in sentences_batch_data:
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        if sentence_data.get("eng_text"):
            eng_text_val = "{{" + sentence_data["eng_text"] + "}}"
            formatted_input_lines.append(f"ORIGINAL_ENGLISH_TEXT: {eng_text_val}")
        if sentence_data.get("advs_text_full"):
            advs_text_full_val = "{{" + sentence_data["advs_text_full"] + "}}"
            formatted_input_lines.append(f"ORIGINAL_AdvS_TEXT: {advs_text_full_val}")

        formatted_input_lines.append("AdvS_SEGMENTS_TO_SIMPLIFY_START::")
        for seg_data in sentence_data["advs_segments"]:
            advs_segment_text_delimited = "{{" + seg_data["text"] + "}}"
            formatted_input_lines.append(
                f"{seg_data['id']}::{advs_segment_text_delimited}"
            )
        formatted_input_lines.append("AdvS_SEGMENTS_TO_SIMPLIFY_END::")
        formatted_input_lines.append("")
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace("{batched_input_for_call4}", batched_input_str)


def format_call5_simsL3_align_lemmas_prompt(
    sentences_batch: List[Dict[str, str]], prompt_template: str
) -> Optional[str]:
    if not prompt_template:
        return None
    formatted_input_lines = []
    for sentence_data in sentences_batch:
        eng_text_delimited = "{{" + sentence_data["eng_text"] + "}}"
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        formatted_input_lines.append(f"ENG_TEXT: {eng_text_delimited}")
        formatted_input_lines.append("")
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace(
        "{batched_input_eng_sentences_with_ids}", batched_input_str
    )


def format_call6_diglotmap_prompt(
    sentences_batch_data: List[Dict[str, Any]], prompt_template: str
) -> Optional[str]:
    if not prompt_template:
        return None
    formatted_input_lines = []
    for sentence_data in sentences_batch_data:
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        eng_text_val = "{{" + sentence_data["eng_text"] + "}}"
        formatted_input_lines.append(f"ENG_TEXT: {eng_text_val}")

        formatted_input_lines.append("SimS_L3_SEGMENTS_START::")
        for seg_data in sentence_data["sims_l3_segments"]:
            seg_text_val = "{{" + seg_data["text"] + "}}"
            formatted_input_lines.append(f"{seg_data['id']}::{seg_text_val}")
        formatted_input_lines.append("SimS_L3_SEGMENTS_END::")

        formatted_input_lines.append("PHRASE_ALIGNMENTS_START::")
        for align_data in sentence_data["phrase_alignments_l3_to_eng"]:
            # This aggressive debug block is no longer needed but was causing issues with type checking in some cases
            # The logic that populates phrase_alignments_l3_to_eng should ensure it's a list of dicts.
            # If not, it means the LLM response parsing failed for call 5.
            # Removing the specific error checks for non-dict here as the main script will halt if parsing fails.

            sims_seg_text_delimited = "{{" + align_data["sims_l3_segment_text"] + "}}"
            eng_span_text_delimited = "{{" + align_data["eng_span_text"] + "}}"
            formatted_input_lines.append(
                f"{align_data['id']} ~ {sims_seg_text_delimited} ~ {eng_span_text_delimited}"
            )
        formatted_input_lines.append("PHRASE_ALIGNMENTS_END::")

        formatted_input_lines.append("L3_SimSL_PER_SEGMENT_START::")
        sorted_segment_ids = sorted(
            sentence_data["l3_simsl_per_segment"].keys(),
            key=lambda x: int(x[1:])
            if x.startswith("S") and x[1:].isdigit()
            else float("inf"),
        )
        for seg_id in sorted_segment_ids:
            lemmas_str = sentence_data["l3_simsl_per_segment"][seg_id]
            formatted_input_lines.append(f"{seg_id}::{lemmas_str}")
        formatted_input_lines.append("L3_SimSL_PER_SEGMENT_END::")
        formatted_input_lines.append("")
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace("{batched_input_for_call6}", batched_input_str)


if __name__ == "__main__":
    # This block is for local testing and should only ensure dummy prompt files exist.
    # The actual prompt templates from the user's files will be loaded by the main script.
    print(f"Creating dummy prompt files in: {PROMPT_DIR.resolve()}")
    PROMPT_DIR.mkdir(parents=True, exist_ok=True)

    dummy_templates = {
        PROMPT_CALL1_FILENAME: "Call 1 Template\nInput:\n{batched_input_sentences_with_ids_and_delimited_text}\nAdvS:: {{TEST}}\nAdvSL:: test\nEND_SENTENCE",
        PROMPT_CALL2_FILENAME: "Call 2 Template\nInput:\n{batched_input_sentences_with_ids_and_delimited_text}\nSimS:: {{TEST}}\nL2_SimSL:: test\nEND_SENTENCE",
        PROMPT_CALL3_FILENAME: "Call 3 Template\nInput:\n{batched_input_advs_sentences_with_ids_and_eng_context}\nAdvS_Segments_Data::\nA1_TEXT::{{TEST}}\nA1_LEMMAS::test\nEND_SENTENCE",
        PROMPT_CALL4_FILENAME: "Call 4 Template\nInput:\n{batched_input_for_call4}\nAdvS_Segments_Data::\nA1_TEXT::{{TEST}}\nA1_LEMMAS::test\nA1_SIMPLER_TEXT::{{TEST}}\nA1_SIMPLER_LEMMAS::test\nEND_SENTENCE",
        PROMPT_CALL5_FILENAME: "Call 5 Template\nInput:\n{batched_input_eng_sentences_with_ids}\nSimS_Segments::\nS1::{{TEST}}\nPHRASE_ALIGN_SimS_L3_TO_ENG::\nS1 ~ {{TEST}} ~ {{TEST}}\nL3_SimSL_PER_SEGMENT::\nS1::test\nEND_SENTENCE",
        PROMPT_CALL6_FILENAME: "Call 6 Template\nInput:\n{batched_input_for_call6}\nDIGLOT_MAP::\nS1::TEST->test(Test)(Y)\nEND_SENTENCE",
    }

    for fname, content in dummy_templates.items():
        file_path = PROMPT_DIR / fname
        if not file_path.exists():  # Only create if they don't exist
            with open(file_path, "w", encoding="utf-8") as f:
                f.write(content)

    # --- Test Formatter Functions (using the dummy templates) ---
    print("\n--- Testing Call 1 Formatter ---")
    sentences1 = [
        {"id": "bk1_s1", "eng_text": "First sentence."},
        {"id": "bk1_s2", "eng_text": 'Second sentence with "quotes".'},
    ]
    template1 = load_prompt_template(PROMPT_CALL1_FILENAME)
    if template1:
        print(format_call1_advs_advsl_prompt(sentences1, template1))
    else:
        print("Skipping Call 1 formatter test due to missing template.")

    print("\n--- Testing Call 3 Formatter ---")
    sentences3_data = [
        {"id": "bk1_s1", "eng_text": "First eng.", "advs_text": "First advs."},
        {"id": "bk1_s2", "eng_text": "Second eng.", "advs_text": "Second advs."},
    ]
    template3 = load_prompt_template(PROMPT_CALL3_FILENAME)
    if template3:
        print(format_call3_advs_segments_lemmas_prompt(sentences3_data, template3))
    else:
        print("Skipping Call 3 formatter test due to missing template.")

    print("\n--- Testing Call 4 Formatter ---")
    sentences4_data = [
        {
            "id": "bk1_s1",
            "eng_text": "Full eng text for context.",
            "advs_text_full": "Full advs text for context.",
            "advs_segments": [
                {"id": "A1", "text": "AdvS Seg 1-1 text"},
                {"id": "A2", "text": "AdvS Seg 1-2 text"},
            ],
        }
    ]
    template4 = load_prompt_template(PROMPT_CALL4_FILENAME)
    if template4:
        print(
            format_call4_simpler_advs_segments_lemmas_prompt(sentences4_data, template4)
        )
    else:
        print("Skipping Call 4 formatter test due to missing template.")

    print("\n--- Testing Call 5 Formatter ---")
    template5 = load_prompt_template(PROMPT_CALL5_FILENAME)
    if template5:
        print(format_call5_simsL3_align_lemmas_prompt(sentences1, template5))
    else:
        print("Skipping Call 5 formatter test due to missing template.")

    print("\n--- Testing Call 6 Formatter ---")
    sentences6_data = [
        {
            "id": "bk1_s1",
            "eng_text": "The quick brown fox.",
            "sims_l3_segments": [
                {"id": "S1", "text": "El rápido zorro"},
                {"id": "S2", "text": "marrón."},
            ],
            "phrase_alignments_l3_to_eng": [
                {
                    "id": "S1",
                    "sims_l3_segment_text": "El rápido zorro",
                    "eng_span_text": "The quick brown",
                },
                {
                    "id": "S2",
                    "sims_l3_segment_text": "marrón.",
                    "eng_span_text": "fox.",
                },
            ],
            "l3_simsl_per_segment": {"S1": "el rápido zorro", "S2": "marrón"},
        }
    ]
    template6 = load_prompt_template(PROMPT_CALL6_FILENAME)
    if template6:
        print(format_call6_diglotmap_prompt(sentences6_data, template6))
    else:
        print("Skipping Call 6 formatter test due to missing template.")
