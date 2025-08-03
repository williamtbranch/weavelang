# llm2books/llm_prompts.py

from pathlib import Path
from typing import Optional
import sys

def load_prompt_template(
    prompt_name: str,
    base_asset_path: Path,
    pair_prompt_dir: Optional[str] = None
) -> str:
    """
    Loads a prompt template, implementing a specific-then-default fallback.
    ... (docstring is the same) ...
    """
    
    # 1. Attempt to find the specific, override prompt first
    if pair_prompt_dir:
        # --- THE FIX IS HERE ---
        # We join the base_asset_path directly with the pair_prompt_dir
        specific_path = base_asset_path / pair_prompt_dir / prompt_name
        # --- END FIX ---
        if specific_path.exists():
            try:
                return specific_path.read_text(encoding="utf-8")
            except Exception as e:
                print(f"ERROR: Could not read specific prompt file {specific_path}: {e}", file=sys.stderr)

    # 2. If no specific prompt was found or provided, fall back to the default
    default_path = base_asset_path / "prompts" / "_defaults" / prompt_name
    if default_path.exists():
        try:
            return default_path.read_text(encoding="utf-8")
        except Exception as e:
            raise FileNotFoundError(f"Default prompt '{default_path}' found but could not be read: {e}")

    # 3. If neither was found, raise a critical error.
    raise FileNotFoundError(f"Prompt '{prompt_name}' not found in default or specific pair directory.")