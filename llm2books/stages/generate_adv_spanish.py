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

    def __init__(self, book_stem: str, config: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            config=config,
            common_resources=common_resources,
            stage_number=1,
            stage_name = "GenerateAdvSpanish",
            batch_size = 5,
            parser_type = 'line'
        )
        # This holds the main JSON data structure as it's being built.
        self.book_data: Dict[str, Any] = {}

    # --- LLMStage Abstract Method Implementations ---

    def get_system_prompt(self) -> str:
        """Loads and returns the system prompt for this stage."""
        return llm_prompts.load_prompt_template("stage1_prompt.txt")

    def prepare_llm_input(
        self, block: Dict[str, Any], s_idx: int
    ) -> Optional[List[Dict[str, Any]]]:
        """
        Prepares the LLM input for a single source-file sentence item.
        For Stage 1, this is a simple one-to-one mapping.
        """
        # The 'block' here is an item from the source .txt file, not a content_block yet.
        # It should have keys like 'llm_block_id' and 'eng_text'.
        return [
            {
                "llm_id": block["llm_block_id"],
                "prompt_text": f"{block['llm_block_id']}: {block['eng_text']}",
            }
        ]

    def prepare_batch_prompt(
        self, batch: List[Dict[str, Any]]
    ) -> Tuple[str, List[str]]:
        """Formats a batch of sentence items into a user prompt."""
        prompt_lines = [f"{item['llm_block_id']}: {item['eng_text']}" for item in batch]
        user_prompt = "\n".join(prompt_lines)
        expected_ids = [item["llm_block_id"] for item in batch]
        return user_prompt, expected_ids

    def process_llm_response(
        self, block: Dict[str, Any], llm_response: Dict[str, str]
    ) -> None:
        """
        Processes the LLM response for a single sentence and adds it to self.book_data.
        'block' is the original source item.
        'llm_response' is the parsed data from the LLM for this block.
        """
        llm_id = block["llm_block_id"]
        # llm_response for Stage 1 will only have one key, which is the llm_id.
        translated_text = llm_response.get(llm_id, "")
        if not translated_text:
            logger.warning(f"      -> Missing translation from LLM for ID: {llm_id}")

        content_block = {
            "block_type": "sentence",
            "source_index_in_original_file": block["source_index"],
            "llm_block_id": llm_id,
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
    def run(self) -> bool:
        # This method's high-level logic remains the same because it's a special case.
        # ... (no changes needed to the main `run` loop itself) ...
        # It will still call `_process_batch`.
        logger.info(f"Executing Stage 1: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        if self.config.force_book:
            if self.output_path.exists():
                self.output_path.unlink()
            in_log_path = (
                self.stage_output_dir / f"{self.book_stem}.stage{self.stage_number}.in"
            )
            out_log_path = (
                self.stage_output_dir / f"{self.book_stem}.stage{self.stage_number}.out"
            )
            if in_log_path.exists():
                in_log_path.unlink()
            if out_log_path.exists():
                out_log_path.unlink()

        if not self.config.force_book and self._is_stage_complete():
            logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
            return True

        source_items = self._load_and_parse_source_file()
        if source_items is None:
            return False

        self._initialize_or_resume_book_data()

        last_processed_index = -1
        if self.book_data.get("content_blocks"):
            # Find the highest source_index from the last processed block
            if self.book_data["content_blocks"]:
                last_processed_index = self.book_data["content_blocks"][-1].get(
                    "source_index_in_original_file", -1
                )

        items_to_process = [
            item for item in source_items if item["source_index"] > last_processed_index
        ]

        current_batch: List[Dict[str, Any]] = []
        for item in items_to_process:
            if item["type"] == "sentence":
                current_batch.append(item)
                if len(current_batch) >= self.config.max_sentences_per_batch:
                    if not self._process_batch(current_batch):
                        return False
                    current_batch.clear()
                    self._save_progress()

            elif item["type"] == "chapter":
                if current_batch:
                    if not self._process_batch(current_batch):
                        return False
                    current_batch.clear()
                    self._save_progress()

                logger.info(f"      -> Adding Chapter Marker: {item['text']}")
                self.book_data["content_blocks"].append(
                    {
                        "block_type": "chapter_marker",
                        "marker_text": item["text"],
                        "source_index_in_original_file": item["source_index"],
                    }
                )

        if current_batch:
            if not self._process_batch(current_batch):
                return False

        logger.info("      -> Finalizing Stage 1 output.")
        self.book_data["processing_status"] = "COMPLETED"
        if self._save_progress():
            logger.info("      -> Successfully completed Stage 1.")
            return True
        return False

    # --- Helper Methods Specific to Stage 1 ---

    def _process_batch(self, batch: List[Dict[str, Any]]) -> bool:
        """
        Processes a single batch of source items through the LLM.
        This is a simplified version of the generic _process_batch in the parent.
        """
        logger.info(
            f"      -> Processing batch of {len(batch)} source items starting with S_ID {batch[0]['s_id']}."
        )

        # Prepare a single prompt for the entire batch
        batch_llm_inputs = []
        for item in batch:
            prepared_items = self.prepare_llm_input(
                item, 0
            )  # s_idx is not relevant here
            if prepared_items:
                batch_llm_inputs.extend(prepared_items)

        user_prompt = "\n".join([item["prompt_text"] for item in batch_llm_inputs])
        expected_ids = [item["llm_id"] for item in batch_llm_inputs]

        # Make the API call
        parsed_data = self._make_api_call_with_retries(user_prompt, expected_ids, batch)

        if parsed_data is None:
            self.book_data["processing_status"] = "FAILED"
            self._save_progress()
            return False

        # Process the response for each item in the original batch
        for item in batch:
            llm_id = item["llm_block_id"]
            # Create a response dict relevant only to this item
            item_response = {llm_id: parsed_data.get(llm_id, "")}
            self.process_llm_response(item, item_response)

        return True

    def _load_and_parse_source_file(self) -> Optional[List[Dict[str, Any]]]:
        """Loads the staged .txt file and parses it into a list of items."""
        input_path = self.staged_dir / f"{self.book_stem}.txt"
        if not input_path.exists():
            logger.critical(
                f"      -> CRITICAL: Source file for Stage 1 not found at {input_path}"
            )
            return None

        try:
            raw_lines = input_path.read_text(encoding="utf-8").splitlines()
        except IOError as e:
            logger.critical(
                f"      -> CRITICAL: Could not read source file {input_path.name}: {e}"
            )
            return None

        all_source_items = []
        sentence_regex = re.compile(r"^{S(\d+):\s*(.*)}$")
        chapter_regex = re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")

        for idx, line in enumerate(raw_lines):
            if chapter_match := chapter_regex.match(line.strip()):
                all_source_items.append(
                    {
                        "type": "chapter",
                        "text": chapter_match.group(1).strip(),
                        "source_index": idx,
                    }
                )
            elif sentence_match := sentence_regex.match(line.strip()):
                s_id_val = int(sentence_match.group(1))
                all_source_items.append(
                    {
                        "type": "sentence",
                        "s_id": f"S{s_id_val}",
                        "llm_block_id": f"id {s_id_val}",
                        "eng_text": sentence_match.group(2).strip(),
                        "source_index": idx,
                    }
                )
        return all_source_items

    def _initialize_or_resume_book_data(self) -> None:
        """Initializes a new book_data dict or loads an existing one for resuming."""
        if not self.config.force_book and self.output_path.exists():
            try:
                with open(self.output_path, "r", encoding="utf-8") as f:
                    self.book_data = json.load(f)
                logger.info(
                    f"      -> Resuming from existing file: {self.output_path.name}"
                )
                return
            except (json.JSONDecodeError, IOError) as e:
                logger.warning(
                    f"      -> Could not parse resume file: {e}. Starting from scratch."
                )

        self.book_data = {
            "book_name": self.book_stem,
            "json_schema_version": "5.0",
            "pipeline_script_version": self.config.version,  # Assuming version is in config
            "processing_status": "PARTIAL",
            "content_blocks": [],
        }
        logger.info("      -> Starting new data file from scratch.")

    def _save_progress(self) -> bool:
        """Saves the current state of self.book_data to the output file."""
        # Add a timestamp or other metadata if needed
        # self.book_data["processing_timestamp"] = get_iso_timestamp()
        try:
            with open(self.output_path, "w", encoding="utf-8") as f:
                json.dump(self.book_data, f, indent=2, ensure_ascii=False)
            return True
        except IOError as e:
            logger.error(
                f"      -> CRITICAL: Could not write progress to {self.output_path.name}: {e}"
            )
            return False
