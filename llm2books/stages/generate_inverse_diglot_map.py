import re
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts, validator

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
            stage_number=5,
            stage_name="GenerateInverseDiglotMap"
        )
        self.parser_type = "multi_line"

    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("generate_inverse_phrase_map", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                simpler_adv_tier = next((t for t in block["tiers"] if t["tier_id"] == "simpler_advanced_target"), None)
                if not simpler_adv_tier: continue
                
                # ============================ START: REPLACEMENT CODE ============================
                # Clean the full_text to be a single, space-separated line
                # to ensure consistent prompting.
                prompt_text = " ".join(simpler_adv_tier.get("full_text", "").strip().split())
                # ============================= END: REPLACEMENT CODE =============================
                # --- A SMARTER FILTER ---
                # A "word" is defined as having at least one letter.
                # This allows single-word sentences but filters out punctuation or empty strings.
                if re.search(r'[a-zA-Z]', prompt_text):
                    items_to_process.append({
                        "id": block['s_id'],
                        "text": prompt_text
                    })
                else:
                    logger.info(f"S_ID {block['s_id']}: Skipping for {self.stage_name} because it contains no alphabetic content ('{prompt_text}').")

        return items_to_process


    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        s_id = block['s_id']
        
        if s_id in llm_results:
            raw_map_str = llm_results[s_id]
            if '->' not in raw_map_str:
                 logger.warning(f"S_ID {s_id}: LLM response for inverse phrase map did not contain '->'. Storing empty map.")
                 block.setdefault("mappings", {})["raw_inverse_phrase_map"] = []
            else:
                block.setdefault("mappings", {})["raw_inverse_phrase_map"] = raw_map_str.splitlines()
        else:
            logger.info(f"S_ID {s_id}: No LLM result found (was filtered). Creating empty inverse phrase map.")
            block.setdefault("mappings", {})["raw_inverse_phrase_map"] = []
        simpler_adv_tier = next((t for t in block["tiers"] if t["tier_id"] == "simpler_advanced_target"), None)
        raw_map = block.get("mappings", {}).get("raw_inverse_phrase_map", [])

        # Count word tokens in the source tier
        source_word_count = 0
        if simpler_adv_tier:
            for seg in simpler_adv_tier.get("segments", []):
                for token in seg.get("tokenized_text", []):
                    if token.get("t") == "w":
                        source_word_count += 1
        
        # If the source had words, the map must not be empty.
        if source_word_count > 0 and not raw_map:
            # We raise a validation error to be caught by the run method, which will halt the pipeline.
            raise validator.ValidationError(
                f"S_ID {s_id}: In-stage validation failed. The source tier has {source_word_count} word(s), "
                f"but an empty raw map was generated. This indicates the item was incorrectly filtered or the LLM failed."
            )


        block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block