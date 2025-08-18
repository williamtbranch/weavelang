import re
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts

#
class GenerateDiglotMap(LLMStage):
    """
    Stage 4: Generates a word-for-word mapping (Diglot Map) from the
    base language to the simple target language for each segment.
    Uses a robust "fill-in-the-blank" prompt and programmatic validation.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=4,
            stage_name="GenerateDiglotMap"
        )
        self.parser_type = "multi_line"

    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("generate_diglot_map", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        """
        Tokenizes the base text and creates a "fill-in-the-blank" prompt
        for each segment.
        """
        items_to_process = []
        word_regex = re.compile(r'\S+') # Simple regex to find words

        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), None)
                if not base_tier: continue
                
                for seg in base_tier.get("segments", []):
                    base_text = seg.get("text", "")
                    words = word_regex.findall(base_text)
                    if not words: continue

                    # Create the "fill-in-the-blank" list
                    prompt_text = "\n".join(f"{word} ->" for word in words)
                    
                    items_to_process.append({
                        "id": f"{block['s_id']}_{seg['seg_id']}",
                        "text": prompt_text,
                        "source_words": words # Store the source words for processing later
                    })
        return items_to_process

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """
        Parses the LLM's "fill-in-the-blank" response and populates the
        diglot map, auto-correcting any multi-word responses.
        """
        mappings = block.setdefault("mappings", {})
        diglot_map = mappings.setdefault("simple_target_to_base_diglot", {})
        
        base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), {})
        
        for seg in base_tier.get("segments", []):
            seg_id = seg["seg_id"]
            lookup_key = f"{block['s_id']}_{seg_id}"
            
            if lookup_key in llm_results:
                response_text = llm_results[lookup_key]
                response_map = {}
                # Parse the "word -> translation" lines from the response
                for line in response_text.splitlines():
                    if '->' in line:
                        parts = line.split('->', 1)
                        if len(parts) == 2:
                            response_map[parts[0].strip()] = parts[1].strip()

                seg_entries = []
                word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok.get("t") == "w"]

                for token in word_tokens:
                    base_word = token.get("v")
                    target_form = response_map.get(base_word, "NO_SUB") # Default to NO_SUB if missing

                    # --- AUTO-CORRECTION LOGIC ---
                    # If LLM returned a multi-word phrase, force it to NO_SUB.
                    if ' ' in target_form:
                        target_form = "NO_SUB"
                    # --- END AUTO-CORRECTION ---

                    is_viable = target_form.upper() not in ["PROPER_NOUN", "NO_SUB"]
                    
                    seg_entries.append([token["di"], "TBD", target_form, is_viable])

                diglot_map[seg_id] = seg_entries
        
        block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block