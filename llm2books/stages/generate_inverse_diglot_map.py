# llm2books/stages/generate_inverse_diglot_map.py

# --- Imports ---
import re
from typing import Any, Dict, List, Optional, Tuple

# --- Relative Imports ---
from .base import LLMStage
from .. import llm_prompts

class GenerateInverseDiglotMap(LLMStage):
    """
    Stage 9: Generates an "inverse diglot map" for the Moderate Spanish (simpler_text)
    of each advanced segment. The map provides a simple English substitute for each
    Spanish word in the phrase.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=9,
            stage_name="GenerateInverseDiglotMap",
            parser_type="block", # The LLM response for each ID is a multi-line block
        )

    # --- LLMStage Abstract Method Implementations ---

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for generating the inverse diglot map."""
        return llm_prompts.load_prompt_template("new_stage9_inverse_diglot_prompt.txt")

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """
        Prepares all 'simpler_text' segments from one sentence as an atomic unit.
        Returns the prompt parts and the total estimated tokens for this unit.
        """
        adv_segments = block.get("adv_spanish_segments", [])
        if not adv_segments:
            return None, 0

        prepared_items = []
        full_prompt_text_for_unit = []
        s_id_num = block["original_sentence_s_id"].replace("S", "")

        for seg in adv_segments:
            simpler_text = seg.get("simpler_text", "").strip()
            if not simpler_text:
                continue

            llm_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            prompt_block = f"{llm_id}: {simpler_text}"

            prepared_items.append({
                "llm_id": llm_id,
                "prompt_text": prompt_block
            })
            full_prompt_text_for_unit.append(prompt_block)

        if not prepared_items:
            return None, 0

        token_estimate = self._estimate_tokens("\n".join(full_prompt_text_for_unit))
        return prepared_items, token_estimate

    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        """
        Parses the LLM's response and adds the `inverse_diglot_map` to each
        advanced segment in the sentence block.
        """
        s_id_num = block["original_sentence_s_id"].replace("S", "")
        # The mapping regex: `spanish_word -> english_substitute`
        mapping_regex = re.compile(r"^\s*([^->]+?)\s*->\s*(.+)$")

        for seg in block.get("adv_spanish_segments", []):
            # The lookup_id is what we expect to find in the keys of the llm_response dict
            lookup_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            
            # The response for one segment is a multi-line string block
            segment_mapping_text = llm_response.get(lookup_id, "")
            
            inverse_map = {}
            for line in segment_mapping_text.splitlines():
                match = mapping_regex.match(line.strip())
                if match:
                    # Key is the exact spanish word, value is the english substitute
                    spanish_word = match.group(1).strip()
                    english_sub = match.group(2).strip()
                    inverse_map[spanish_word] = english_sub
            
            # Add the newly created map to the segment object.
            # The field will be created if it doesn't exist.
            seg["inverse_diglot_map"] = inverse_map

        # Mark this stage as complete for the block
        status_key = f"stage{self.stage_number}"
        block.setdefault("llm_call_status", {})[status_key] = "COMPLETED_LLM"