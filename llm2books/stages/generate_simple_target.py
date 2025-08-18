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
        self.parser_type = "single_line"

    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("generate_simple_target", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), None)
                if not base_tier: continue
                
                for seg in base_tier.get("segments", []):
                    original_text = "".join(t['v'] for t in seg['tokenized_text'])
                    if original_text.strip():
                        items_to_process.append({
                            "id": f"{block['s_id']}_{seg['seg_id']}",
                            "text": original_text
                        })
        return items_to_process

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """
        Adds the 'simple_target' tier to a single sentence block.
        """
        if any(t['tier_id'] == 'simple_target' for t in block.get("tiers", [])):
            return block # Already processed

        simple_target_tier = {"tier_id": "simple_target", "segments": []}
        base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), {})
        
        segment_texts = []
        for seg in base_tier.get("segments", []):
            lookup_key = f"{block['s_id']}_{seg['seg_id']}"
            original_text = "".join(t['v'] for t in seg['tokenized_text'])
            translated_text = llm_results.get(lookup_key, original_text)
            segment_texts.append(translated_text)
            
            simple_target_tier["segments"].append({
                "seg_id": seg['seg_id'],
                "text": translated_text,
                "tokenized_text": [{"t": "b", "v": translated_text}]
            })
        
        simple_target_tier["full_text"] = "".join(segment_texts)
        block.setdefault("tiers", []).append(simple_target_tier)
        block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block