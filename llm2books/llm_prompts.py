# llm2books/llm_prompts.py
from pathlib import Path

from typing import Dict

def get_system_prompt(prompt_name: str, language_config: Dict) -> str:
    """
    Loads a system prompt, falling back from language-pair-specific to default.
    """
    tool_root_dir = Path(__file__).resolve().parent.parent
    base_asset_path = tool_root_dir / "assets"
    
    # Use the pre-calculated prompt directory for the specific language pair
    pair_prompt_dir_str = language_config.get("pair_prompt_dir")
    if pair_prompt_dir_str:
        specific_path = base_asset_path / pair_prompt_dir_str / f"{prompt_name}.txt"
        if specific_path.exists():
            return specific_path.read_text(encoding="utf-8")

    # Fallback to the default prompt
    default_path = base_asset_path / "prompts" / "_defaults" / f"{prompt_name}.txt"
    if default_path.exists():
        return default_path.read_text(encoding="utf-8")
        
    raise FileNotFoundError(f"Prompt '{prompt_name}' not found in default or specific directories.")