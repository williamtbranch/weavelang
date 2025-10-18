# In llm2books/stages/generate_inverse_diglot_map.py

import re
import json
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts, llm_utils
from ..phrase_mapper_helpers import refactor_token_stream
from ..validator import ValidationError

class GenerateInverseDiglotMap(LLMStage):
    """
    Stage 5: Generates and VALIDATES an "inverse diglot map" from the simple_target
    tier back to the base language, correctly operating on a PER-SEGMENT basis.
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

    def _validate_llm_groups(self, parsed_response: Dict[str, str], batch_items: List[Dict]):
        """Callback to perform structural validation inside the retry loop."""
        for item in batch_items:
            item_id = item['id'] # e.g., "S19_S1"
            raw_mapping_str = parsed_response.get(item_id, "")
            
            llm_groups = []
            for line in raw_mapping_str.splitlines():
                if '->' in line:
                    parts = line.split('->', 1)
                    if len(parts) == 2 and parts[0].strip():
                        llm_groups.append(parts[0].strip())
            
            logger.debug(f"ID {item_id}: Running pre-save structural validation for inverse map...")
            # This will raise ValidationError on failure, triggering a retry in the llm_utils.
            refactor_token_stream(item['original_tokens_for_validation'], llm_groups)
            logger.debug(f"ID {item_id}: Structural validation PASSED.")

    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("generate_inverse_phrase_map", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                s_id = block['s_id']
                source_tier = next((t for t in block["tiers"] if t["tier_id"] == "simple_target"), None)
                if not source_tier: continue

                for seg in source_tier.get("segments", []):
                    seg_id = seg["seg_id"]
                    prompt_text = " ".join(seg.get("text", "").strip().split())
                    
                    if re.search(r'[a-zA-Z]', prompt_text):
                        items_to_process.append({
                            "id": f"{s_id}_{seg_id}", 
                            "text": prompt_text,
                            "original_tokens_for_validation": seg.get("tokenized_text", [])
                        })
        return items_to_process

    def run(self) -> bool:
        """Custom run method to perform validation within the retry loop."""
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
        items_for_this_run = [item for item in all_items if not self._is_item_complete(input_data, item['id'])]
        
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
                llm_clients=self.resources['llm_clients'], job_name=self.stage_name,
                system_prompt=system_prompt, items_to_process=batch_items,
                llm_logger=llm_logger, parser_type=self.parser_type,
                stage_config=self.stage_config, models_config=self.models_config,
                pipeline_config=self.pipeline_config,
                post_process_validator=self._validate_llm_groups
            )
            
            if llm_results_list is None: return False
            
            results_by_sid = {}
            for item in llm_results_list:
                s_id = item['id'].split('_')[0]
                results_by_sid.setdefault(s_id, {})[item['id']] = item['llm_response']

            for s_id, llm_results_for_sid in results_by_sid.items():
                block_to_update = next((b for b in input_data['content_blocks'] if b.get('s_id') == s_id), None)
                if block_to_update:
                    self.process_llm_results_for_block(block_to_update, llm_results_for_sid)

            if not self._save_output_data(input_data, "PARTIAL"):
                logger.error("CRITICAL: Failed to save progress. Halting.")
                return False
            logger.info(f"      -> Successfully validated and saved batch ending with {batch_items[-1]['id']}.")
        
        logger.info("      -> All items for this stage have been processed.")
        return self._save_output_data(input_data, "COMPLETED")

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """Stores the validated raw LLM output."""
        s_id = block['s_id']
        mappings = block.setdefault("mappings", {})
        map_key = "raw_simple_to_base_inv_diglot_map"
        raw_map_by_segment = mappings.setdefault(map_key, {})

        for item_id, response_text in llm_results.items():
            if item_id.startswith(s_id):
                seg_id = item_id.split('_', 1)[1]
                raw_map_by_segment[seg_id] = response_text.splitlines()

        block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block