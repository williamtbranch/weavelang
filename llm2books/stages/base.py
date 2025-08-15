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

    def _save_output_data(self, data: Dict[str, Any], status: str) -> bool:
        data["processing_status"] = status
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

        if self._is_stage_complete(): # We removed force_book, so this check is now simpler
            logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
            return True

        input_data = self._load_input_data()
        if input_data is None:
            return False

        logger.info("      -> Processing data with SpaCy model...")
        output_data = self._process_data(input_data)

        final_status = "COMPLETED"
        if self.is_part_a:
            final_status = f"PARTIAL_{self.stage_number}A_COMPLETE"

        if self._save_output_data(output_data, final_status):
            logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
            return True
        else:
            return False

    @abstractmethod
    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        pass

    #_get_input_path
    def _get_input_path(self) -> Path:
        prev_stage_num = self.stage_number - 1
        lang_pair_dir = f"{self.resources['language_config']['base_code']}-{self.resources['language_config']['target_code']}"
        return self.content_project_root / "pipeline_runs" / lang_pair_dir / self.book_stem / f"stage{prev_stage_num}" / f"{self.book_stem}.stage{prev_stage_num}.json"

    #_run_llm_batch_job
    def _run_llm_batch_job(self, job_name: str, system_prompt: str, items_to_process: List[Dict], llm_logger) -> Optional[List[Dict]]:
        # NOTE: Removed LLMLogger type hint to avoid top-level import
        BATCH_SIZE, MAX_RETRIES, RETRY_DELAY = 10, 3, 5
        all_results = []
        batch_num = 0
        llm_client = self.resources['llm_client']

        for i in range(0, len(items_to_process), BATCH_SIZE):
            batch_num += 1
            batch = items_to_process[i:i + BATCH_SIZE]
            
            prompt_ids = [item['id'] for item in batch]
            user_prompt = "\n".join([f"{item['id']}: {item['text']}" for item in batch])
            logger.info(f"    -> Running {job_name} LLM batch {batch_num}...")
            
            for _ in range(MAX_RETRIES):
                raw_response = ""
                try:
                    message = llm_client.messages.create(model="claude-3-haiku-20240307", system=system_prompt, messages=[{"role": "user", "content": user_prompt}], max_tokens=4096)
                    raw_response = message.content[0].text
                    llm_logger.log_batch(job_name, batch_num, system_prompt, user_prompt, raw_response)
                    
                    if self.parser_type == 'single_line':
                        parsed_response = self._parse_singleline_llm_response(raw_response)
                    else:
                        parsed_response = self._parse_multiline_llm_response(raw_response)
                    
                    if all(pid in parsed_response for pid in prompt_ids):
                        for item in batch:
                            item['llm_response'] = parsed_response[item['id']]
                        all_results.extend(batch)
                        break
                    else:
                        missing_ids = [pid for pid in prompt_ids if pid not in parsed_response]
                        logger.warning(f"      -> {job_name} batch failed validation (missing IDs: {missing_ids}). Retrying...")
                except Exception as e:
                    logger.error(f"      -> API Error during {job_name} batch: {e}. Retrying...")
                    if raw_response: llm_logger.log_batch(job_name, batch_num, system_prompt, user_prompt, f"FAILED_RESPONSE: {raw_response}\nERROR: {e}")
                time.sleep(RETRY_DELAY)
            else:
                logger.error(f"      -> {job_name} batch failed after {MAX_RETRIES} retries. Aborting job."); return None
        return all_results

    def _parse_singleline_llm_response(self, raw_text: str) -> Dict[str, str]:
        parsed = {}
        line_regex = re.compile(r"^([^:]+):\s*(.*)$")
        for line in raw_text.splitlines():
            match = line_regex.match(line)
            if match:
                parsed[match.group(1).strip()] = match.group(2).strip()
        return parsed

    def _parse_multiline_llm_response(self, raw_text: str) -> Dict[str, str]:
        """
        Parses a multi-line LLM response where content for an ID can span
        multiple lines.
        """
        parsed = {}
        current_id = None
        current_lines = []

        # Regex to find an ID at the start of a line. It captures the ID.
        # It's robust enough for S1, S1_S1, S1_A1, etc.
        id_regex = re.compile(r"^\s*([A-Za-z0-9_]+):")

        # Add a sentinel value (None) to the end to ensure the last block is processed
        for line in raw_text.strip().splitlines() + [None]:
            match = id_regex.match(line) if line is not None else None

            if match:
                # Found a new ID. Finalize the previous one if it exists.
                if current_id:
                    parsed[current_id] = "\n".join(current_lines).strip()

                # Start the new block
                current_id = match.group(1).strip()
                # Get the part of the line after the colon
                after_colon = line[match.end():].strip()
                current_lines = [after_colon] if after_colon else []
            elif current_id:
                # This is a continuation line for the current block
                if line is not None:
                    current_lines.append(line)
        
        return parsed

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
        lang_pair_dir = f"{self.resources['language_config']['base_code']}-{self.resources['language_config']['target_code']}"
        self.llm_logger_dir = self.content_project_root / "common_pool" / "llm_logs" / self.book_stem
        self.parser_type = "single_line"
    
    @abstractmethod
    def get_system_prompt(self) -> str: pass
    @abstractmethod
    def prepare_items_for_llm(self, book_data: Dict) -> List[Dict]: pass
    @abstractmethod
    def process_llm_responses(self, book_data: Dict, llm_responses: List[Dict]) -> Dict: pass
        
    def run(self) -> bool:
        # Import necessary components here to avoid top-level circular dependencies
        from ..llm_logger import LLMLogger
        from .. import llm_utils

        logger.info(f"Executing LLM Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        
        #
        # 1. Load the input data from the previous stage.
        input_path = self._get_input_path()
        try:
            with open(input_path, 'r', encoding='utf-8') as f:
                book_data = json.load(f)
        except Exception as e:
            logger.error(f"Could not read or parse input file {input_path.name}: {e}")
            return False

        # 2. Check for and load existing partial output from this stage.
        completed_s_ids = set()
        if self.output_path.exists():
            try:
                with open(self.output_path, 'r', encoding='utf-8') as f:
                    existing_output_data = json.load(f)
                
                # If the stage was already fully completed, we can skip it entirely.
                if existing_output_data.get("processing_status") == "COMPLETED":
                    logger.info(f"      -> Stage is already marked as 'COMPLETED'. Skipping.")
                    return True

                # Otherwise, find out which sentences were already done.
                for block in existing_output_data.get("content_blocks", []):
                    status = block.get("processing_status", {}).get(self.stage_name)
                    if status == "COMPLETED":
                        completed_s_ids.add(block.get("s_id"))
                
                if completed_s_ids:
                    logger.info(f"      -> Resuming stage. Found {len(completed_s_ids)} already completed sentences.")
                    # We will use the existing output as our starting point.
                    book_data = existing_output_data

            except (IOError, json.JSONDecodeError):
                logger.warning(f"      -> Could not parse existing output file {self.output_path.name}. Re-running stage from scratch.")
        # --- END Resumability Logic ---

        all_items_to_process = self.prepare_items_for_llm(book_data)
        
        # Filter out items that have already been completed
        items_for_this_run = [
            item for item in all_items_to_process 
            if item['id'].split('_')[0] not in completed_s_ids
        ]

        if not items_for_this_run:
            logger.info("      -> All required items have already been processed in a previous run.")
            # We still need to save to mark the whole stage as complete
            return self._save_output_data(book_data, "COMPLETED")

        logger.info(f"      -> Preparing to process {len(items_for_this_run)} new items for the LLM.")
        llm_logger = LLMLogger(self.llm_logger_dir)
        system_prompt = self.get_system_prompt()
        
        llm_results = llm_utils.run_llm_batch_job(
            llm_client=self.resources['llm_client'],
            job_name=self.stage_name,
            system_prompt=system_prompt,
            items_to_process=items_for_this_run,
            llm_logger=llm_logger,
            parser_type=self.parser_type
        )

        if llm_results is None:
            # An error occurred, but we should still save the progress we had.
            self._save_output_data(book_data, "PARTIAL")
            return False

        updated_book_data = self.process_llm_responses(book_data, llm_results)
        
        # Mark the entire stage as complete now that all items are processed
        return self._save_output_data(updated_book_data, "COMPLETED")

    def _get_input_path(self) -> Path:
        prev_stage_num = self.stage_number - 1
        lang_pair_dir = f"{self.resources['language_config']['base_code']}-{self.resources['language_config']['target_code']}"
        return self.content_project_root / "pipeline_runs" / lang_pair_dir / self.book_stem / f"stage{prev_stage_num}" / f"{self.book_stem}.stage{prev_stage_num}.json"