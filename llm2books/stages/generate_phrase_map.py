# In llm2books/stages/generate_phrase_map.py

import re
import json
from typing import Any, Dict, List
import inspect 

from .base import LLMStage, logger
from .. import llm_prompts, llm_utils

def _normalize_for_matching(text: str) -> str:
    """Strips punctuation and extra whitespace, and lowercases for matching."""
    s = re.sub(r'[^\w\s]', '', text)
    return re.sub(r'\s+', ' ', s).strip().lower()

def _validate_llm_map(s_id: str, original_sentence: str, llm_map_lines: List[str]) -> bool:
    """
    Validates that the LLM's phrase map is exhaustive against actual words.
    Now takes a list of lines instead of a raw string.
    """
    # --- THIS IS THE FIX ---
    # We check if the list itself is empty, not try to strip it.
    if not llm_map_lines:
        logger.error(f"LLM map validation FAILED for S_ID {s_id}: No mapping lines were extracted from the response.")
        return False
    # --- END OF FIX ---

    original_words = _normalize_for_matching(original_sentence).split()
    
    llm_words = []
    for line in llm_map_lines:
        if '->' in line:
            parts = line.split('->', 1)
            if len(parts) == 2:
                en_phrase = parts[0].strip()
                llm_words.extend(_normalize_for_matching(en_phrase).split())

    if original_words != llm_words:
        logger.error(f"LLM map validation FAILED for S_ID {s_id}.")
        logger.error(f"  - Mismatch in word streams (punctuation excluded).")
        logger.error(f"  - Expected Words: {original_words}")
        logger.error(f"  - Mapped Words:   {llm_words}")
        return False
    
    return True


class GeneratePhraseMap(LLMStage):
    """
    New Stage 5: Generates a phrase-based mapping from the base language
    to the simple target language for each sentence, with strict validation.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=5,
            stage_name="GeneratePhraseMap"
        )
        self.parser_type = "multi_line"

    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("generate_diglot_map", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), None)
                if not base_tier: continue
                
                prompt_text = base_tier["full_text"]
                
                items_to_process.append({
                    "id": block['s_id'],
                    "text": prompt_text
                })
        return items_to_process
    def run(self) -> bool:
        """
        Final robust run method. The llm_utils module now handles all complex parsing.
        This method just validates the clean output.
        """
        from ..llm_logger import LLMLogger
        
        logger.info(f"Executing LLM Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        self.llm_logger_dir.mkdir(parents=True, exist_ok=True)

        input_data = self._load_input_data()
        if input_data is None: return False

        if self.output_path.exists():
            try:
                with open(self.output_path, 'r', encoding='utf-8') as f:
                    input_data = json.load(f)
            except Exception: pass

        all_items = self.prepare_llm_items(input_data)
        completed_ids = { item['id'] for item in all_items if self._is_item_complete(input_data, item['id']) }
        items_for_this_run = [item for item in all_items if item['id'] not in completed_ids]

        if not items_for_this_run:
            logger.info("      -> All items for this stage are already complete.")
            return True

        logger.info(f"      -> Processing {len(items_for_this_run)} new items for the LLM.")
        llm_logger = LLMLogger(self.llm_logger_dir)
        system_prompt = self.get_system_prompt()
        batch_size = self.stage_config.get("batch_size_in_items", 10)

        for i in range(0, len(items_for_this_run), batch_size):
            batch_items = items_for_this_run[i:i + batch_size]
            
            llm_results_list = llm_utils.run_llm_batch_job(
                llm_client=self.resources['llm_client'], job_name=self.stage_name,
                system_prompt=system_prompt, items_to_process=batch_items,
                llm_logger=llm_logger, parser_type=self.parser_type,
                stage_config=self.stage_config, models_config=self.models_config
            )

            if llm_results_list is None: return False
            
            # The utility now returns clean, validated data.
            final_map = {item['id']: item['llm_response'] for item in llm_results_list}

            for item in batch_items:
                s_id = item['id']
                clean_mapping_str = final_map.get(s_id, "")
                
                # We can now directly validate the clean mapping string
                if not _validate_llm_map(s_id, item['text'], clean_mapping_str.splitlines()):
                    logger.error(f"Validation failed for S_ID {s_id}. Halting. Check LLM log for '{self.stage_name}'.")
                    return False
                
                block_to_update = next((b for b in input_data['content_blocks'] if b.get('s_id') == s_id), None)
                if block_to_update:
                    self.process_llm_results_for_block(block_to_update, {s_id: clean_mapping_str})

            if not self._save_output_data(input_data, "PARTIAL"):
                logger.error("CRITICAL: Failed to save progress. Halting.")
                return False
            logger.info(f"      -> Successfully processed and saved batch ending with {batch_items[-1]['id']}.")
        
        logger.info("      -> All items for this stage have been processed and validated.")
        return True

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        s_id = block['s_id']
        if s_id in llm_results:
            validated_lines = [
                line for line in llm_results[s_id].splitlines()
                if not line.strip().upper().startswith("VALIDATION:")
            ]
            block.setdefault("mappings", {})["raw_phrase_map"] = validated_lines
            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block