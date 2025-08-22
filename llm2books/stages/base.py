import logging
import json
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Dict, Any, List, Optional
logger = logging.getLogger("pipeline")

class Stage(ABC):
    """
    Abstract Base Class for a single stage in the WeaveLang processing pipeline.
    """
    def __init__(
        self,
        book_stem: str,
        cli_args: Any,
        common_resources: Dict[str, Any],
        stage_number: int,
        stage_name: str,
    ):
        self.book_stem = book_stem
        self.cli_args = cli_args
        self.resources = common_resources
        
        self.pipeline_config = self.resources.get("pipeline_config", {})
        self.models_config = self.resources.get("models_config", {})
        self.stages_config = self.resources.get("stages_config", {})
        
        self.stage_config = self.stages_config.get(self.__class__.__name__, {})
        
        self.stage_number = stage_number
        self.stage_name = stage_name
        self.is_part_a: bool = False
        self._initialize_paths()

    def _initialize_paths(self):
        content_project_dir_str = self.resources.get("content_project_dir")
        if not content_project_dir_str:
            raise ValueError("'content_project_dir' not found in common_resources.")

        self.content_project_root = Path(content_project_dir_str)
        lang_config = self.resources.get("language_config", {})
        base_code = lang_config.get("base_code", "unknown")
        target_code = lang_config.get("target_code", "unknown")
        lang_pair_dir = f"{base_code}-{target_code}"
        
        self.pipeline_run_dir = self.content_project_root / "pipeline_runs" / lang_pair_dir / self.book_stem
        self.stage_output_dir = self.pipeline_run_dir / f"stage{self.stage_number}"
        self.output_path = self.stage_output_dir / f"{self.book_stem}.stage{self.stage_number}.json"

    @abstractmethod
    def run(self) -> bool:
        pass

    def _get_input_path(self) -> Path:
        prev_stage_num = self.stage_number - 1
        if self.stage_number == 1:
            return None 
        prev_stage_dir = self.pipeline_run_dir / f"stage{prev_stage_num}"
        return prev_stage_dir / f"{self.book_stem}.stage{prev_stage_num}.json"

    def _load_input_data(self) -> Optional[Dict[str, Any]]:
        input_path = self._get_input_path()
        if not input_path:
            if self.stage_number == 1:
                return {}
            logger.critical(f"      -> CRITICAL: Input path could not be determined for stage {self.stage_name}.")
            return None

        if not input_path.exists():
            logger.critical(f"      -> CRITICAL: Input file not found at {input_path}")
            return None
        try:
            with open(input_path, "r", encoding="utf-8") as f:
                return json.load(f)
        except (IOError, json.JSONDecodeError) as e:
            logger.critical(f"      -> CRITICAL: Could not read or parse input file {input_path.name}: {e}")
            return None

    def _save_output_data(self, data: Dict[str, Any], status: str) -> bool:
        # This function should only save the data, not modify the status field itself,
        # as the status is now managed within the blocks.
        try:
            with open(self.output_path, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2, ensure_ascii=False)
            return True
        except IOError as e:
            logger.error(
                f"      -> CRITICAL: Could not write output to {self.output_path.name}: {e}"
            )
            return False

    def __str__(self) -> str:
        return f"Stage(#{self.stage_number} - {self.stage_name} for book '{self.book_stem}')"


