import re
import json
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts, llm_utils
from ..phrase_mapper_helpers import refactor_token_stream
from ..validator import ValidationError

class GeneratePhraseMap(LLMStage):
    """
    Stage 3: Generates a phrase-based mapping and IMMEDIATELY validates its
    structural integrity against the source tokens before saving. This prevents
    pipeline pollution from malformed LLM responses.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=3,
            stage_name="GeneratePhraseMap"
        )
        self.parser_type = "multi_line"

    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("generate_diglot_map", self.resources["language_config"])

    def _validate_llm_groups(self, parsed_response: Dict[str, str], batch_items: List[Dict]):
        """Callback to perform structural validation inside the retry loop."""
        for item in batch_items:
            s_id = item['id']
            raw_mapping_str = parsed_response.get(s_id, "")
            
            llm_groups = []
            for line in raw_mapping_str.splitlines():
                if '->' in line:
                    parts = line.split('->', 1)
                    if len(parts) == 2 and parts[0].strip():
                        llm_groups.append(parts[0].strip())
            
            logger.debug(f"S_ID {s_id}: Running pre-save structural validation...")
            # This call will raise ValidationError on failure, which will be
            # caught by the `run_llm_batch_job`'s main try/except block.
            refactor_token_stream(item['original_tokens_for_validation'], llm_groups)
            logger.debug(f"S_ID {s_id}: Structural validation PASSED.")

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), None)
                if not base_tier: continue
                
                prompt_text = " ".join(base_tier.get("full_text", "").strip().split())
                
                # We need the original tokens for our validation dry run
                original_tokens_for_validation = [
                    token for seg in base_tier.get("segments", [])
                    for token in seg.get("tokenized_text", [])
                ]
                
                if not original_tokens_for_validation or not prompt_text:
                    continue
                
                items_to_process.append({
                    "id": block['s_id'],
                    "text": prompt_text,
                    # Attach the tokens needed for validation to the item
                    "original_tokens_for_validation": original_tokens_for_validation
                })
        return items_to_process

    #
    def run(self) -> bool:
        """
        A custom, self-validating run method. It calls the LLM and passes a
        callback to run the robust refactor function for validation inside
        the retry loop.
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
        
        items_for_this_run = [
            item for item in all_items if not self._is_item_complete(input_data, item['id'])
        ]
        
        if not items_for_this_run:
            logger.info("      -> All items for this stage are already complete.")
            return self._save_output_data(input_data, "COMPLETED")

        logger.info(f"      -> Processing {len(items_for_this_run)} new items for the LLM.")
        llm_logger = LLMLogger(self.llm_logger_dir)
        system_prompt = self.get_system_prompt()
        batch_size = self.stage_config.get("batch_size_in_items", 10)

        for i in range(0, len(items_for_this_run), batch_size):
            batch_items = items_for_this_run[i:i + batch_size]
            
            # --- MODIFIED: Pass the new validator method as a callback ---
            llm_results_list = llm_utils.run_llm_batch_job(
                llm_clients=self.resources['llm_clients'], 
                job_name=self.stage_name,
                system_prompt=system_prompt, 
                items_to_process=batch_items,
                llm_logger=llm_logger, 
                parser_type=self.parser_type,
                stage_config=self.stage_config, 
                models_config=self.models_config,
                pipeline_config=self.pipeline_config,
                post_process_validator=self._validate_llm_groups # <-- Pass the callback
            )
            
            if llm_results_list is None: 
                return False # This now means all retries have failed

            final_map = {item['id']: item['llm_response'] for item in llm_results_list}

            # At this point, the results are GUARANTEED to be structurally valid.
            for item in batch_items:
                s_id = item['id']
                raw_mapping_str = final_map.get(s_id, "")
                block_to_update = next((b for b in input_data['content_blocks'] if b.get('s_id') == s_id), None)
                if block_to_update:
                    self.process_llm_results_for_block(block_to_update, {s_id: raw_mapping_str})

            if not self._save_output_data(input_data, "PARTIAL"):
                logger.error("CRITICAL: Failed to save progress. Halting.")
                return False
            logger.info(f"      -> Successfully validated and saved batch ending with {batch_items[-1]['id']}.")
        
        logger.info("      -> All items for this stage have been processed and validated.")
        return self._save_output_data(input_data, "COMPLETED")

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """This function now simply stores the validated raw LLM output."""
        s_id = block['s_id']
        if s_id in llm_results:
            # We store the raw lines, confident they are structurally valid.
            block.setdefault("mappings", {})["raw_phrase_map"] = llm_results[s_id].splitlines()
            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block