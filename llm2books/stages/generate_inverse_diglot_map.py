import re
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts, validator

class GenerateInverseDiglotMap(LLMStage):
    """
    Stage 5: Generates an "inverse diglot map" from the simple_target
    tier back to the base language, correctly operating on a PER-SEGMENT basis.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=5,
            stage_name="GenerateInverseDiglotMap"
        )
        self.parser_type = "multi_line" # The prompt is multi-line, so this is correct

    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("generate_inverse_phrase_map", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        """
        --- CORRECTED LOGIC ---
        Prepares a separate LLM item for each SEGMENT within the simple_target tier.
        """
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                s_id = block['s_id']
                source_tier = next((t for t in block["tiers"] if t["tier_id"] == "simple_target"), None)
                if not source_tier: continue

                for seg in source_tier.get("segments", []):
                    seg_id = seg["seg_id"]
                    # Use the segment's text, not the tier's full_text
                    prompt_text = " ".join(seg.get("text", "").strip().split())
                    
                    if re.search(r'[a-zA-Z]', prompt_text):
                        items_to_process.append({
                            # The ID must be unique per segment
                            "id": f"{s_id}_{seg_id}", 
                            "text": prompt_text
                        })
                    else:
                        logger.info(f"S_ID {s_id}_{seg_id}: Skipping for {self.stage_name} because it has no alphabetic content.")
        return items_to_process

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """
        --- CORRECTED LOGIC ---
        Processes the per-segment LLM results and stores them in a structured map.
        """
        s_id = block['s_id']
        mappings = block.setdefault("mappings", {})
        map_key = "raw_simple_to_base_inv_diglot_map"
        
        # The map should be a dictionary keyed by segment ID
        raw_map_by_segment = mappings.setdefault(map_key, {})

        source_tier = next((t for t in block["tiers"] if t["tier_id"] == "simple_target"), None)
        if not source_tier:
            return block

        for seg in source_tier.get("segments", []):
            seg_id = seg["seg_id"]
            lookup_key = f"{s_id}_{seg_id}"
            
            # If we have a result for this segment, store it. Otherwise, store an empty list.
            if lookup_key in llm_results:
                raw_map_str = llm_results[lookup_key]
                raw_map_by_segment[seg_id] = raw_map_str.splitlines() if '->' in raw_map_str else []
            else:
                raw_map_by_segment[seg_id] = []
        
        # The old, incorrect validation is no longer needed here.
        # The core logic of checking for empty maps is now handled by the LLM runner's validation.

        block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block