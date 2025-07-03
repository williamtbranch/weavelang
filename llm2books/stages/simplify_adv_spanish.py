from typing import Any, Dict, List, Optional, Tuple
from .base import LLMStage
from .. import llm_prompts


class SimplifyAdvSpanish(LLMStage):
    """
    Stage 3b: Simplifies the vocabulary of each advanced Spanish segment using an LLM.
    This stage reads from and writes to the same 'stage3.json' file created by 3a.
    """

    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=3,
            stage_name="SimplifyAdvSpanish",
            parser_type="line",
        )

    # --- LLMStage Abstract Method Implementations ---

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for simplifying segments."""
        template_path = llm_prompts.PROMPT_DIR / "new_stage3_simplifier_prompt.txt"
        try:
            return template_path.read_text(encoding="utf-8")
        except Exception as e:
            # Use the logger from the base class
            self.logger.critical(f"Could not load system prompt {template_path.name}: {e}")
            return ""

    # This method is now correctly indented to be part of the class
    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """
        Prepares all advanced segments from one sentence as an atomic unit.
        Returns the prompt parts and the total estimated tokens for this unit.
        """
        segments = block.get("adv_spanish_segments", [])
        if not segments:
            return None, 0

        prepared_items = []
        full_prompt_text_for_unit = []
        s_id_num = block["original_sentence_s_id"].replace("S", "")

        for seg in segments:
            llm_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            prompt_line = f"{llm_id}: {seg['advanced_text']}"
            
            prepared_items.append({
                "llm_id": llm_id,
                "prompt_text": prompt_line
            })
            full_prompt_text_for_unit.append(prompt_line)

        # Estimate tokens based on the combined text of all parts for this unit
        token_estimate = self._estimate_tokens("\n".join(full_prompt_text_for_unit))
        
        return prepared_items, token_estimate

    # This method is also correctly indented now
    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        """
        Updates the block with the simplified text from the LLM's response.
        """
        s_id_num = block["original_sentence_s_id"].replace("S", "")
        for seg in block.get("adv_spanish_segments", []):
            lookup_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            # Default to the original text if the LLM fails to provide a simplification for a specific ID
            simpler_text = llm_response.get(lookup_id, seg.get("advanced_text", ""))
            seg["simpler_text"] = simpler_text

        # Mark this specific sub-stage as complete for the block
        status_key = f"stage{self.stage_number}b"
        block.setdefault("llm_call_status", {})[status_key] = "COMPLETED_LLM"