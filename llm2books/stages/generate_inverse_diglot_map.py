import re
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts

class GenerateInverseDiglotMap(LLMStage):
    """
    Stage 5: Generates an "inverse diglot map" from the simpler advanced target
    tier back to the base language. Uses a robust "fill-in-the-blank" prompt.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=7,
            stage_name="GenerateInverseDiglotMap"
        )
        self.parser_type = "multi_line"

    #
    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("generate_inverse_diglot_map", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        items_to_process = []
        word_regex = re.compile(r'\S+')

        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                simpler_adv_tier = next((t for t in block["tiers"] if t["tier_id"] == "simpler_advanced_target"), None)
                if not simpler_adv_tier: continue

                for seg in simpler_adv_tier.get("segments", []):
                    target_text = "".join(t['v'] for t in seg.get("tokenized_text", []))
                    words = word_regex.findall(target_text)
                    if not words: continue
                    
                    # --- THIS IS THE FIX ---
                    # Create the "fill-in-the-blank" list
                    fill_in_blank_lines = "\n".join(f"{word} ->" for word in words)
                    # Add the MAPPINGS header that our universal parser now expects.
                    prompt_text = f"MAPPINGS:\n{fill_in_blank_lines}"
                    # --- END OF FIX ---

                    items_to_process.append({
                        "id": f"{block['s_id']}_{seg['seg_id']}",
                        "text": prompt_text,
                    })
        return items_to_process

    #
    def get_system_prompt(self) -> str:
        # We need a new, dedicated prompt for this task.
        return llm_prompts.get_system_prompt("generate_inverse_phrase_map", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                simpler_adv_tier = next((t for t in block["tiers"] if t["tier_id"] == "simpler_advanced_target"), None)
                if not simpler_adv_tier: continue
                
                # We now send the full, clean text of the tier to the LLM
                prompt_text = simpler_adv_tier["full_text"]
                
                items_to_process.append({
                    "id": block['s_id'],
                    "text": prompt_text
                })
        return items_to_process

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """
        Stores the raw LLM phrase map output for the next stage to process.
        """
        s_id = block['s_id']
        if s_id in llm_results:
            # We no longer parse here. We just store the raw lines.
            raw_map_str = llm_results[s_id]
            
            # Basic validation can still happen here
            if '->' not in raw_map_str:
                 logger.warning(f"S_ID {s_id}: LLM response for inverse phrase map did not contain '->'. Storing empty map.")
                 block.setdefault("mappings", {})["raw_inverse_phrase_map"] = []
            else:
                block.setdefault("mappings", {})["raw_inverse_phrase_map"] = raw_map_str.splitlines()

        block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block