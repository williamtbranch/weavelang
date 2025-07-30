# llm2books/stages/simplify_advanced_target.py
from typing import Any, Dict, List, Optional, Tuple
from .base import LLMStage, logger
from .. import llm_prompts

def reconstruct_text_from_tokens(tokens: List[Dict[str, Any]]) -> str:
    """Helper to reconstruct a plain text string from a V2 token list."""
    return "".join(token.get("v", "") for token in tokens)

class SimplifyAdvancedTarget(LLMStage):
    """
    Stage 3b (V2): Simplifies the vocabulary of each advanced target segment.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=3,
            stage_name="SimplifyAdvancedTarget",
            parser_type="line",
        )

    def get_system_prompt(self) -> str:
        prompt_dir = self.resources["language_config"]["prompt_dir"]
        return llm_prompts.load_prompt_template(self.stage_name, prompt_dir)

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        adv_target_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "advanced_target"), None)
        if not adv_target_tier or not adv_target_tier.get("segments"):
            return None, 0

        prepared_items = []
        full_prompt_text_for_unit = []
        s_id_num = block["s_id"].replace("S", "")

        for seg in adv_target_tier["segments"]:
            adv_text = reconstruct_text_from_tokens(seg.get("tokenized_text", []))
            if not adv_text.strip(): continue

            llm_id = f"id {s_id_num}_{seg['seg_id']}".lower()
            prompt_line = f"{llm_id}: {adv_text}"
            
            prepared_items.append({"llm_id": llm_id})
            full_prompt_text_for_unit.append(prompt_line)

        token_estimate = self._estimate_tokens("\n".join(full_prompt_text_for_unit))
        return prepared_items, token_estimate

    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        s_id_num = block["s_id"].replace("S", "")
        adv_target_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "advanced_target"), {})
        simpler_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simpler_advanced_target"), {})
        
        all_simpler_texts = []
        
        # We iterate based on the structure of the advanced tier's segments
        for seg_data in adv_target_tier.get("segments", []):
            seg_id = seg_data["seg_id"]
            lookup_id = f"id {s_id_num}_{seg_id}".lower()
            
            # Reconstruct original advanced text to use as a fallback
            adv_text = reconstruct_text_from_tokens(seg_data.get("tokenized_text", []))
            simpler_text = llm_response.get(lookup_id, adv_text)
            all_simpler_texts.append(simpler_text)

            simpler_tier.setdefault("segments", []).append({
                "seg_id": seg_id,
                "post_separator": seg_data.get("post_separator", ""),
                "tokenized_text": [{"t": "b", "v": ""}, {"t": "w", "v": simpler_text}]
            })
        
        # Reconstruct and store the full text for the simpler tier
        full_simpler_text_parts = []
        for seg in simpler_tier.get("segments", []):
            text = reconstruct_text_from_tokens(seg.get("tokenized_text", []))
            sep = seg.get("post_separator", "")
            full_simpler_text_parts.append(text)
            full_simpler_text_parts.append(sep)
            
        simpler_tier["full_text"] = "".join(full_simpler_text_parts)

        status_key = f"stage{self.stage_number}b"
        block.setdefault("llm_call_status", {})[status_key] = "COMPLETED_LLM"