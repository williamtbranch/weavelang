# llm2books/stages/base.py

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
        
        # All stages get access to the full config dictionaries
        self.pipeline_config = self.resources.get("pipeline_config", {})
        self.models_config = self.resources.get("models_config", {})
        self.stages_config = self.resources.get("stages_config", {})
        
        # Each stage will look up its own specific config.
        # This will be overridden in child classes like LLMStage.
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

    def _is_stage_complete(self) -> bool:
        if not hasattr(self, 'output_path'):
             self._initialize_paths()

        if not self.output_path.exists():
            return False
        try:
            with open(self.output_path, "r", encoding="utf-8") as f:
                data = json.load(f)
            # A 'part a' stage is complete if it's marked as such
            if self.is_part_a and data.get("processing_status", "").startswith(
                f"PARTIAL_{self.stage_number}A"
            ):
                return True
            # Any other stage is complete only if marked fully COMPLETED
            return data.get("processing_status") == "COMPLETED"
        except (json.JSONDecodeError, IOError):
            return False

    #
    def _save_output_data(self, data: Dict[str, Any], status: str) -> bool:
        data["processing_status"] = status  # <--- DELETE THIS LINE
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

    #
    def run(self) -> bool:
        logger.info(f"Executing SpaCy Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        # Check for our own output file first for a quick skip
        if self.output_path.exists():
            try:
                with open(self.output_path, 'r', encoding='utf-8') as f:
                    data = json.load(f)
                if data.get("processing_status") == "COMPLETED":
                    logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
                    return True
            except (IOError, json.JSONDecodeError):
                pass # File is corrupt or unreadable, so we'll overwrite it.

        # Now, load the actual input we need to process
        input_data = self._load_input_data()
        if input_data is None:
            # _load_input_data already logged the critical error
            return False

        logger.info("      -> Processing data with SpaCy model...")
        from .. import validator # Local import to get the exception type

        try:
            output_data = self._process_data(input_data)
        except validator.ValidationError as e:
            # Catch the specific data integrity error
            logger.error(f"      -> CRITICAL: Data validation failed during stage {self.stage_name}.")
            logger.error(f"         Reason: {e}")
            return False
        except Exception as e:
            # Catch any other unexpected errors during processing
            logger.error(f"      -> An unexpected error occurred during the _process_data step for stage {self.stage_name}: {e}")
            return False
        # --- END OF FIX ---

        final_status = "COMPLETED"
        if self.is_part_a:
            final_status = f"PARTIAL_{self.stage_number}A_COMPLETE"

        if self._save_output_data(output_data, final_status):
            logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
            return True
        else:
            return False

        

    def _get_input_path(self) -> Path:
        prev_stage_num = self.stage_number - 1
        
        # Stage 1 is special, it has no preceding stage file
        if self.stage_number == 1:
            return None 

        prev_stage_dir = self.pipeline_run_dir / f"stage{prev_stage_num}"
        return prev_stage_dir / f"{self.book_stem}.stage{prev_stage_num}.json"

    def _load_input_data(self) -> Optional[Dict[str, Any]]:
        input_path = self._get_input_path()
        if not input_path:
            # This is expected for Stage 1, which will handle its own input.
            if self.stage_number == 1:
                return {} # Return an empty dict to proceed
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

    def _load_input_data(self) -> Optional[Dict[str, Any]]:
        input_path = self._get_input_path()
        if not input_path.exists():
            logger.critical(f"      -> CRITICAL: Input file not found at {input_path}")
            return None
        try:
            with open(input_path, "r", encoding="utf-8") as f:
                return json.load(f)
        except (IOError, json.JSONDecodeError) as e:
            logger.critical(f"      -> CRITICAL: Could not read or parse input file {input_path.name}: {e}")
            return None

#
class LLMStage(Stage, ABC):
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any], stage_number: int, stage_name: str):
        super().__init__(book_stem, cli_args, common_resources, stage_number, stage_name)
        self.llm_logger_dir = self.pipeline_run_dir / "llm_logs"
        self.parser_type = "single_line" # Default, can be overridden by child class
    
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

    #
    def run(self) -> bool:
        from ..llm_logger import LLMLogger
        from .. import llm_utils

        logger.info(f"Executing LLM Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        self.llm_logger_dir.mkdir(parents=True, exist_ok=True)

        input_path = self._get_input_path()
        try:
            with open(input_path, 'r', encoding='utf-8') as f:
                book_data = json.load(f)
        except Exception as e:
            logger.error(f"Could not read or parse input file {input_path.name}: {e}")
            return False

        # If a partial file for this stage already exists, use it instead.
        if self.output_path.exists():
            try:
                with open(self.output_path, 'r', encoding='utf-8') as f:
                    logger.info(f"      -> Resuming from existing output file: {self.output_path.name}")
                    book_data = json.load(f)
            except  (IOError, json.JSONDecodeError):
                # If the file is corrupt, we log a warning and proceed as if it didn't exist,
                # using the data from the previous stage. The corrupt file will be overwritten.
                logger.warning(f"      -> Could not parse existing output file {self.output_path.name}. Re-running stage from scratch.")



        # Prepare all possible items and then filter out the completed ones
        all_possible_items = self.prepare_llm_items(book_data)
        completed_ids = {
            item['id'] for item in all_possible_items 
            if self._is_item_complete(book_data, item['id'])
        }
        
        items_for_this_run = [item for item in all_possible_items if item['id'] not in completed_ids]

        if not items_for_this_run:
            logger.info("      -> All items for this stage are already complete.")
            return self._save_output_data(book_data, "COMPLETED")

        logger.info(f"      -> Processing {len(items_for_this_run)} new items for the LLM.")
        llm_logger = LLMLogger(self.llm_logger_dir)
        system_prompt = self.get_system_prompt()

        # Group items by s_id to process one sentence at a time
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
                parser_type=self.parser_type
            )

            if llm_results is None:
                logger.error(f"LLM batch failed for sentence {s_id}. Halting stage.")
                return False

            # Find the corresponding block in book_data
            block_to_update = next((b for b in book_data['content_blocks'] if b.get('s_id') == s_id), None)
            if block_to_update:
                response_map = {item['id']: item['llm_response'] for item in llm_results}
                updated_block = self.process_llm_results_for_block(block_to_update, response_map)
                
                # Replace the old block with the updated one
                for i, block in enumerate(book_data['content_blocks']):
                    if block.get('s_id') == s_id:
                        book_data['content_blocks'][i] = updated_block
                        break
                
                # Commit this sentence's progress to disk
                if not self._save_output_data(book_data, "PARTIAL"):
                    logger.error(f"Failed to save progress after processing {s_id}. Halting.")
                    return False
        
        logger.info("      -> All items for this stage have been processed.")
        return self._save_output_data(book_data, "COMPLETED")

    def _is_item_complete(self, book_data: Dict, item_id: str) -> bool:
        """Checks if a specific item's work is already done in the book_data."""
        s_id = item_id.split('_')[0]
        block = next((b for b in book_data.get("content_blocks", []) if b.get("s_id") == s_id), None)
        if not block:
            return False
        return block.get("processing_status", {}).get(self.stage_name) == "COMPLETED"

    def _get_input_path(self) -> Path:
        prev_stage_num = self.stage_number - 1
        lang_pair_dir = f"{self.resources['language_config']['base_code']}-{self.resources['language_config']['target_code']}"
        return self.content_project_root / "pipeline_runs" / lang_pair_dir / self.book_stem / f"stage{prev_stage_num}" / f"{self.book_stem}.stage{prev_stage_num}.json"