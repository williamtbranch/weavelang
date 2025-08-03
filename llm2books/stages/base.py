# llm2books/stages/base.py

import logging
import re
import json
import time
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Dict, Any, List, Optional, Tuple


# --- Attempt to import optional libraries ---
try:
    import anthropic
except ImportError:
    anthropic = None

# Get the pipeline's logger
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
        self.staged_dir = self.content_project_root / "Staged"
        self.llm_output_base_dir = self.content_project_root / "pipeline"
        self.stage_output_dir = self.llm_output_base_dir / f"stage{self.stage_number}"
        self.output_path = (
            self.stage_output_dir / f"{self.book_stem}.stage{self.stage_number}.json"
        )

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

    def _get_input_path(self) -> Path:
        if self.stage_number <= 1:
            raise ValueError("SpaCyStage cannot be the first stage in the pipeline.")
        
        prev_stage_num = self.stage_number - 1
        return (
            self.llm_output_base_dir
            / f"stage{prev_stage_num}"
            / f"{self.book_stem}.stage{prev_stage_num}.json"
        )

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

class LLMStage(Stage, ABC):
    # --- THIS IS THE NEW, CORRECT CONSTRUCTOR ---
    def __init__(
        self,
        book_stem: str,
        cli_args: Any,
        common_resources: Dict[str, Any],
        stage_number: int,
        stage_name: str,
        parser_type: str = "line",
    ):
        # First, call the parent constructor to set up paths, etc.
        super().__init__(book_stem, cli_args, common_resources, stage_number, stage_name)
        
        # Now, perform the LLM-specific initialization, including getting its own config.
        # This overrides the self.stage_config set by the parent.
        self.stage_config = self.stages_config.get(self.__class__.__name__, {})
        
        # Optional: Keep the debug lines for one more run to confirm the fix.
        # You can remove these after it's confirmed to be working.
        class_name_str = self.__class__.__name__
        print(f"--- DEBUG FOR STAGE: {class_name_str} (Stage {self.stage_number}) ---")
        print(f"DEBUG_DATA: self.stages_config = {self.stages_config}")
        print(f"DEBUG_RESULT: Config for '{class_name_str}' is: {self.stage_config}")
        print(f"--- END DEBUG ---")

        self.log_path = (
            self.stage_output_dir / f"{self.book_stem}.stage{self.stage_number}.log"
        )
        self.parser_type = parser_type
        self.batch_counter = 0
    # --- END OF NEW CONSTRUCTOR ---

    @abstractmethod
    def get_system_prompt(self) -> str:
        pass

    @abstractmethod
    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        pass

    @abstractmethod
    def process_llm_response(
        self, block: Dict[str, Any], llm_response: Dict[str, str]
    ) -> None:
        pass
    
    def _estimate_tokens(self, text: str) -> int:
        return len(text) // 4

    def run(self) -> bool:
        logger.info(
            f"Executing LLM Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'"
        )
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        if not self.cli_args.force_book and self._is_stage_complete():
            logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
            return True
        
        if self.cli_args.force_book == self.book_stem:
            if self.output_path.exists(): self.output_path.unlink()
            if self.log_path.exists(): self.log_path.unlink()

        data = self._load_data_for_processing()
        if data is None:
            if self.stage_number == 1:
                data = {}
            else:
                return False

        items_to_process = self._get_items_to_process(data)
        if not items_to_process:
            logger.info(
                "      -> No items to process for this stage (already complete)."
            )
            # Finalize the existing data as complete for this stage
            self._save_progress(data, "COMPLETED")
            return True

        logger.info(
            f"      -> Found {len(items_to_process)} content blocks needing processing for this run."
        )

        batch_token_limit = self.stage_config.get("batch_size_in_tokens", 8000)
        logger.info(f"      -> Using token-based batching with a limit of ~{batch_token_limit} tokens.")
        
        current_batch_units = []
        current_batch_token_count = 0

        for unit in items_to_process:
            prompt_parts_for_unit, unit_token_estimate = self.prepare_atomic_unit(unit)

            if not prompt_parts_for_unit:
                continue
                
            if unit_token_estimate > batch_token_limit:
                logger.warning(f"      -> Atomic unit for S_ID {unit.get('original_sentence_s_id', 'N/A')} "
                               f"(~{unit_token_estimate} tokens) exceeds batch limit of {batch_token_limit}. "
                               "Processing it in its own oversized batch.")
                if current_batch_units:
                    if not self._process_batch(current_batch_units, data): return False
                    current_batch_units, current_batch_token_count = [], 0
                
                oversized_batch = [{'unit_data': unit, 'prompt_parts': prompt_parts_for_unit}]
                if not self._process_batch(oversized_batch, data): return False
                continue

            if current_batch_units and (current_batch_token_count + unit_token_estimate > batch_token_limit):
                if not self._process_batch(current_batch_units, data): return False
                current_batch_units = [{'unit_data': unit, 'prompt_parts': prompt_parts_for_unit}]
                current_batch_token_count = unit_token_estimate
            else:
                current_batch_units.append({'unit_data': unit, 'prompt_parts': prompt_parts_for_unit})
                current_batch_token_count += unit_token_estimate

        if current_batch_units:
            if not self._process_batch(current_batch_units, data): return False

        logger.info(f"      -> Finalizing Stage {self.stage_number} output.")
        return self._save_progress(data, "COMPLETED")

    def _load_data_for_processing(self) -> Optional[Dict[str, Any]]:
        if not self.cli_args.force_book == self.book_stem and self.output_path.exists():
            logger.info(f"      -> Resuming from existing partial file: {self.output_path.name}")
            try:
                with open(self.output_path, "r", encoding="utf-8") as f:
                    return json.load(f)
            except (IOError, json.JSONDecodeError) as e:
                logger.warning(
                    f"      -> Could not read or parse resume file {self.output_path.name}: {e}. "
                    "Will start stage from scratch."
                )
        
        logger.info("      -> Starting stage from scratch, loading previous stage's output.")
        input_path = self._get_input_path_for_stage()
        
        if self.stage_number == 1:
            return None

        if not input_path.exists():
            logger.critical(f"      -> CRITICAL: Input file not found at {input_path}")
            return None
        
        try:
            with open(input_path, "r", encoding="utf-8") as f:
                return json.load(f)
        except (IOError, json.JSONDecodeError) as e:
            logger.critical(f"      -> CRITICAL: Could not read/parse input file {input_path.name}: {e}")
            return None

    def _get_input_path_for_stage(self) -> Path:
        is_part_b_of_multipart_stage = "Simplify" in self.stage_name or "GenerateSimpleSpanish" in self.stage_name
        if is_part_b_of_multipart_stage:
            return self.output_path

        if self.stage_number == 1:
             return self.staged_dir / f"{self.book_stem}.txt"

        prev_stage_num = self.stage_number - 1
        return (
            self.llm_output_base_dir
            / f"stage{prev_stage_num}"
            / f"{self.book_stem}.stage{prev_stage_num}.json"
        )
    
    def _get_items_to_process(self, data: Dict[str, Any]) -> List[Dict[str, Any]]:
        if self.stage_number == 1:
            return []
        
        status_key = f"stage{self.stage_number}"
        if "Simplify" in self.stage_name or "GenerateSimpleSpanish" in self.stage_name:
            status_key += "b" 

        items_to_process = []
        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                if block.get("llm_call_status", {}).get(status_key) != "COMPLETED_LLM":
                    items_to_process.append(block)
        return items_to_process

    def _process_batch(
        self, batch_units: List[Dict[str, Any]], data: Dict[str, Any]
    ) -> bool:
        self.batch_counter += 1
        logger.info(
            f"      -> Processing batch #{self.batch_counter} with {len(batch_units)} atomic units..."
        )

        prompt_parts = []
        expected_ids = []
        for unit_info in batch_units:
            for part in unit_info['prompt_parts']:
                prompt_parts.append(part["prompt_text"])
                expected_ids.append(part["llm_id"])

        user_prompt = "\n".join(prompt_parts)
        self._write_batch_header_to_log(user_prompt)
        
        parsed_data = self._make_api_call_with_retries(user_prompt, expected_ids)

        if parsed_data is None:
            self._save_progress(data, "FAILED")
            return False

        for unit_info in batch_units:
            self.process_llm_response(unit_info['unit_data'], parsed_data)
        
        try:
            with open(self.log_path, "a", encoding="utf-8") as f:
                f.write(f"--- END OF BATCH {self.batch_counter} ---\n\n")
        except IOError as e:
            logger.warning(f"      -> Could not write batch footer to log file: {e}")

        return self._save_progress(data, "PARTIAL")

    def _save_progress(self, data: Dict[str, Any], status: str) -> bool:
        if self.stage_number == 1:
            data_to_save = self.book_data
        else:
            data_to_save = data

        data_to_save["processing_status"] = status
        try:
            with open(self.output_path, "w", encoding="utf-8") as f:
                json.dump(data_to_save, f, indent=2, ensure_ascii=False)
            return True
        except IOError as e:
            logger.error(
                f"      -> CRITICAL: Could not write progress to {self.output_path.name}: {e}"
            )
            return False
    
    def _make_api_call_with_retries(
        self, user_prompt: str, expected_ids: List[str], system_prompt_override: Optional[str] = None
    ) -> Optional[Dict[str, str]]:
        system_prompt = system_prompt_override if system_prompt_override is not None else self.get_system_prompt()
        if not system_prompt:
            logger.critical("      -> CRITICAL: System prompt is missing.")
            return None

        parser_func = self._parse_llm_response_block if self.parser_type == "block" else self._parse_llm_response_line
        
        primary_model_key = self.stage_config.get("primary_model")
        fallback_model_key = self.stage_config.get("fallback_model")

        parsed_data = self._attempt_model(system_prompt, user_prompt, primary_model_key, "primary", parser_func, expected_ids)
        if parsed_data is not None:
            return parsed_data

        if fallback_model_key:
            logger.warning(f"      -> Primary model failed. Escalating to fallback model '{fallback_model_key}'")
            parsed_data = self._attempt_model(system_prompt, user_prompt, fallback_model_key, "fallback", parser_func, expected_ids)
            if parsed_data is not None:
                return parsed_data
        
        batch_info_str = f"Failing Batch starts with IDs: {', '.join(expected_ids[:3])}..."
        self._write_error_debug_file(
            batch_info=batch_info_str,
            last_prompt=user_prompt,
            error_reason="All API and validation attempts failed for all configured models."
        )
        return None

    def _attempt_model(self, system_prompt, user_prompt, model_key, model_tier, parser_func, expected_ids):
        if not model_key:
            logger.info(f"      -> No model defined for tier '{model_tier}'. Skipping.")
            return None
        
        model_config = self.models_config.get(model_key)
        if not model_config:
            logger.error(f"      -> Model key '{model_key}' not found in [models] section of config. Skipping.")
            return None
        
        model_name = model_config.get("name")
        max_api_retries = self.pipeline_config.get("max_api_retries", 3)
        max_validation_retries = self.pipeline_config.get("max_validation_retries", 4)
        
        last_error_reason = "Unknown error"
        current_prompt = user_prompt
        
        total_api_attempts = 0

        for validation_attempt in range(max_validation_retries):
            response = None
            for _ in range(max_api_retries):
                total_api_attempts += 1
                response, error_msg = self._make_llm_api_call(
                    system_prompt, current_prompt, model_name, total_api_attempts
                )
                if response: break
                last_error_reason = f"API Error with '{model_name}': {error_msg}"
                if _ < max_api_retries - 1:
                    time.sleep(self.pipeline_config.get("retry_delay", 5))
            
            raw_response_text = response or ""
            
            if response:
                parsed_data, errors = parser_func(response, expected_ids)
                if not errors:
                    logger.info(f"        -> Successful validation on validation attempt {validation_attempt + 1} with {model_tier} model.")
                    self._log_attempt(model_name, "SUCCESS", "Validation passed", raw_response_text)
                    return parsed_data
                else:
                    last_error_reason = f"Validation Error ({model_name}): {errors}"
                    current_prompt += f"\n\nPRIOR_ATTEMPT_FAILED: Your last response had errors: {errors}. Please correct."
            else:
                self._log_attempt(model_name, "FAILED", last_error_reason, raw_response_text)
                return None
            
            self._log_attempt(model_name, "FAILED", last_error_reason, raw_response_text)
        return None

    def _write_batch_header_to_log(self, user_prompt: str):
        try:
            with open(self.log_path, "a", encoding="utf-8") as f:
                f.write(f"--- BATCH {self.batch_counter} ---\n")
                f.write("USER_PROMPT_DATA_START\n")
                f.write(user_prompt + "\n")
                f.write("USER_PROMPT_DATA_END\n\n")
        except IOError as e:
            logger.warning(f"      -> Could not write batch header to log file: {e}")
            
    def _log_attempt(self, model_name: str, status: str, reason: str, response_text: str):
        try:
            with open(self.log_path, "a", encoding="utf-8") as f:
                f.write(f"--- ATTEMPT (Model: {model_name}) ---\n")
                f.write(f"STATUS: {status}\n")
                f.write(f"REASON: {reason}\n")
                f.write("RESPONSE_START\n")
                f.write(response_text + "\n")
                f.write("RESPONSE_END\n\n")
        except IOError as e:
            logger.warning(f"      -> Could not write attempt to log file: {e}")
            
    def _write_error_debug_file(self, batch_info: str, last_prompt: str, error_reason: str):
        error_dir = self.llm_output_base_dir / "errors"
        error_dir.mkdir(parents=True, exist_ok=True)
        file_path = error_dir / f"{self.book_stem}.stage{self.stage_number}.err.txt"
        try:
            with open(file_path, "w", encoding="utf-8") as f:
                f.write(f"--- WEAVELANG PIPELINE: FATAL BATCH ERROR ---\nBook: {self.book_stem}\n")
                f.write(f"Stage: {self.stage_number} ({self.stage_name})\nTimestamp: {time.asctime()}\n")
                f.write(f"Error Reason: {error_reason}\n\n--- BATCH INFO ---\n{batch_info}\n")
                f.write(f"\n--- FINAL PROMPT SENT TO LLM ---\n{last_prompt}\n\n--- NOTE ---\n")
                f.write(f"See the full sequence of attempts in {self.log_path.relative_to(self.content_project_root)}")
            logger.critical(f"Wrote fatal error details to: {file_path}")
        except IOError as e:
            logger.critical(f"Could not write fatal error file to {file_path}: {e}")

    def _make_llm_api_call(self, system_prompt: str, user_prompt: str, model: str, attempt: int) -> Tuple[Optional[str], str]:
        logger.info(f"        -> Making API call to {model} (Attempt {attempt})...")
        provider = self.pipeline_config.get("llm_provider", "claude")
        if provider == "claude":
            if not anthropic: return None, "Anthropic SDK not installed."
            try:
                message = self.resources["llm_client"].messages.create(
                    model=model, system=system_prompt,
                    messages=[{"role": "user", "content": user_prompt}], max_tokens=8192,
                )
                return message.content[0].text, ""
            except anthropic.APIError as e:
                return None, f"Anthropic APIError: {e}"
            except Exception as e:
                return None, f"Generic Exception: {e}"
        return None, f"Unsupported LLM provider: {provider}"
    
    def _parse_llm_response_line(self, raw_text: str, expected_ids: List[str]) -> Tuple[Dict[str, str], List[str]]:
        parsed_data: Dict[str, str] = {}
        errors: List[str] = []
        id_line_regex = re.compile(r"^\s*(id\s+[\w_]+)\s*:(.*)$", re.IGNORECASE | re.MULTILINE)
        for match in id_line_regex.finditer(raw_text):
            block_id = match.group(1).strip().lower()
            content = match.group(2).strip()
            parsed_data[block_id] = content
        invalid_ids = []
        for expected_id in expected_ids:
            eid_lower = expected_id.lower()
            if eid_lower not in parsed_data or not parsed_data[eid_lower]:
                invalid_ids.append(expected_id)
        if invalid_ids:
            errors.append(f"Missing or empty content for IDs: {', '.join(sorted(invalid_ids))}")
        return parsed_data, errors

    def _parse_llm_response_block(self, raw_text: str, expected_ids: List[str]) -> Tuple[Dict[str, str], List[str]]:
        parsed_data: Dict[str, str] = {}
        errors: List[str] = []
        delimiter_regex = re.compile(r"(^\s*id\s+[\w_]+:?)", re.IGNORECASE | re.MULTILINE)
        parts = delimiter_regex.split(raw_text)
        if len(parts) > 1:
            for i in range(1, len(parts), 2):
                block_id = parts[i].strip().rstrip(':').lower()
                content = ""
                if i + 1 < len(parts): content = parts[i+1].strip()
                parsed_data[block_id] = content
        invalid_ids = []
        for expected_id in expected_ids:
            eid_lower = expected_id.lower()
            if eid_lower not in parsed_data or not parsed_data[eid_lower]:
                invalid_ids.append(expected_id)
        if invalid_ids:
            errors.append(f"Missing or empty content for IDs: {', '.join(sorted(invalid_ids))}")
        return parsed_data, errors