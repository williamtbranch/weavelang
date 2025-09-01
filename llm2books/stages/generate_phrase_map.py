import re
import json
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts, llm_utils
# --- THIS IS THE KEY ---
# We now import the parser/aligner into the generation stage itself.
from ..phrase_mapper_helpers import align_and_parse_to_atoms

class GeneratePhraseMap(LLMStage):
    """
    Stage 5: Generates a phrase-based mapping and IMMEDIATELY validates it
    against the source tokens before saving. This prevents pipeline pollution
    from malformed LLM responses.
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
                
                # We now prepare the ground-truth tokens here for the validator.
                word_tokens_for_validation = [
                    token for seg in base_tier.get("segments", [])
                    for token in seg.get("tokenized_text", [])
                    if token.get("t") == "w"
                ]
                
                if not word_tokens_for_validation: continue
                
                items_to_process.append({
                    "id": block['s_id'],
                    "text": prompt_text,
                    "source_tokens_for_validation": word_tokens_for_validation
                })
        return items_to_process

    def run(self) -> bool:
        """
        A new, self-validating run method. It calls the LLM and then
        immediately runs the robust parser to validate the response.
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
            return self._save_output_data(input_data, "COMPLETED")

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
            
            final_map = {item['id']: item['llm_response'] for item in llm_results_list}

            # --- VALIDATION AT THE SOURCE HAPPENS HERE ---
            for item in batch_items:
                s_id = item['id']
                raw_mapping_str = final_map.get(s_id, "")
                raw_map_lines = raw_mapping_str.splitlines()
                
                try:
                    logger.debug(f"S_ID {s_id}: Running pre-save validation dry run...")
                    # This is the validation "dry run". If it fails, it will raise a ValueError.
                    align_and_parse_to_atoms(raw_map_lines, item['source_tokens_for_validation'])
                    logger.debug(f"S_ID {s_id}: Validation PASSED.")
                except ValueError as e:
                    logger.error(f"CRITICAL VALIDATION FAILURE for S_ID {s_id} in Stage {self.stage_name}.")
                    logger.error(f"LLM response is not alignable with source tokens. Reason: {e}")
                    logger.error("The pipeline will now HALT to prevent saving corrupt data. Please check the LLM log for this stage to debug the prompt/response.")
                    return False # Halt the entire pipeline

                # If validation passes, we can safely update the block
                block_to_update = next((b for b in input_data['content_blocks'] if b.get('s_id') == s_id), None)
                if block_to_update:
                    self.process_llm_results_for_block(block_to_update, {s_id: raw_mapping_str})
            # --- END OF VALIDATION BLOCK ---

            if not self._save_output_data(input_data, "PARTIAL"):
                logger.error("CRITICAL: Failed to save progress. Halting.")
                return False
            logger.info(f"      -> Successfully validated and saved batch ending with {batch_items[-1]['id']}.")
        
        logger.info("      -> All items for this stage have been processed and validated.")
        return True

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """This function now simply stores the validated raw LLM output."""
        s_id = block['s_id']
        if s_id in llm_results:
            # We store the raw lines, confident they are valid.
            block.setdefault("mappings", {})["raw_phrase_map"] = llm_results[s_id].splitlines()
            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block