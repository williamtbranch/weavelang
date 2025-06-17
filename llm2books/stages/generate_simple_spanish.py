from typing import Any, Dict, List, Optional

from .base import LLMStage
from .. import llm_prompts


class GenerateSimpleSpanish(LLMStage):
    """
    Stage 5b: Translates English segments into simple Spanish (L3) using an LLM.
    This stage reads from and writes to the 'stage5.json' file.
    """

    def __init__(self, book_stem: str, config: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            config=config,
            common_resources=common_resources,
            stage_number=5,
            stage_name="GenerateSimpleSpanish",
            batch_size=2,
            parser_type="block", # The prompt asks for output in blocks per ID
        )

    # --- LLMStage Abstract Method Implementations ---

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for translating to simple Spanish."""
        return llm_prompts.load_prompt_template("new_stage5_translator_prompt.txt")

    def prepare_llm_input(
        self, block: Dict[str, Any], s_idx: int
    ) -> Optional[List[Dict[str, Any]]]:
        alignments = block.get("phrase_alignments_l3_to_english", [])
        if not alignments:
            return None

        prepared_items = []
        s_id_num = block["original_sentence_s_id"].replace("S", "")

        for align in alignments:
            llm_id = f"id {s_id_num}_{align['segment_id']}".lower()
            prepared_items.append({
                "llm_id": llm_id,
                "prompt_text": f"{llm_id}: {align['english_span_text']}"
            })
        return prepared_items

    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        s_id_num = block["original_sentence_s_id"].replace("S", "")
        all_l3_texts = []

        for align in block.get("phrase_alignments_l3_to_english", []):
            lookup_id = f"id {s_id_num}_{align['segment_id']}".lower()
            spa_text = llm_response.get(lookup_id, "")
            align["simple_spanish_text"] = spa_text
            all_l3_texts.append(spa_text)

        for seg in block.get("simple_spanish_l3_segments", []):
            lookup_id = f"id {s_id_num}_{seg['segment_id']}".lower()
            spa_text = llm_response.get(lookup_id, "")
            seg["simple_text"] = spa_text

        block["simple_spanish_l3_full"] = {
            "text": " ".join(all_l3_texts).strip(),
            "lemmas": [],
        }

        status_key = f"stage{self.stage_number}b"
        block.setdefault("llm_call_status", {})[status_key] = "COMPLETED_LLM"
