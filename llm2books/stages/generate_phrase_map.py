# llm2books/stages/generate_phrase_map.py
from typing import Any, Dict, List

from .base import LLMStage, logger
from .. import llm_prompts, llm_utils
from ..phrase_mapper_helpers import refactor_token_stream
from pathlib import Path

HUMAN_REVIEW_DIR_NAME = "human_review"
HUMAN_REVIEW_MARKER = "%%HUMAN_REVIEW_APPROVED%%"

def check_approval_status(file_path: "Path") -> bool:
    if not file_path.is_file():
        return False
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            first_line = f.readline().strip()
        return first_line == HUMAN_REVIEW_MARKER
    except Exception:
        return False

class GeneratePhraseMap(LLMStage):
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=4,
            stage_name="GeneratePhraseMap"
        )
        self.human_review_dir = self.pipeline_run_dir / HUMAN_REVIEW_DIR_NAME
        # --- THIS IS THE FIX ---
        # The primary output of the stage is the .json file.
        # self.output_path is already correctly defined in the base class.
        # We add a new attribute for the review file.
        self.review_file_path = self.human_review_dir / f"{self.book_stem}.dig.txt"
        self.parser_type = "multi_line"

    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("generate_diglot_map", self.resources["language_config"])

    def _validate_llm_groups(self, parsed_response: Dict[str, str], batch_items: List[Dict]):
        """
        Gate 1 Validator: Ensures the LLM's grouping can be losslessly
        reconstructed from the original token stream before writing to disk.
        """
        for item in batch_items:
            s_id = item['id']
            raw_mapping_str = parsed_response.get(s_id, "")
            
            # Parse the LLM's proposed groups from the response
            llm_groups = []
            for line in raw_mapping_str.splitlines():
                if '->' in line:
                    parts = line.split('->', 1)
                    if len(parts) == 2 and parts[0].strip():
                        llm_groups.append(parts[0].strip())

            logger.debug(f"S_ID {s_id}: Running structural validation for forward map...")
            # This will raise ValidationError if the groups are invalid,
            # which is caught by run_llm_batch_job to trigger a retry.
            refactor_token_stream(item['original_tokens_for_validation'], llm_groups)
            logger.debug(f"S_ID {s_id}: Structural validation PASSED.")

    def _validate_llm_groups(self, parsed_response: Dict[str, str], batch_items: List[Dict]):
        # ... (implementation unchanged) ...
        for item in batch_items:
            s_id = item['id']
            raw_mapping_str = parsed_response.get(s_id, "")
            llm_groups = []
            for line in raw_mapping_str.splitlines():
                if '->' in line:
                    parts = line.split('->', 1)
                    if len(parts) == 2 and parts[0].strip():
                        llm_groups.append(parts[0].strip())
            logger.debug(f"S_ID {s_id}: Running structural validation for forward map...")
            refactor_token_stream(item['original_tokens_for_validation'], llm_groups)
            logger.debug(f"S_ID {s_id}: Structural validation PASSED.")

    #
    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        """Prepares sentences from the `basic_base` tier for the LLM."""
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                # The source for this mapping is the new basic_base tier
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "basic_base"), None)
                if not base_tier:
                    logger.debug(f"S_ID {block['s_id']}: Could not find 'basic_base' tier in prepare_llm_items for GeneratePhraseMap.")
                    continue
                
                prompt_text = " ".join(base_tier.get("full_text", "").strip().split())
                
                original_tokens = [
                    token for seg in base_tier.get("segments", [])
                    for token in seg.get("tokenized_text", [])
                ]
                
                if original_tokens and prompt_text:
                    items_to_process.append({
                        "id": block['s_id'],
                        "text": prompt_text,
                        "original_tokens_for_validation": original_tokens
                    })
        return items_to_process

     
    def run(self) -> bool:
        from ..llm_logger import LLMLogger
        from .. import llm_utils
        from .. import llm_overrides

        logger.info(f"Executing LLM Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        self.llm_logger_dir.mkdir(parents=True, exist_ok=True)
        llm_logger = LLMLogger(self.llm_logger_dir)
        #
        if self.review_file_path.exists() and check_approval_status(self.review_file_path):
            logger.info(f"      -> Found approved review file '{self.review_file_path.name}'. Skipping LLM generation.")
            # We still need to pass the data through, so we'll just load the input and save it as the output.
            input_data = self._load_input_data()
            if not input_data: return False
            if not self._save_output_data(input_data, "SKIPPED_HAS_APPROVED_FILE"): return False
            return True
        
        system_prompt = self.get_system_prompt()
        input_data = self._load_input_data()
        if input_data is None: return False

        if self.output_path.exists():
            try:
                with open(self.output_path, 'r', encoding='utf-8') as f:
                    logger.info(f"      -> Resuming from existing output file: {self.output_path.name}")
                    input_data = __import__('json').load(f)
            except (IOError, __import__('json').JSONDecodeError):
                logger.warning(f"      -> Could not parse existing output file. Re-running.")
        
        manual_overrides = llm_overrides.load_manual_overrides(self.stage_name, llm_logger)
        if manual_overrides:
            overrides_by_sid = {}
            for item_id, response_text in manual_overrides.items():
                s_id = item_id.split('_')[0]
                overrides_by_sid.setdefault(s_id, {})[item_id] = response_text

            for s_id, llm_results_for_sid in overrides_by_sid.items():
                block_to_update = next((b for b in input_data['content_blocks'] if b.get('s_id') == s_id), None)
                if block_to_update:
                    updated_block = self.process_llm_results_for_block(block_to_update, llm_results_for_sid)
                    for block_idx, block in enumerate(input_data['content_blocks']):
                        if block.get('s_id') == s_id:
                            input_data['content_blocks'][block_idx] = updated_block
                            break
            
            if not self._save_output_data(input_data, "PARTIAL"):
                logger.error("Failed to save progress after applying manual overrides. Halting.")
                return False
            logger.info(f"      -> Applied and saved {len(manual_overrides)} manual fix(es).")

        all_possible_items = self.prepare_llm_items(input_data)
        
        items_for_this_run = [
            item for item in all_possible_items 
            if not self._is_item_complete(input_data, item['id'])
        ]

        if not items_for_this_run:
            logger.info("      -> All items for this stage are already complete or manually fixed.")
            if not self._save_output_data(input_data, "COMPLETED"): return False

            # --- ADDED: Ensure review file is created even if no LLM call is needed ---
            logger.info(f"      -> Assembling human review file from existing data: {self.review_file_path.name}")
            output_data = self._load_current_stage_output() or input_data
            try:
                self.human_review_dir.mkdir(parents=True, exist_ok=True)
                with open(self.review_file_path, 'w', encoding='utf-8') as f:
                    f.write(f"# {HUMAN_REVIEW_MARKER}\n")
                    f.write(f"# File: {self.review_file_path.name}\n# Instructions: Edit phrase mappings. Left side MUST perfectly match words from the original.\n\n")
                    for block in output_data.get("content_blocks", []):
                        if block.get("block_type") == "sentence":
                            s_id = block['s_id']
                            raw_map_lines = block.get("mappings", {}).get("raw_phrase_map", [])
                            if raw_map_lines:
                                f.write(f"{s_id}:\n")
                                f.write("MAPPINGS:\n")
                                for line in raw_map_lines: f.write(f"{line}\n")
                                f.write("\n")
            except IOError as e:
                logger.error(f"      -> CRITICAL: Failed to write human review file from existing data: {e}")
                return False
            # --- END ADDED SECTION ---

            return True

        logger.info(f"      -> Processing {len(items_for_this_run)} new items for the LLM.")
        
        items_by_sid = {}
        for item in items_for_this_run:
            s_id = item['id'].split('_')[0]
            items_by_sid.setdefault(s_id, []).append(item)

        batch_size = self.stage_config.get("batch_size_in_items", 10)
        
        sentence_batches = []
        current_batch = []
        sorted_sids = sorted(items_by_sid.keys(), key=lambda x: int(x[1:]))

        for s_id in sorted_sids:
            items_for_sentence = items_by_sid[s_id]
            if current_batch and (len(current_batch) + len(items_for_sentence) > batch_size):
                sentence_batches.append(current_batch)
                current_batch = []
            current_batch.extend(items_for_sentence)
        if current_batch:
            sentence_batches.append(current_batch)

        system_prompt = self.get_system_prompt()
        
        total_batches = len(sentence_batches)
        for i, batch_items in enumerate(sentence_batches):
            logger.info(f"      -> Processing batch {i + 1}/{total_batches}...")

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
                    updated_block = self.process_llm_results_for_block(block_to_update, llm_results_for_sid)
                    for block_idx, block in enumerate(input_data['content_blocks']):
                        if block.get('s_id') == s_id:
                            input_data['content_blocks'][block_idx] = updated_block
                            break
            
            if not self._save_output_data(input_data, "PARTIAL"):
                logger.error("Failed to save progress after processing batch. Halting.")
                return False
        
        logger.info("      -> All items for this stage have been processed.")
        if not self._save_output_data(input_data, "COMPLETED"):
            return False

        # --- Create the review file from the final, complete JSON data ---
        logger.info(f"      -> Assembling human review file: {self.review_file_path.name}")
        output_data = self._load_current_stage_output()
        if not output_data:
            logger.error("      -> Could not reload data to write final review file.")
            return False
        try:
            self.human_review_dir.mkdir(parents=True, exist_ok=True)
            with open(self.review_file_path, 'w', encoding='utf-8') as f:
                f.write(f"# {HUMAN_REVIEW_MARKER}\n")
                f.write(f"# File: {self.review_file_path.name}\n# Instructions: Edit phrase mappings. Left side MUST perfectly match words from the original.\n\n")
                for block in output_data.get("content_blocks", []):
                    if block.get("block_type") == "sentence":
                        s_id = block['s_id']
                        raw_map_lines = block.get("mappings", {}).get("raw_phrase_map", [])
                        if raw_map_lines:
                            f.write(f"{s_id}:\n")
                            f.write("MAPPINGS:\n")
                            for line in raw_map_lines: f.write(f"{line}\n")
                            f.write("\n")
        except IOError as e:
            logger.error(f"      -> CRITICAL: Failed to write human review file: {e}")
            return False

        logger.info(f"      -> Successfully created review file.")
        return True

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        # ... (implementation unchanged) ...
        s_id = block['s_id']
        if s_id in llm_results:
            block.setdefault("mappings", {})["raw_phrase_map"] = llm_results[s_id].splitlines()
            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block

    def _load_current_stage_output(self):
        """Helper to load the JSON file this stage just created."""
        try:
            with open(self.output_path, 'r', encoding='utf-8') as f:
                return __import__('json').load(f)
        except (IOError, __import__('json').JSONDecodeError):
            return None