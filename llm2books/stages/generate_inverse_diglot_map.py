# llm2books/stages/generate_inverse_diglot_map.py
import re
from typing import Any, Dict, List, Optional, Tuple
from .base import LLMStage, logger
from .. import llm_prompts

def reconstruct_text_from_tokens(tokens: List[Dict[str, Any]]) -> str:
    """Helper to reconstruct a plain text string from a V2 token list."""
    return "".join(token.get("v", "") for token in tokens)

class GenerateInverseDiglotMap(LLMStage):
    """
    Stage 9 (V2): Generates an "inverse diglot map" for the simpler_advanced_target tier.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=9,
            stage_name="GenerateInverseDiglotMap",
            parser_type="block",
        )

    def get_system_prompt(self) -> str:
        prompt_dir = self.resources["language_config"]["prompt_dir"]
        return llm_prompts.load_prompt_template(self.stage_name, prompt_dir)

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """Prepares all simpler_advanced_target segments for the LLM."""
        simpler_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simpler_advanced_target"), None)
        if not simpler_tier or not simpler_tier.get("segments"):
            return None, 0

        prepared_items = []
        full_prompt_text_for_unit = []
        s_id_num = block["s_id"].replace("S", "")

        for seg in simpler_tier["segments"]:
            target_text = reconstruct_text_from_tokens(seg.get("tokenized_text", []))
            if not target_text.strip(): continue

            llm_id = f"id {s_id_num}_{seg['seg_id']}".lower()
            prompt_block = f"{llm_id}: {target_text}"
            
            prepared_items.append({"llm_id": llm_id})
            full_prompt_text_for_unit.append(prompt_block)

        if not prepared_items: return None, 0
        token_estimate = self._estimate_tokens("\n".join(full_prompt_text_for_unit))
        return prepared_items, token_estimate

    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        """Parses the LLM response and populates the inverse diglot map."""
        mappings = block.setdefault("mappings", {})
        inv_diglot_map = mappings.setdefault("adv_target_to_base_inv_diglot", {})
        
        s_id_num = block["s_id"].replace("S", "")
        simpler_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simpler_advanced_target"), {})
        
        mapping_regex = re.compile(r"^\s*([^->]+?)\s*->\s*(.+)$")

        for seg in simpler_tier.get("segments", []):
            seg_id = seg["seg_id"]
            lookup_id = f"id {s_id_num}_{seg_id}".lower()
            segment_mapping_text = llm_response.get(lookup_id, "")
            
            seg_entries = []
            target_word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok["t"] == "w"]

            for line in segment_mapping_text.splitlines():
                map_match = mapping_regex.match(line.strip())
                if map_match:
                    target_word = map_match.group(1).strip()
                    base_substitute = map_match.group(2).strip()
                    
                    # Find the index of this target word in our token list
                    word_index = next(
                        (i for i, tok in enumerate(target_word_tokens) if tok.get("v") == target_word),
                        None
                    )
                    if word_index is None: continue

                    # Format: [target_word_index, target_lemma_id, "base_substitute"]
                    # The target_lemma_id (placeholder 0) will be filled in by the next stage.
                    seg_entries.append([word_index, 0, base_substitute])
            
            inv_diglot_map[seg_id] = seg_entries

        status_key = f"stage{self.stage_number}"
        block.setdefault("llm_call_status", {})[status_key] = "COMPLETED_LLM"