# llm2books/stages/base.py

import logging
import re
import json
import time
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Dict, Any, List, Optional, Tuple

from .. import helper

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
        config: Any,
        common_resources: Dict[str, Any],
        stage_number: int,
        stage_name: str,
    ):
        self.book_stem = book_stem
        self.config = config
        self.resources = common_resources
        self.stage_number = stage_number
        self.stage_name = stage_name
        # A flag for multipart stages (e.g., 3a/3b)
        self.is_part_a = False

        content_project_dir_str = self.resources.get("content_project_dir")
        if not content_project_dir_str:
            raise ValueError("'content_project_dir' not found in common_resources.")

        self.content_project_root = Path(content_project_dir_str)
        self.staged_dir = self.content_project_root / self.config.input_staged_subdir
        self.llm_output_base_dir = (
            self.content_project_root / self.config.output_llm_subdir
        )
        self.stage_output_dir = self.llm_output_base_dir / f"stage{self.stage_number}"
        self.output_path = (
            self.stage_output_dir / f"{self.book_stem}.stage{self.stage_number}.json"
        )

    @abstractmethod
    def run(self) -> bool:
        """The main execution method for the stage."""
        pass

    def _is_stage_complete(self) -> bool:
        """Checks if the output file for this stage already exists and is marked complete."""
        if not self.output_path.exists():
            return False
        try:
            with open(self.output_path, "r", encoding="utf-8") as f:
                data = json.load(f)
            # For multipart stages (like 3a), a partial status is considered complete for that part.
            if self.is_part_a and data.get("processing_status", "").startswith(
                "PARTIAL"
            ):
                return True
            return data.get("processing_status") == "COMPLETED"
        except (json.JSONDecodeError, IOError):
            return False

    def _save_output_data(self, data: Dict[str, Any], status: str) -> bool:
        """Saves the data dictionary to the stage's output file with a status."""
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
    """
    Abstract Base Class for stages that exclusively use SpaCy for processing.
    """

    def __init__(
        self,
        book_stem: str,
        config: Any,
        common_resources: Dict[str, Any],
        stage_number: int,
        stage_name: str,
    ):
        super().__init__(book_stem, config, common_resources, stage_number, stage_name)

    def run(self) -> bool:
        logger.info(f"Executing SpaCy Stage {self.stage_number}: {self.stage_name}")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        if not self.config.force_book and self._is_stage_complete():
            logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
            return True

        input_data = self._load_input_data()
        if input_data is None:
            return False

        logger.info("      -> Processing data with SpaCy model...")
        output_data = self._process_data(input_data)

        # Determine the final status
        final_status = "COMPLETED"
        # If this is the 'a' part of a multi-step stage, its completion is partial.
        if self.is_part_a:
            final_status = f"PARTIAL_{self.stage_number}A_COMPLETE"

        if self._save_output_data(output_data, final_status):
            logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
            return True
        else:
            return False

    @abstractmethod
    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """The core processing logic for the specific SpaCy stage."""
        pass

    def _get_input_path(self) -> Path:
        """Determines the input file path for a SpaCy stage."""
        if self.stage_number <= 1:
            raise ValueError("SpaCyStage cannot be the first stage in the pipeline.")

        # A SpaCy stage always reads from the output of the previous stage number.
        prev_stage_num = self.stage_number - 1
        return (
            self.llm_output_base_dir
            / f"stage{prev_stage_num}"
            / f"{self.book_stem}.stage{prev_stage_num}.json"
        )

    def _load_input_data(self) -> Optional[Dict[str, Any]]:
        """Loads and parses the JSON input file for the stage."""
        input_path = self._get_input_path()
        if not input_path.exists():
            logger.critical(f"      -> CRITICAL: Input file not found at {input_path}")
            return None

        try:
            with open(input_path, "r", encoding="utf-8") as f:
                return json.load(f)
        except (IOError, json.JSONDecodeError) as e:
            logger.critical(
                f"      -> CRITICAL: Could not read or parse input file {input_path.name}: {e}"
            )
            return None


