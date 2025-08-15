# llm2books/stages/generate_simple_target.py
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts

class GenerateSimpleTarget(LLMStage):
    """
    Stage 2: Translates the base language segments into a simple version of
    the target language.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=2,
            stage_name="GenerateSimpleTarget"
        )
        # This stage needs a multi-line parser because the LLM will be translating
        # potentially long segments of text.
        self.parser_type = "single_line"

    def get_system_prompt(self) -> str:
        """Loads the system prompt for this stage."""
        return llm_prompts.get_system_prompt(
            "generate_simple_target", 
            self.resources["language_config"]
        )

    #prepare_items_for_llm
    def prepare_items_for_llm(self, book_data: Dict) -> List[Dict]:
        """
        Extracts all base language segments to be translated.
        """
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), None)
                if not base_tier:
                    continue
                
                for seg in base_tier.get("segments", []):
                    # Reconstruct the text from the tokenized list
                    original_text = "".join(t['v'] for t in seg['tokenized_text'])
                    if original_text.strip():
                        # --- THE FIX IS HERE ---
                        # We are no longer adding the "id " prefix to the key.
                        # The key is now consistent with the pool manager.
                        item_id = f"{block['s_id']}_{seg['seg_id']}"
                        items_to_process.append({
                            "id": item_id,
                            "text": original_text
                        })
        return items_to_process
    def process_llm_responses(self, book_data: Dict, llm_responses: List[Dict]) -> Dict:
        """
        Adds a new 'simple_target' tier to each sentence block, populated
        with the translated text from the LLM and the required data structure.
        """
        response_map = {item['id']: item['llm_response'] for item in llm_responses}
        
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                if any(t['tier_id'] == 'simple_target' for t in block.get("tiers", [])):
                    continue

                simple_target_tier = {"tier_id": "simple_target", "segments": []}
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), {})
                
                segment_texts = []
                for seg in base_tier.get("segments", []):
                    lookup_key = f"{block['s_id']}_{seg['seg_id']}"
                    original_text = "".join(t['v'] for t in seg['tokenized_text'])
                    translated_text = response_map.get(lookup_key, original_text)
                    segment_texts.append(translated_text)
                    
                    # --- THE FIX IS HERE ---
                    # We now add BOTH the required 'text' field for data integrity
                    # AND the temporary placeholder 'tokenized_text' for the next stage.
                    simple_target_tier["segments"].append({
                        "seg_id": seg['seg_id'],
                        "text": translated_text, # The complete, untokenized phrase text.
                        "tokenized_text": [{"t": "b", "v": translated_text}] # The temporary placeholder.
                    })
                    # --- END OF FIX ---
                
                simple_target_tier["full_text"] = "".join(segment_texts) # Use join without space for lossless reconstruction
                block.setdefault("tiers", []).append(simple_target_tier)
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        
        return book_data