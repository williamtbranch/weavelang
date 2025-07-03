from typing import Any, Dict, List, Optional, Tuple
from .base import LLMStage
from .. import llm_prompts


class GenerateSimpleSpanish(LLMStage):
    """
    Stage 5b: Translates English segments into simple Spanish (L3) using an LLM.
    This stage reads from and writes to the 'stage5.json' file.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=5,
            stage_name="GenerateSimpleSpanish",
            parser_type="line",
        )

    # --- LLMStage Abstract Method Implementations ---

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for translating to simple Spanish."""
        return llm_prompts.load_prompt_template("new_stage5_translator_prompt.txt")

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """
        Prepares all English segments from one sentence as an atomic unit for translation.
        Returns the prompt parts and the total estimated tokens for this unit.
        """
        alignments = block.get("phrase_alignments_l3_to_english", [])
        if not alignments:
            return None, 0

        prepared_items = []
        full_prompt_text_for_unit = []
        s_id_num = block["original_sentence_s_id"].replace("S", "")

        for align in alignments:
            llm_id = f"id {s_id_num}_{align['segment_id']}".lower()
            prompt_line = f"{llm_id}: {align['english_span_text']}"
            
            prepared_items.append({
                "llm_id": llm_id,
                "prompt_text": prompt_line
            })
            full_prompt_text_for_unit.append(prompt_line)

        token_estimate = self._estimate_tokens("\n".join(full_prompt_text_for_unit))
        
        return prepared_items, token_estimate


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
