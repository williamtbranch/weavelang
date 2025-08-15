# llm2books/stages/generate_diglot_map.py
import re
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts

class GenerateDiglotMap(LLMStage):
    """
    Stage 4: Generates a word-for-word mapping (Diglot Map) from the
    base language to the simple target language for each segment.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=4,
            stage_name="GenerateDiglotMap"
        )
        # The LLM will return a multi-line block for each segment's map.
        self.parser_type = "multi_line"

    def get_system_prompt(self) -> str:
        """Loads the system prompt for this stage."""
        return llm_prompts.get_system_prompt(
            "generate_diglot_map", 
            self.resources["language_config"]
        )

    def prepare_items_for_llm(self, book_data: Dict) -> List[Dict]:
        """
        Creates a prompt for each segment containing both the base and simple
        target text, asking for a word-for-word mapping.
        """
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), None)
                simple_target_tier = next((t for t in block["tiers"] if t["tier_id"] == "simple_target"), None)

                if not base_tier or not simple_target_tier:
                    continue
                
                # Align segments by index, assuming they correspond one-to-one
                for i, base_seg in enumerate(base_tier.get("segments", [])):
                    if i < len(simple_target_tier.get("segments", [])):
                        target_seg = simple_target_tier["segments"][i]
                        
                        base_text = "".join(t['v'] for t in base_seg['tokenized_text'])
                        target_text = target_seg.get("text", "") # Use the raw text from Stage 2/3

                        if base_text.strip() and target_text.strip():
                            prompt_text = (
                                f"Base: {base_text.strip()}\n"
                                f"Target: {target_text.strip()}"
                            )
                            items_to_process.append({
                                "id": f"{block['s_id']}_{base_seg['seg_id']}",
                                "text": prompt_text
                            })
        return items_to_process

    def process_llm_responses(self, book_data: Dict, llm_responses: List[Dict]) -> Dict:
        """
        Parses the LLM's word mappings and populates the diglot map in the JSON.
        """
        response_map = {item['id']: item['llm_response'] for item in llm_responses}
        mapping_regex = re.compile(r"^\s*([^->]+?)\s*->\s*(.+)$")

        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                mappings = block.setdefault("mappings", {})
                diglot_map = mappings.setdefault("simple_target_to_base_diglot", {})
                
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), {})
                
                for seg in base_tier.get("segments", []):
                    seg_id = seg["seg_id"]
                    lookup_key = f"{block['s_id']}_{seg_id}"
                    
                    if lookup_key in response_map:
                        segment_mapping_text = response_map[lookup_key]
                        seg_entries = []
                        
                        for line in segment_mapping_text.splitlines():
                            map_match = mapping_regex.match(line.strip())
                            if map_match:
                                base_word = map_match.group(1).strip()
                                target_form = map_match.group(2).strip()
                                is_viable = target_form.upper() not in ["PROPER_NOUN", "NO_SUB"]

                                # Find the diglot_index (di) for the given base word
                                word_token = next((
                                    tok for tok in seg.get("tokenized_text", []) 
                                    if tok.get("t") == "w" and tok.get("v") == base_word
                                ), None)
                                
                                if word_token and "di" in word_token:
                                    # Format: [base_word_di, "target_lemma", "exact_target_form", is_viable_bool]
                                    # The "target_lemma" is a placeholder to be filled by the next stage.
                                    seg_entries.append([word_token["di"], "TBD", target_form, is_viable])

                        diglot_map[seg_id] = seg_entries
                
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

        return book_data