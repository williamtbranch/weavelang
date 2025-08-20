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
            stage_number=5,
            stage_name="GenerateInverseDiglotMap"
        )
        self.parser_type = "multi_line"

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
                    
                    prompt_text = "\n".join(f"{word} ->" for word in words)
                    items_to_process.append({
                        "id": f"{block['s_id']}_{seg['seg_id']}",
                        "text": prompt_text,
                    })
        return items_to_process

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """
        Parses the LLM's response and builds the inverse diglot map.
        This version uses a sentence-level index for the words.
        """
        mappings = block.setdefault("mappings", {})
        inv_diglot_map = mappings.setdefault("simpler_adv_target_to_base_inv_diglot", {})
        
        simpler_adv_tier = next((t for t in block["tiers"] if t["tier_id"] == "simpler_advanced_target"), {})

        # --- THIS IS THE FIX for sentence-level indexing ---
        sentence_word_index_counter = 0

        for seg in simpler_adv_tier.get("segments", []):
            seg_id = seg["seg_id"]
            lookup_key = f"{block['s_id']}_{seg_id}"

            if lookup_key in llm_results:
                response_text = llm_results[lookup_key]
                response_map = {}
                for line in response_text.splitlines():
                    if '->' in line:
                        parts = line.split('->', 1)
                        if len(parts) == 2:
                            response_map[parts[0].strip()] = parts[1].strip()

                seg_entries = []
                word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok.get("t") == "w"]

                for token in word_tokens:
                    target_word = token.get("v")
                    base_sub = response_map.get(target_word, "NO_SUB")

                    if ' ' in base_sub:
                        base_sub = "NO_SUB"

                    seg_entries.append([sentence_word_index_counter, "TBD", base_sub])
                    sentence_word_index_counter += 1

                inv_diglot_map[seg_id] = seg_entries

        block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block