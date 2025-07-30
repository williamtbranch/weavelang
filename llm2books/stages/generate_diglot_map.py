# llm2books/stages/generate_diglot_map.py
import re
from typing import Any, Dict, List, Optional, Tuple
from .base import LLMStage, logger
from .. import llm_prompts

def reconstruct_text_from_tokens(tokens: List[Dict[str, Any]]) -> str:
    """Helper to reconstruct a plain text string from a V2 token list."""
    return "".join(token.get("v", "") for token in tokens)

class GenerateDiglotMap(LLMStage):
    """
    Stage 7 (V2): Generates a word-for-word mapping (Diglot Map) from the
    base language to the simple target language.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=7,
            stage_name="GenerateDiglotMap",
            parser_type="block",
        )

    def get_system_prompt(self) -> str:
        prompt_dir = self.resources["language_config"]["prompt_dir"]
        return llm_prompts.load_prompt_template(self.stage_name, prompt_dir)

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """Prepares all base segments from one sentence for diglot mapping."""
        base_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "base"), None)
        if not base_tier or not base_tier.get("segments"):
            return None, 0

        prepared_items = []
        full_prompt_text_for_unit = []
        s_id_num = block["s_id"].replace("S", "")

        for seg in base_tier["segments"]:
            base_text = reconstruct_text_from_tokens(seg.get("tokenized_text", []))
            cleaned_base_text = re.sub(r"[^\w\s-]", "", base_text).strip()
            if not cleaned_base_text:
                continue
            
            llm_id = f"id {s_id_num}_{seg['seg_id']}".lower()
            prompt_block = f"{llm_id}:\n{cleaned_base_text}"
            
            prepared_items.append({"llm_id": llm_id})
            full_prompt_text_for_unit.append(prompt_block)

        if not prepared_items: return None, 0
        token_estimate = self._estimate_tokens("\n".join(full_prompt_text_for_unit))
        return prepared_items, token_estimate

    def process_llm_response(self, block: Dict[str, Any], llm_response: Dict[str, str]) -> None:
        """Parses the LLM response and populates the diglot map in the mappings object."""
        mappings = block.setdefault("mappings", {})
        diglot_map = mappings.setdefault("simple_target_to_base_diglot", {})
        
        s_id_num = block["s_id"].replace("S", "")
        base_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "base"), {})
        
        mapping_regex = re.compile(r"^\s*([^->]+?)\s*->\s*(.+)$")

        for seg in base_tier.get("segments", []):
            seg_id = seg["seg_id"]
            lookup_id = f"id {s_id_num}_{seg_id}".lower()
            segment_mapping_text = llm_response.get(lookup_id, "")
            
            seg_entries = []
            base_word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok["t"] == "w"]

            for line in segment_mapping_text.splitlines():
                map_match = mapping_regex.match(line.strip())
                if map_match:
                    # The LLM gives us the base word and the target form.
                    base_word_from_llm = map_match.group(1).strip()
                    target_form = map_match.group(2).strip()
                    
                    # Find the diglot_index of this base word in our token list
                    di_tuple = next(
                        ((i, tok.get("di")) for i, tok in enumerate(base_word_tokens) if tok.get("v") == base_word_from_llm),
                        None
                    )
                    if di_tuple is None: continue # Skip if word not found
                    
                    base_word_index = di_tuple[1]
                    is_viable = target_form not in ["PROPER_NOUN", "NO_SUB"]
                    
                    # Format: [base_word_di, target_lemma_id, "exact_target_form", is_viable_bool]
                    # The target_lemma_id (placeholder 0) will be filled in by the next stage.
                    seg_entries.append([base_word_index, 0, target_form, is_viable])

            diglot_map[seg_id] = seg_entries

        status_key = f"stage{self.stage_number}"
        block.setdefault("llm_call_status", {})[status_key] = "COMPLETED_LLM"