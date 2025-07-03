import json
import re
from typing import Any, Dict, List, Optional, Tuple
from .base import LLMStage, logger
from .. import llm_prompts


class GenerateAdvSpanish(LLMStage):
    """
    The first stage of the pipeline. Translates the source English text from a
    staged .txt file into Advanced Spanish using an LLM.
    """

    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        ## MODIFIED: Call super with the new required parameters
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=1,
            stage_name="GenerateAdvSpanish",
            parser_type='line'
        )
        # This holds the main JSON data structure as it's being built.
        self.book_data: Dict[str, Any] = {}

    # --- LLMStage Abstract Method Implementations ---

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for this stage."""
        # Using a helper from the base class could be cleaner, but this is fine.
        template_path = llm_prompts.PROMPT_DIR / "stage1_prompt.txt"
        try:
            return template_path.read_text(encoding="utf-8")
        except Exception as e:
            logger.critical(f"Could not load system prompt {template_path.name}: {e}")
            return ""

    ## NEW: Replaces prepare_llm_input
    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """
        For Stage 1, an atomic unit is a single sentence. This prepares its
        prompt part and estimates its token count.
        """
        prompt_text = f"{block['llm_block_id']}: {block['eng_text']}"
        token_estimate = self._estimate_tokens(prompt_text)
        
        prompt_parts = [
            {
                "llm_id": block["llm_block_id"],
                "prompt_text": prompt_text,
            }
        ]
        return prompt_parts, token_estimate

    def run(self) -> bool:
        logger.info(f"Executing Stage 1: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        if self.cli_args.force_book:
            if self.output_path.exists(): self.output_path.unlink()
            if self.log_path.exists(): self.log_path.unlink()

        if not self.cli_args.force_book and self._is_stage_complete():
            logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
            return True

        source_items = self._load_and_parse_source_file()
        if source_items is None: return False

        # This is now the main data object we will modify and save.
        self._initialize_or_resume_book_data()
        
        # Find which source items haven't been processed yet.
        processed_indices = {b['source_index_in_original_file'] for b in self.book_data.get("content_blocks", [])}
        items_to_process = [item for item in source_items if item["source_index"] not in processed_indices]
        
        logger.info(f"      -> Found {len(items_to_process)} source items to process for this run.")

        batch_token_limit = self.stage_config.get("batch_size_in_tokens", 8000)
        current_batch_items = []
        current_batch_token_count = 0
        
        for item in items_to_process:
            if item["type"] == "sentence":
                prompt_parts, token_estimate = self.prepare_atomic_unit(item)
                
                if current_batch_items and (current_batch_token_count + token_estimate > batch_token_limit):
                    if not self._process_batch_stage1(current_batch_items): return False
                    current_batch_items = [item]
                    current_batch_token_count = token_estimate
                else:
                    current_batch_items.append(item)
                    current_batch_token_count += token_estimate

            elif item["type"] == "chapter":
                if current_batch_items:
                    if not self._process_batch_stage1(current_batch_items): return False
                    current_batch_items, current_batch_token_count = [], 0
                
                logger.info(f"      -> Adding Chapter Marker: {item['text']}")
                self.book_data["content_blocks"].append({
                    "block_type": "chapter_marker", "marker_text": item["text"],
                    "source_index_in_original_file": item["source_index"],
                })

        if current_batch_items:
            if not self._process_batch_stage1(current_batch_items): return False

        logger.info("      -> Finalizing Stage 1 output.")
        self.book_data["processing_status"] = "COMPLETED"
        return self._save_progress()


    def process_llm_response(
        self, block: Dict[str, Any], llm_response: Dict[str, str]
    ) -> None:
        """
        Processes the LLM response for a single sentence and adds it to self.book_data.
        'block' is the original source item.
        'llm_response' contains the parsed data for the entire batch.
        """
        llm_id = block["llm_block_id"].lower()
        translated_text = llm_response.get(llm_id, "")
        if not translated_text:
            logger.warning(f"      -> Missing translation from LLM for ID: {llm_id}")

        content_block = {
            "block_type": "sentence",
            "source_index_in_original_file": block["source_index"],
            "llm_block_id": block["llm_block_id"], # Keep original case
            "original_sentence_s_id": block["s_id"],
            "english_text": block["eng_text"],
            "adv_spanish_full": {"text": translated_text, "lemmas": []},
            "adv_spanish_segments": [],
            "simpler_adv_spanish_full": {"text": "", "lemmas": []},
            "simple_spanish_l3_full": {"text": "", "lemmas": []},
            "simple_spanish_l3_segments": [],
            "phrase_alignments_l3_to_english": [],
            "simple_spanish_l3_lemmas_per_segment": {},
            "diglot_map_entries": [],
            "llm_call_status": {f"stage{self.stage_number}": "COMPLETED_LLM"},
            "processing_notes": [],
        }
        self.book_data["content_blocks"].append(content_block)

    # --- Overridden run() method for this special first stage ---
    # MODIFIED: This is now much simpler, it just delegates to the parent class's run method.
    def run(self) -> bool:
        logger.info(f"Executing Stage 1: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        if self.cli_args.force_book:
            if self.output_path.exists(): self.output_path.unlink()
            if self.log_path.exists(): self.log_path.unlink()

        if not self.cli_args.force_book and self._is_stage_complete():
            logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
            return True

        source_items = self._load_and_parse_source_file()
        if source_items is None: return False

        self._initialize_or_resume_book_data()
        
        last_processed_index = -1
        if self.book_data.get("content_blocks"):
            if self.book_data["content_blocks"]:
                last_processed_index = self.book_data["content_blocks"][-1].get("source_index_in_original_file", -1)

        # We now process the source items directly instead of JSON blocks
        items_to_process = [item for item in source_items if item["source_index"] > last_processed_index]
        
        # This is now the "data" object that the base run method expects to modify.
        # But for stage 1, process_llm_response modifies self.book_data directly.
        # This is a bit of a special case.
        
        # Simplified batching for Stage 1's unique structure
        batch_token_limit = self.stage_config.get("batch_size_in_tokens", 8000)
        current_batch_units = []
        current_batch_token_count = 0
        
        for item in items_to_process:
            if item["type"] == "sentence":
                prompt_text = f"{item['llm_block_id']}: {item['eng_text']}"
                token_estimate = self._estimate_tokens(prompt_text)

                if current_batch_units and (current_batch_token_count + token_estimate > batch_token_limit):
                    if not self._process_batch_stage1(current_batch_units): return False
                    current_batch_units = [{'unit_data': item, 'token_estimate': token_estimate}]
                    current_batch_token_count = token_estimate
                else:
                    current_batch_units.append({'unit_data': item, 'token_estimate': token_estimate})
                    current_batch_token_count += token_estimate

            elif item["type"] == "chapter":
                if current_batch_units:
                    if not self._process_batch_stage1(current_batch_units): return False
                    current_batch_units, current_batch_token_count = [], 0
                
                logger.info(f"      -> Adding Chapter Marker: {item['text']}")
                self.book_data["content_blocks"].append({
                    "block_type": "chapter_marker", "marker_text": item["text"],
                    "source_index_in_original_file": item["source_index"],
                })

        if current_batch_units:
            if not self._process_batch_stage1(current_batch_units): return False

        logger.info("      -> Finalizing Stage 1 output.")
        self.book_data["processing_status"] = "COMPLETED"
        return self._save_progress()
        
    def _process_batch_stage1(self, batch_units: List[Dict[str, Any]]) -> bool:
        self.batch_counter += 1
        logger.info(f"      -> Processing batch #{self.batch_counter} with {len(batch_units)} sentences...")

        prompt_parts = [f"{unit['unit_data']['llm_block_id']}: {unit['unit_data']['eng_text']}" for unit in batch_units]
        expected_ids = [unit['unit_data']['llm_block_id'] for unit in batch_units]
        user_prompt = "\n".join(prompt_parts)

        self._write_batch_header_to_log(user_prompt)
        parsed_data = self._make_api_call_with_retries(user_prompt, expected_ids)

        if parsed_data is None:
            self.book_data["processing_status"] = "FAILED"
            self._save_progress()
            return False

        for unit in batch_units:
            self.process_llm_response(unit['unit_data'], parsed_data)
        
        try:
            with open(self.log_path, "a", encoding="utf-8") as f:
                f.write(f"--- END OF BATCH {self.batch_counter} ---\n\n")
        except IOError as e:
            logger.warning(f"      -> Could not write batch footer to log file: {e}")

        return self._save_progress()

    # --- Helper Methods Specific to Stage 1 ---
    # (These remain largely the same, but simplified)
    def _load_and_parse_source_file(self) -> Optional[List[Dict[str, Any]]]:
        input_path = self.staged_dir / f"{self.book_stem}.txt"
        if not input_path.exists():
            logger.critical(f"CRITICAL: Source file for Stage 1 not found: {input_path}")
            return None
        try:
            raw_lines = input_path.read_text(encoding="utf-8").splitlines()
        except IOError as e:
            logger.critical(f"CRITICAL: Could not read source file {input_path.name}: {e}")
            return None

        all_source_items, sentence_regex, chapter_regex = [], re.compile(r"^{S(\d+):\s*(.*)}$"), re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")
        for idx, line in enumerate(raw_lines):
            if chapter_match := chapter_regex.match(line.strip()):
                all_source_items.append({"type": "chapter", "text": chapter_match.group(1).strip(), "source_index": idx})
            elif sentence_match := sentence_regex.match(line.strip()):
                s_id_val = int(sentence_match.group(1))
                all_source_items.append({
                    "type": "sentence", "s_id": f"S{s_id_val}", "llm_block_id": f"id {s_id_val}",
                    "eng_text": sentence_match.group(2).strip(), "source_index": idx,
                })
        return all_source_items

    def _initialize_or_resume_book_data(self) -> None:
        if not self.cli_args.force_book and self.output_path.exists():
            try:
                with open(self.output_path, "r", encoding="utf-8") as f:
                    self.book_data = json.load(f)
                logger.info(f"      -> Resuming from existing file: {self.output_path.name}")
                return
            except (json.JSONDecodeError, IOError) as e:
                logger.warning(f"Could not parse resume file: {e}. Starting over.")

        self.book_data = {
            "book_name": self.book_stem,
            "json_schema_version": "5.0",
            "pipeline_script_version": self.cli_args.version,
            "processing_status": "PARTIAL", "content_blocks": [],
        }
        logger.info("      -> Starting new data file from scratch.")

    def _save_progress(self) -> bool:
        try:
            with open(self.output_path, "w", encoding="utf-8") as f:
                json.dump(self.book_data, f, indent=2, ensure_ascii=False)
            return True
        except IOError as e:
            logger.error(f"CRITICAL: Could not write progress to {self.output_path.name}: {e}")
            return False