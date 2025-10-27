import logging
import json
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Dict, Any, List, Optional
import time
logger = logging.getLogger("pipeline")

# ... (Stage and SpaCyStage classes are unchanged) ...
class Stage(ABC):
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
        print(f"\n--- [DEBUG] Stage '{self.stage_name}' (Number {self.stage_number}) is SAVING to: {self.output_path}\n")
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

        if self.output_path.exists():
            try:
                with open(self.output_path, 'r', encoding='utf-8') as f:
                    data = json.load(f)
                    all_blocks_completed = True
                    sentence_blocks_found = False
                    for block in data.get("content_blocks", []):
                        if block.get("block_type") == "sentence":
                            sentence_blocks_found = True
                            if block.get("processing_status", {}).get(self.stage_name) != "COMPLETED":
                                all_blocks_completed = False
                                break
                    
                    if sentence_blocks_found and all_blocks_completed:
                        logger.info(f"      -> Stage is already marked as 'COMPLETED' for all blocks. Skipping.")
                        return True
            except (IOError, json.JSONDecodeError):
                logger.warning(f"      -> Could not parse existing output file for resumability check. Re-running stage.")
                pass
        
        input_data = self._load_input_data()
        if input_data is None:
            return False

        logger.info("      -> Processing data with SpaCy model...")
        from .. import validator

        try:
            output_data = self._process_data(input_data)
            if output_data is None:
                 return False
        except validator.ValidationError as e:
            logger.error(f"      -> CRITICAL: Data validation failed during stage {self.stage_name}.")
            logger.error(f"         Reason: {e}")
            return False
        except Exception as e:
            logger.error(f"      -> An unexpected error occurred during the _process_data step for stage {self.stage_name}: {e}")
            logger.exception("Traceback:")
            return False

        if self._save_output_data(output_data, "COMPLETED"):
            logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
            return True
        else:
            return False

class LLMStage(Stage, ABC):
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any], stage_number: int, stage_name: str):
        super().__init__(book_stem, cli_args, common_resources, stage_number, stage_name)
        # --- START DEBUGGING ---
        print(f"\n--- [DEBUG] Initializing LLMStage: {stage_name} ---")
        print(f"--- [DEBUG] self.stage_name = '{self.stage_name}'")
        print(f"--- [DEBUG] Available keys in self.stages_config: {list(self.stages_config.keys())}")
        # --- END DEBUGGING ---

        self.stage_config = self.stages_config.get(self.stage_name, {})
        
        # --- MORE DEBUGGING ---
        print(f"--- [DEBUG] Looked up config for '{self.stage_name}'. Found: {self.stage_config}\n")
        # --- END DEBUGGING ---

        self.llm_logger_dir = self.pipeline_run_dir / "llm_logs"
        self.parser_type = "single_line"
    
    @abstractmethod
    def get_system_prompt(self) -> str: pass

    @abstractmethod
    def prepare_llm_items(self, book_data: Dict) -> List[Dict]: pass

    @abstractmethod
    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict: pass

    def run(self) -> bool:
        from ..llm_logger import LLMLogger
        from .. import llm_utils
        from .. import llm_overrides
        import json # Add this import here for the resume logic

        logger.info(f"Executing LLM Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        self.llm_logger_dir.mkdir(parents=True, exist_ok=True)
        llm_logger = LLMLogger(self.llm_logger_dir)

        # --- START: ROBUST CACHING LOGIC ---
        system_prompt = self.get_system_prompt()
        cached_prompt = None
        cache_creation_time = 0
        CACHE_TTL_SECONDS = 55 * 60  # 55 minutes

        primary_model_key = self.stage_config.get("primary_model")
        primary_model_info = self.models_config.get(primary_model_key, {})
        primary_provider = primary_model_info.get("provider")

        def refresh_gemini_cache():
            nonlocal cached_prompt, cache_creation_time
            if primary_provider == "gemini":
                try:
                    model_name_for_cache = primary_model_info.get("name", "").split('/')[-1]
                    if model_name_for_cache:
                        logger.info(f"      -> Caching/Refreshing system prompt for Gemini model '{model_name_for_cache}'...")
                        cached_prompt = llm_utils.create_gemini_cache(
                            model_name=model_name_for_cache,
                            system_prompt=system_prompt
                        )
                        if cached_prompt:
                            cache_creation_time = time.time()
                            logger.info("      -> System prompt cached successfully.")
                        else:
                            cache_creation_time = 0 # Reset on failure
                except Exception as e:
                    logger.warning(f"      -> Could not create Gemini cache, proceeding without it. Error: {e}")
                    cached_prompt = None
                    cache_creation_time = 0
        
        # Initial cache creation
        refresh_gemini_cache()
        # --- END: ROBUST CACHING LOGIC ---

        input_data = self._load_input_data()
        if input_data is None: return False

        if self.output_path.exists():
            try:
                with open(self.output_path, 'r', encoding='utf-8') as f:
                    logger.info(f"      -> Resuming from existing output file: {self.output_path.name}")
                    input_data = json.load(f)
            except (IOError, json.JSONDecodeError):
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

        total_batches = len(sentence_batches)
        for i, batch_items in enumerate(sentence_batches):
            logger.info(f"      -> Processing batch {i + 1}/{total_batches}...")

            # Check and refresh cache before each batch
            if cached_prompt and (time.time() - cache_creation_time > CACHE_TTL_SECONDS):
                logger.info("      -> Gemini cache has expired. Refreshing...")
                refresh_gemini_cache()
            
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
                post_process_validator=self._validate_llm_groups if hasattr(self, '_validate_llm_groups') else None,
                cached_prompt=cached_prompt
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
        return self._save_output_data(input_data, "COMPLETED")

    def _is_item_complete(self, book_data: Dict, item_id: str) -> bool:
        s_id = item_id.split('_')[0]
        block = next((b for b in book_data.get("content_blocks", []) if b.get('s_id') == s_id), None)
        if not block: return False
        status = block.get("processing_status", {}).get(self.stage_name)
        return status in ["COMPLETED", "RETRY_SEMANTIC_FAIL"]