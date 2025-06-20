from typing import Any, Dict, List, Optional

from .base import LLMStage
from .. import llm_prompts


class SimplifyAdvSpanish(LLMStage):
    """
    Stage 3b: Simplifies the vocabulary of each advanced Spanish segment using an LLM.
    This stage reads from and writes to the same 'stage3.json' file created by 3a.
    """

    def __init__(self, book_stem: str, config: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            config=config,
            common_resources=common_resources,
            stage_number=3,
            stage_name="SimplifyAdvSpanish",
            batch_size=2,
            parser_type="line", # The prompt asks for output in blocks per ID
        )

    # --- LLMStage Abstract Method Implementations ---

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for simplifying segments."""
        return llm_prompts.load_prompt_template("new_stage3_simplifier_prompt.txt")

    def prepare_llm_input(
        self, block: Dict[str, Any], s_idx: int
    ) -> Optional[List[Dict[str, Any]]]:
        segments = block.get("adv_spanish_segments", [])
        if not segments:
            return None

        prepared_items = []
        s_id_num = block["original_sentence_s_id"].replace("S", "")

        for seg in segments:
            llm_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            prepared_items.append({
                "llm_id": llm_id,
                "prompt_text": f"{llm_id}: {seg['advanced_text']}"
            })
        return prepared_items

    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        s_id_num = block["original_sentence_s_id"].replace("S", "")
        for seg in block.get("adv_spanish_segments", []):
            lookup_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            simpler_text = llm_response.get(lookup_id, seg.get("advanced_text", ""))
            seg["simpler_text"] = simpler_text

        status_key = f"stage{self.stage_number}b"
        block.setdefault("llm_call_status", {})[status_key] = "COMPLETED_LLM"