import os
import json
from typing import Dict, List
import time


class Stage1:
    args = None
    llm_client = None
    book_stem = None
    staged_dir = None
    llm_output_base_dir = None
    system_prompt = None
    in_log_path = None
    out_log_path = None
    stage_num = 1
    book_data = None
    error_output_dir = None

    def __init__(self, helper, book_stem, llm_client, args, staged_dir, llm_output_base_dir):
        self.args = args
        self.llm_client = llm_client
        self.book_stem = book_stem
        self.staged_dir = staged_dir
        self.llm_output_base_dir = llm_output_base_dir
        self.helper = helper

        self.stage_output_dir = llm_output_base_dir / f"stage{self.stage_num}"
        self.output_path = (
            self.stage_output_dir / f"{book_stem}.stage{self.stage_num}.json"
        )
        self.in_log_path = (
            self.stage_output_dir / f"{book_stem}.stage{self.stage_num}.in"
        )
        self.out_log_path = (
            self.stage_output_dir / f"{book_stem}.stage{self.stage_num}.out"
        )
        self.error_output_dir = llm_output_base_dir / "errors"

    def genBlock(self, item, parsed_data):
        return {
            "block_type": "sentence",
            "source_index_in_original_file": item["source_index"],
            "llm_block_id": item["llm_block_id"],
            "original_sentence_s_id": item["s_id"],
            "english_text": item["eng_text"],
            "adv_spanish_full": {
                "text": parsed_data.get(item["llm_block_id"], ""),
                "lemmas": [],
            },
            "adv_spanish_segments": [],
            "simpler_adv_spanish_full": {"text": "", "lemmas": []},
            "simple_spanish_l3_full": {"text": "", "lemmas": []},
            "simple_spanish_l3_segments": [],
            "phrase_alignments_l3_to_english": [],
            "simple_spanish_l3_lemmas_per_segment": {},
            "diglot_map_entries": [],
            "llm_call_status": {f"stage{self.stage_num}": "COMPLETED"},
            "processing_notes": [],
        }

    def process_batch(self, batch: List[Dict]) -> bool:
        if not batch:
            return True
        self.helper.logger.info(
            f"      Processing batch of {len(batch)} sentences starting with S_ID {batch[0]['s_id']}."
        )

        batch_ids = [item["llm_block_id"] for item in batch]
        user_prompt = "\n".join(
            [f"{item['llm_block_id']}: {item['eng_text']}" for item in batch]
        )

        last_error_reason, last_prompt_sent, llm_response_text = (
            "Unknown error",
            user_prompt,
            "",
        )

        for model_tier in ["primary", "fallback"]:
            model_to_use = (
                self.args.llm_model
                if model_tier == "primary"
                else self.args.llm_fallback_model
            )
            if not model_to_use:
                continue

            for attempt in range(self.args.max_api_retries):
                response, error_msg = self.helper._make_llm_api_call(
                    self.llm_client,
                    self.args.llm_provider,
                    self.system_prompt,
                    user_prompt,
                    model_to_use,
                    attempt + 1,
                    self.args.max_api_retries,
                    self.helper.DEFAULT_CLAUDE_MAX_TOKENS_OUTPUT,
                )
                llm_response_text = response or ""

                if response is not None:
                    parsed_data, errors = self.helper._parse_llm_response_blocks(
                        response, batch_ids
                    )
                    if not errors:
                        try:
                            with open(self.in_log_path, "a", encoding="utf-8") as f:
                                f.write(
                                    f"--- BATCH START ---\n{user_prompt}\n--- BATCH END ---\n\n"
                                )
                            with open(self.out_log_path, "a", encoding="utf-8") as f:
                                f.write(
                                    f"--- BATCH START ---\n{llm_response_text}\n--- BATCH END ---\n\n"
                                )
                        except IOError as e:
                            self.helper.logger.warning(
                                f"Could not write to .in/.out log files: {e}"
                            )

                        for item in batch:
                            block = self.genBlock(item, parsed_data)
                            self.book_data["content_blocks"].append(block)
                        return True  # Batch succeeded
                else:
                    last_error_reason = f"API Error with '{model_to_use}': {error_msg}"
                if attempt < self.args.max_api_retries - 1:
                    time.sleep(self.args.retry_delay)

            # If we exit all loops without success
            self.helper._write_error_debug_file(
                self.book_stem,
                str(self.stage_num),
                self.error_output_dir,
                f"Failing Batch starts at S_ID: {batch[0]['s_id']}\nBatch IDs: {', '.join(batch_ids)}",
                last_prompt_sent,
                llm_response_text,
                last_error_reason,
            )
            return False

    def run(self) -> bool:
        self.helper.logger.info(
            f"      Executing Stage {self.stage_num} for '{self.book_stem}'..."
        )
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        if self.args.force_book:
            if self.output_path.exists():
                os.remove(self.output_path)
            if self.in_log_path.exists():
                os.remove(self.in_log_path)
            if self.out_log_path.exists():
                os.remove(self.out_log_path)

        if not self.args.force_book and self.helper.is_stage_complete(
            self.book_stem, self.stage_num, self.llm_output_base_dir
        ):
            self.helper.logger.info(
                f"      Stage {self.stage_num} is already complete. Skipping."
            )
            return True

        input_path = self.helper.get_input_path_for_stage(
            self.book_stem, self.stage_num, self.staged_dir, self.llm_output_base_dir
        )
        if not input_path.exists():
            self.helper.logger.error(
                f"      Halting: Source file for Stage 1 not found at {input_path}"
            )
            return False

        raw_lines = input_path.read_text(encoding="utf-8").splitlines()
        all_source_items = []
        for idx, line in enumerate(raw_lines):
            if chapter_match := self.helper.CHAPTER_MARKER_REGEX.match(line.strip()):
                all_source_items.append(
                    {
                        "type": "chapter",
                        "text": chapter_match.group(1).strip(),
                        "source_index": idx,
                    }
                )
            elif sentence_match := self.helper.SENTENCE_LINE_REGEX.match(line.strip()):
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

        self.book_data = {
            "book_name": self.book_stem,
            "json_schema_version": "5.0",
            "processing_status": "PARTIAL",
            "content_blocks": [],
        }
        start_from_source_index = 0
        if not self.args.force_book and self.output_path.exists():
            try:
                with open(self.output_path, "r", encoding="utf-8") as f:
                    self.book_data = json.load(f)

                if self.book_data.get("content_blocks"):
                    last_processed_block = self.book_data.get("content_blocks", [])[-1]
                else:
                    last_processed_block = None

                if last_processed_block:
                    start_from_source_index = (
                        last_processed_block.get("source_index_in_original_file", -1)
                        + 1
                    )
                self.helper.logger.info(
                    f"      Resuming Stage {self.stage_num} from source item index {start_from_source_index}."
                )
            except (json.JSONDecodeError, IOError, IndexError) as e:
                self.helper.logger.warning(
                    f"      Could not read or parse existing Stage 1 file: {e}. Starting from scratch."
                )
                self.book_data["content_blocks"] = []
                start_from_source_index = 0

        self.system_prompt = self.helper._load_prompt_template(
            f"stage{self.stage_num}_prompt.txt"
        )
        if not self.system_prompt:
            return False

        items_to_process = all_source_items[start_from_source_index:]
        current_batch_items = []

        for item in items_to_process:
            if item["type"] == "sentence":
                current_batch_items.append(item)
                if len(current_batch_items) >= self.args.max_sentences_per_batch:
                    if not self.process_batch(current_batch_items):
                        return False
                    current_batch_items.clear()
                    # Save progress
                    self.book_data["processing_timestamp"] = self.helper.get_iso_timestamp()
                    try:
                        with open(self.output_path, "w", encoding="utf-8") as f:
                            json.dump(self.book_data, f, indent=2, ensure_ascii=False)
                    except IOError as e:
                        self.helper.logger.error(
                            f"      CRITICAL: Could not write progress to {self.output_path.name}: {e}"
                        )
                        return False

            elif item["type"] == "chapter":
                # Process any pending sentence batch before the chapter
                if not self.process_batch(current_batch_items):
                    return False
                current_batch_items.clear()

                # Process the chapter itself
                self.helper.logger.info(f"      Adding Chapter Marker: {item['text']}")
                self.book_data["content_blocks"].append(
                    {
                        "block_type": "chapter_marker",
                        "marker_text": item["text"],
                        "source_index_in_original_file": item["source_index"],
                    }
                )

        # Process any final batch that didn't reach the max size
        if not self.process_batch(current_batch_items):
            return False

        self.book_data["processing_status"] = "COMPLETED"
        self.book_data["processing_timestamp"] = self.helper.get_iso_timestamp()
        try:
            with open(self.output_path, "w", encoding="utf-8") as f:
                json.dump(self.book_data, f, indent=2, ensure_ascii=False)
            self.helper.logger.info(
                f"      Successfully wrote final Stage {self.stage_num} output to {self.output_path.name}"
            )
            return True
        except IOError as e:
            self.helper.logger.error(
                f"      Could not write final output to {self.output_path.name}: {e}"
            )
            return False
