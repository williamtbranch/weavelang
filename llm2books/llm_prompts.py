# Filename: llm2books/llm_prompts.py
# Purpose: To load and format prompts for the multi-call LLM processing pipeline.

from pathlib import Path
from typing import List, Dict, Any, Optional
import sys

# --- NEW: Generic prompt filenames. The specific language pair is handled by the directory path. ---
# These names should reflect the generic PURPOSE of the prompt.
GENERIC_PROMPT_FILENAMES = {
    "GenerateAdvancedTarget": "stage1_gen_adv_target.txt",
    "SimplifyAdvancedTarget": "stage3b_simplify_adv_target.txt",
    "GenerateSimpleTarget": "stage5b_gen_simple_target.txt",
    "GenerateDiglotMap": "stage7_gen_diglot_map.txt",
    "GenerateInverseDiglotMap": "stage9_gen_inv_diglot_map.txt",
    # Add other LLM stages here as they are refactored
}

# --- MODIFIED: Helper function now takes the directory as an argument ---
def load_prompt_template(stage_name: str, prompt_dir: Path) -> Optional[str]:
    """Loads a prompt template for a given stage from the specified language-pair directory."""
    
    generic_filename = GENERIC_PROMPT_FILENAMES.get(stage_name)
    if not generic_filename:
        print(f"ERROR: No prompt filename defined for stage '{stage_name}' in llm_prompts.py.", file=sys.stderr)
        return None

    file_path = prompt_dir / generic_filename
    if not file_path.exists():
        print(f"ERROR: Prompt template file not found: {file_path}", file=sys.stderr)
        return None
    try:
        with open(file_path, "r", encoding="utf-8") as f:
            return f.read()
    except Exception as e:
        print(
            f"ERROR: Could not read prompt template file {file_path}: {e}",
            file=sys.stderr,
        )
        return None

# --- NOTE on Formatting Functions ---
# The formatting functions themselves (e.g., format_call1_advs_advsl_prompt) are currently language-agnostic.
# They just insert text into placeholders. For now, we will rename them to match the new generic stage names.
# If a future language requires a completely different prompt structure, we would handle that with a new function.

def format_GenerateAdvancedTarget_prompt(
    sentences_batch: List[Dict[str, str]], prompt_template: str
) -> Optional[str]:
    if not prompt_template:
        return None
    formatted_input_lines = []
    for sentence_data in sentences_batch:
        delimited_text = "{{" + sentence_data["eng_text"] + "}}" # This will become "base_text"
        formatted_input_lines.append(f"--- INPUT (ID: {sentence_data['id']}) ---")
        formatted_input_lines.append(f"TEXT: {delimited_text}")
        formatted_input_lines.append("")
    batched_input_str = "\n".join(formatted_input_lines)
    return prompt_template.replace(
        "{batched_input_sentences_with_ids_and_delimited_text}", batched_input_str
    )

# ... We would continue to rename the other formatters here as we refactor each stage ...
# For now, this is enough to establish the new pattern. The old formatters will be removed
# as we refactor the stages that use them.