class SpaCyStage(Stage, ABC):
    def __init__(
        self,
        book_stem: str,
        cli_args: Any,
        common_resources: Dict[str, Any],
        stage_number: int,
        stage_name: str,
    ):
        super().__init__(book_stem, cli_args, common_resources, stage_number, stage_name)

    def run(self) -> bool:
        logger.info(f"Executing SpaCy Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        # Check for our own output file first for a quick skip
        if self.output_path.exists():
            try:
                with open(self.output_path, 'r', encoding='utf-8') as f:
                    # Quick check to see if the first block is already done for this stage
                    data = json.load(f)
                    first_block = next((b for b in data.get("content_blocks", [])), None)
                    if first_block and first_block.get("processing_status", {}).get(self.stage_name) == "COMPLETED":
                        logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
                        return True
            except (IOError, json.JSONDecodeError):
                pass

        input_data = self._load_input_data()
        if input_data is None:
            return False

        logger.info("      -> Processing data with SpaCy model...")
        from .. import validator

        try:
            output_data = self._process_data(input_data)
            if output_data is None: # Explicit failure from process_data
                 return False
        except validator.ValidationError as e:
            logger.error(f"      -> CRITICAL: Data validation failed during stage {self.stage_name}.")
            logger.error(f"         Reason: {e}")
            return False
        except Exception as e:
            logger.error(f"      -> An unexpected error occurred during the _process_data step for stage {self.stage_name}: {e}")
            logger.exception("Traceback:") # Added for more detail
            return False

        if self._save_output_data(output_data, "COMPLETED"): # Status is for file metadata, not block data
            logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
            return True
        else:
            return False

class LLMStage(Stage, ABC):
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any], stage_number: int, stage_name: str):
        super().__init__(book_stem, cli_args, common_resources, stage_number, stage_name)
        self.llm_logger_dir = self.pipeline_run_dir / "llm_logs"
        self.parser_type = "single_line"
    
    @abstractmethod
    def get_system_prompt(self) -> str: pass

    @abstractmethod
    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        """Prepares the list of items to be sent to the LLM."""
        pass

    @abstractmethod
    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        """Processes LLM results for a single content_block and returns the updated block."""
        pass

    def run(self) -> bool:
        from ..llm_logger import LLMLogger
        from .. import llm_utils

        logger.info(f"Executing LLM Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        self.llm_logger_dir.mkdir(parents=True, exist_ok=True)

        input_data = self._load_input_data()
        if input_data is None: return False

        if self.output_path.exists():
            try:
                with open(self.output_path, 'r', encoding='utf-8') as f:
                    logger.info(f"      -> Resuming from existing output file: {self.output_path.name}")
                    input_data = json.load(f)
            except (IOError, json.JSONDecodeError):
                logger.warning(f"      -> Could not parse existing output file. Re-running.")

        all_possible_items = self.prepare_llm_items(input_data)
        completed_ids = {
            item['id'] for item in all_possible_items 
            if self._is_item_complete(input_data, item['id'])
        }
        
        items_for_this_run = [item for item in all_possible_items if item['id'] not in completed_ids]

        if not items_for_this_run:
            logger.info("      -> All items for this stage are already complete.")
            # Ensure the top-level status is also marked complete if we're skipping
            return self._save_output_data(input_data, "COMPLETED")

        logger.info(f"      -> Processing {len(items_for_this_run)} new items for the LLM.")
        llm_logger = LLMLogger(self.llm_logger_dir)
        system_prompt = self.get_system_prompt()

        #
        items_by_sid = {}
        for item in items_for_this_run:
            s_id = item['id'].split('_')[0]
            items_by_sid.setdefault(s_id, []).append(item)

        total_sids = len(items_by_sid)
        processed_sids = 0
        for s_id, items_for_sid in items_by_sid.items():
            processed_sids += 1
            logger.info(f"      -> Processing sentence {s_id} ({processed_sids}/{total_sids})...")

            llm_results = llm_utils.run_llm_batch_job(
                llm_client=self.resources['llm_client'],
                job_name=self.stage_name,
                system_prompt=system_prompt,
                items_to_process=items_for_sid,
                llm_logger=llm_logger,
                parser_type=self.parser_type,
                # --- THIS IS THE FIX ---
                stage_config=self.stage_config,
                models_config=self.models_config
            )
            #

            if llm_results is None:
                logger.error(f"LLM batch failed for sentence {s_id}. Halting stage.")
                return False

            block_to_update = next((b for b in input_data['content_blocks'] if b.get('s_id') == s_id), None)
            if block_to_update:
                response_map = {item['id']: item['llm_response'] for item in llm_results}
                updated_block = self.process_llm_results_for_block(block_to_update, response_map)
                
                for i, block in enumerate(input_data['content_blocks']):
                    if block.get('s_id') == s_id:
                        input_data['content_blocks'][i] = updated_block
                        break
                
                if not self._save_output_data(input_data, "PARTIAL"):
                    logger.error(f"Failed to save progress after processing {s_id}. Halting.")
                    return False
        
        logger.info("      -> All items for this stage have been processed.")
        return self._save_output_data(input_data, "COMPLETED")

    def _is_item_complete(self, book_data: Dict, item_id: str) -> bool:
        s_id = item_id.split('_')[0]
        block = next((b for b in book_data.get("content_blocks", []) if b.get("s_id") == s_id), None)
        if not block:
            return False
        return block.get("processing_status", {}).get(self.stage_name) == "COMPLETED"