class LLMStage(Stage, ABC):
    """
    Abstract Base Class for stages that use an LLM for processing.
    """

    def __init__(
        self,
        book_stem: str,
        config: Any,
        common_resources: Dict[str, Any],
        stage_number: int,
        stage_name: str,
        batch_size: int,
        parser_type: str = "line",
    ):
        super().__init__(book_stem, config, common_resources, stage_number, stage_name)
        self.in_log_path = (
            self.stage_output_dir / f"{self.book_stem}.stage{self.stage_number}.in"
        )
        self.out_log_path = (
            self.stage_output_dir / f"{self.book_stem}.stage{self.stage_number}.out"
        )
        self.batch_size = batch_size
        self.parser_type = parser_type

    @abstractmethod
    def get_system_prompt(self) -> str:
        pass

    @abstractmethod
    def prepare_llm_input(
        self, block: Dict[str, Any], s_idx: int
    ) -> Optional[List[Dict[str, Any]]]:
        pass

    @abstractmethod
    def process_llm_response(
        self, block: Dict[str, Any], llm_response: Dict[str, str]
    ) -> None:
        pass

    def run(self) -> bool:
        logger.info(
            f"Executing LLM Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'"
        )
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        if not self.config.force_book and self._is_stage_complete():
            logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
            return True

        data = self._load_input_data()
        if data is None:
            return False

        items_to_process = self._get_items_to_process(data)
        if not items_to_process:
            logger.info(
                "      -> No items to process for this stage. Marking as complete."
            )
            return self._save_progress(data, "COMPLETED")

        logger.info(
            f"      -> Found {len(items_to_process)} content blocks needing processing."
        )

        current_batch: List[Dict[str, Any]] = []
        for item in items_to_process:
            current_batch.append(item)
            if len(current_batch) >= self.batch_size:
                if not self._process_batch(current_batch, data):
                    return False
                current_batch.clear()

        if current_batch:
            if not self._process_batch(current_batch, data):
                return False

        logger.info(f"      -> Finalizing Stage {self.stage_number} output.")
        return self._save_progress(data, "COMPLETED")

    def _get_input_path(self) -> Path:
        # A multipart stage (like 3b, 5b) reads from its own stage number file
        # as it is modifying the output of part 'a'.
        is_multipart_stage_b = (
            "Simplify" in self.stage_name or "GenerateSimpleSpanish" in self.stage_name
        )

        if is_multipart_stage_b:
            return self.output_path

        # Standard stages read from the previous stage's output.
        # This handles Stage 2, 4, 6, 7.
        if self.stage_number > 1:
            prev_stage_num = self.stage_number - 1
            # Special case: Stage 4 reads from 3, Stage 6 from 5.
            if self.stage_name == "FinalizeSimplerSpanish":
                prev_stage_num = 3
            if self.stage_name == "LemmatizeSimpleSpanish":
                prev_stage_num = 5

            return (
                self.llm_output_base_dir
                / f"stage{prev_stage_num}"
                / f"{self.book_stem}.stage{prev_stage_num}.json"
            )

        # Stage 1 reads from the initial staged text file.
        if self.stage_name == "GenerateAdvSpanish":
            return self.staged_dir / f"{self.book_stem}.txt"

        raise ValueError(f"Could not determine input path for stage {self.stage_name}")

    def _load_input_data(self) -> Optional[Dict[str, Any]]:
        input_path = self._get_input_path()
        if not input_path.exists():
            logger.critical(f"      -> CRITICAL: Input file not found at {input_path}")
            return None
        try:
            with open(input_path, "r", encoding="utf-8") as f:
                return json.load(f)
        except (IOError, json.JSONDecodeError) as e:
            logger.critical(
                f"      -> CRITICAL: Could not read or parse input file {input_path.name}: {e}"
            )
            return None

    def _get_items_to_process(self, data: Dict[str, Any]) -> List[Dict[str, Any]]:
        status_key = f"stage{self.stage_number}"
        if "Simplify" in self.stage_name or "GenerateSimpleSpanish" in self.stage_name:
            status_key += "b"

        items_to_process = []
        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                # Check if the specific status key for this LLM stage is NOT 'COMPLETED_LLM'
                if block.get("llm_call_status", {}).get(status_key) != "COMPLETED_LLM":
                    items_to_process.append(block)
        return items_to_process

    def _process_batch(
        self, batch_blocks: List[Dict[str, Any]], data: Dict[str, Any]
    ) -> bool:
        logger.info(
            f"      -> Processing batch of {len(batch_blocks)} sentence-blocks..."
        )

        prompt_parts = []
        expected_ids = []
        
        for s_idx, block in enumerate(batch_blocks):
            prepared_items = self.prepare_llm_input(block, s_idx)
            if prepared_items:
                for item in prepared_items:
                    prompt_parts.append(item["prompt_text"])
                    expected_ids.append(item["llm_id"])

        if not prompt_parts:
            logger.info("      -> Batch resulted in no items for the LLM. Skipping and saving progress.")
            return self._save_progress(data, "PARTIAL")

        user_prompt = "\n".join(prompt_parts)
        
        parsed_data = self._make_api_call_with_retries(
            user_prompt, expected_ids, batch_blocks
        )

        if parsed_data is None:
            self._save_progress(data, "FAILED")
            return False

        # --- THIS IS THE FIX ---
        # REMOVE the complex filtering.
        # SIMPLY loop through the blocks and pass the ENTIRE parsed_data dictionary.
        # The individual stage's `process_llm_response` will be responsible
        # for looking up the specific IDs it needs from the batch's data.
        for block in batch_blocks:
            self.process_llm_response(block, parsed_data)
        # --- END FIX ---

        return self._save_progress(data, "PARTIAL")

    def _save_progress(self, data: Dict[str, Any], status: str) -> bool:
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

    def _make_api_call_with_retries(
        self,
        user_prompt: str,
        expected_ids: List[str],
        batch_blocks: List[Dict[str, Any]],
    ) -> Optional[Dict[str, str]]:
        system_prompt = self.get_system_prompt()
        if not system_prompt:
            logger.critical("      -> CRITICAL: System prompt is missing.")
            return None

        parser_func = (
            self._parse_llm_response_block
            if self.parser_type == "block"
            else self._parse_llm_response_line
        )
        last_error_reason, llm_response_text = "Unknown error", ""

        current_prompt = user_prompt
        for validation_attempt in range(self.config.max_validation_retries):
            response = None
            for api_attempt in range(self.config.max_api_retries):
                response, error_msg = self._make_llm_api_call(
                    system_prompt,
                    current_prompt,
                    self.config.llm_model,
                    api_attempt + 1,
                )
                if response:
                    break
                last_error_reason = (
                    f"API Error with '{self.config.llm_model}': {error_msg}"
                )
                if api_attempt < self.config.max_api_retries - 1:
                    time.sleep(self.config.retry_delay)

            llm_response_text = response or ""
            if llm_response_text:
                parsed_data, errors = parser_func(response, expected_ids)
                if not errors:
                    logger.info(
                        f"        -> Successful validation on attempt {validation_attempt + 1} with primary model."
                    )
                    self._log_api_traffic(
                        current_prompt, llm_response_text, self.config.llm_model
                    )
                    return parsed_data
                else:
                    last_error_reason = f"Validation Error (Primary Model): {errors}"
                    current_prompt += f"\n\nPRIOR_ATTEMPT_FAILED: Your last response had errors: {errors}. Please correct your output and ensure all expected IDs are present and correctly formatted."
            else:
                break

        if self.config.llm_fallback_model:
            logger.warning(
                f"      -> Primary model failed. Escalating to fallback model: {self.config.llm_fallback_model}"
            )
            current_prompt = user_prompt

            for validation_attempt in range(self.config.max_validation_retries):
                response = None
                for api_attempt in range(self.config.max_api_retries):
                    response, error_msg = self._make_llm_api_call(
                        system_prompt,
                        current_prompt,
                        self.config.llm_fallback_model,
                        api_attempt + 1,
                    )
                    if response:
                        break
                    last_error_reason = f"API Error with '{self.config.llm_fallback_model}': {error_msg}"
                    if api_attempt < self.config.max_api_retries - 1:
                        time.sleep(self.config.retry_delay)

                llm_response_text = response or ""
                if llm_response_text:
                    parsed_data, errors = parser_func(response, expected_ids)
                    if not errors:
                        logger.info(
                            "        -> Successful validation with fallback model."
                        )
                        self._log_api_traffic(
                            current_prompt,
                            llm_response_text,
                            self.config.llm_fallback_model,
                        )
                        return parsed_data
                    else:
                        last_error_reason = (
                            f"Validation Error (Fallback Model): {errors}"
                        )
                        current_prompt += f"\n\nPRIOR_ATTEMPT_FAILED: Your last response had errors: {errors}. Please correct your output."
                else:
                    break

        logger.critical(
            f"      -> PERMANENT FAILURE for batch. Last error: {last_error_reason}"
        )
        batch_info_str = (
            f"Failing Batch starts with IDs: {', '.join(expected_ids[:3])}..."
        )
        self._write_error_debug_file(
            batch_info=batch_info_str,
            last_prompt=current_prompt,
            last_response=llm_response_text,
            error_reason=last_error_reason,
        )
        return None

    def _log_api_traffic(self, user_prompt: str, response_text: str, model_name: str):
        try:
            with open(self.in_log_path, "a", encoding="utf-8") as f_in:
                f_in.write(f"--- BATCH (Model: {model_name}) ---\n{user_prompt}\n\n")
            with open(self.out_log_path, "a", encoding="utf-8") as f_out:
                f_out.write(f"--- BATCH (Model: {model_name}) ---\n{response_text}\n\n")
        except IOError as e:
            logger.warning(f"      -> Could not write to .in/.out log files: {e}")

    def _write_error_debug_file(
        self, batch_info: str, last_prompt: str, last_response: str, error_reason: str
    ):
        error_dir = self.llm_output_base_dir / "errors"
        error_dir.mkdir(parents=True, exist_ok=True)
        file_path = error_dir / f"{self.book_stem}.stage{self.stage_number}.err.txt"
        try:
            with open(file_path, "w", encoding="utf-8") as f:
                f.write("--- WEAVELANG PIPELINE: FATAL BATCH ERROR ---\n")
                f.write(f"Book: {self.book_stem}\n")
                f.write(f"Stage: {self.stage_number} ({self.stage_name})\n")
                f.write(f"Timestamp: {time.asctime()}\n")
                f.write(f"Error Reason: {error_reason}\n")
                f.write("\n--- BATCH INFO ---\n")
                f.write(f"{batch_info}\n")
                f.write("\n--- LAST PROMPT SENT TO LLM ---\n")
                f.write(last_prompt)
                f.write("\n\n--- LAST RAW RESPONSE RECEIVED ---\n")
                f.write(last_response)
            logger.critical(f"Wrote fatal error details to: {file_path}")
        except IOError as e:
            logger.critical(f"Could not write fatal error file to {file_path}: {e}")

    def _make_llm_api_call(
        self, system_prompt: str, user_prompt: str, model: str, attempt: int
    ) -> Tuple[Optional[str], str]:
        logger.info(f"        -> Making API call to {model} (Attempt {attempt})...")
        if self.config.llm_provider == "claude":
            if not anthropic:
                return None, "Anthropic SDK not installed."
            try:
                message = self.resources["llm_client"].messages.create(
                    model=model,
                    system=system_prompt,
                    messages=[{"role": "user", "content": user_prompt}],
                    max_tokens=4096,
                )
                return message.content[0].text, ""
            except anthropic.APIError as e:
                return None, f"Anthropic APIError: {e}"
            except Exception as e:
                return None, f"Generic Exception: {e}"
        return None, f"Unsupported LLM provider: {self.config.llm_provider}"

    def _parse_llm_response_line(
        self, raw_text: str, expected_ids: List[str]
    ) -> Tuple[Dict[str, str], List[str]]:
        parsed_data: Dict[str, str] = {}
        errors: List[str] = []
        id_line_regex = re.compile(
            r"^\s*(id\s+[\w_]+)\s*:(.*)$", re.IGNORECASE | re.MULTILINE
        )
        for match in id_line_regex.finditer(raw_text):
            block_id = match.group(1).strip().lower()
            content = match.group(2).strip()
            parsed_data[block_id] = content

        found_ids_set = set(parsed_data.keys())
        expected_ids_set = set(key.lower() for key in expected_ids)
        missing_ids = expected_ids_set - found_ids_set

        if missing_ids:
            errors.append(
                f"Missing IDs in response: {', '.join(sorted(list(missing_ids)))}"
            )

        return parsed_data, errors

    def _parse_llm_response_block(
        self, raw_text: str, expected_ids: List[str]
    ) -> Tuple[Dict[str, str], List[str]]:
        parsed_data: Dict[str, str] = {}
        errors: List[str] = []

        # --- START OF NEW IMPLEMENTATION ---
        
        # This regex just finds the ID lines, which we'll use as delimiters.
        # The `()` are crucial as they tell re.split to keep the delimiter.
        delimiter_regex = re.compile(r"(^\s*id\s+[\w_]+:?)", re.IGNORECASE | re.MULTILINE)
        
        # Split the text into a list: ['', 'id 1:', 'content1', 'id 2:', 'content2', ...]
        parts = delimiter_regex.split(raw_text)

        # The first item is any text before the first ID, which we can ignore.
        # We iterate over the remaining parts in pairs of (ID, content).
        if len(parts) > 1:
            for i in range(1, len(parts), 2):
                # The ID is the delimiter itself. Clean it up.
                block_id = parts[i].strip().rstrip(':').lower()
                
                # The content is the very next item in the list.
                content = ""
                if i + 1 < len(parts):
                    content = parts[i+1].strip()
                
                parsed_data[block_id] = content

        # --- END OF NEW IMPLEMENTATION ---

        # The validation logic below remains the same.
        found_ids_set = set(parsed_data.keys())
        expected_ids_set = set(key.lower() for key in expected_ids)
        missing_ids = expected_ids_set - found_ids_set

        if missing_ids:
            errors.append(
                f"Missing IDs in response: {', '.join(sorted(list(missing_ids)))}"
            )

        return parsed_data, errors
