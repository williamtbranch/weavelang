# llm2books/stages/generate_advanced_target.py
import re
from typing import Any, Dict, List, Optional, Tuple
from .base import LLMStage, logger
from .. import llm_prompts

class GenerateAdvancedTarget(LLMStage):
    """
    Stage 1 (V2): Translates the source base language text into the advanced target language.
    This is the first stage and creates the initial V2 JSON file.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=1,
            stage_name="GenerateAdvancedTarget",
            parser_type='line'
        )

    def get_system_prompt(self) -> str:
        prompt_dir = self.resources["language_config"]["prompt_dir"]
        return llm_prompts.load_prompt_template(self.stage_name, prompt_dir)

    def prepare_atomic_unit(self, block: Dict[str, Any]) -> Tuple[Optional[List[Dict[str, Any]]], int]:
        """An atomic unit is a single sentence."""
        prompt_text = f"{block['llm_block_id']}: {block['base_text']}"
        token_estimate = self._estimate_tokens(prompt_text)
        prompt_parts = [{"llm_id": block["llm_block_id"], "prompt_text": prompt_text}]
        return prompt_parts, token_estimate

    def run(self) -> bool:
        """Overridden run method for the special first stage."""
        logger.info(f"Executing Stage 1: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)

        if self.cli_args.force_book == self.book_stem and self.output_path.exists():
            self.output_path.unlink()

        if not self.cli_args.force_book == self.book_stem and self._is_stage_complete():
            logger.info("      -> Stage is already marked as 'COMPLETED'. Skipping.")
            return True

        # --- Stage 1 reads from the raw .txt file, not a JSON ---
        source_items = self._load_and_parse_source_file()
        if source_items is None: return False

        # --- Initialize the V2 book data structure ---
        lang_config = self.resources["language_config"]
        book_data = {
            "book_meta": {
                "book_name": self.book_stem,
                "schema_version": "2.0",
                "base_language": lang_config["base_code"],
                "target_language": lang_config["target_code"]
            },
            "content_blocks": []
        }

        # --- Process in batches ---
        batch_token_limit = self.stage_config.get("batch_size_in_tokens", 8000)
        current_batch_items = []
        current_batch_token_count = 0
        
        for item in source_items:
            if item["type"] == "sentence":
                prompt_parts, token_estimate = self.prepare_atomic_unit(item)
                if current_batch_items and (current_batch_token_count + token_estimate > batch_token_limit):
                    if not self._process_batch_stage1(current_batch_items, book_data): return False
                    current_batch_items, current_batch_token_count = [item], token_estimate
                else:
                    current_batch_items.append(item)
                    current_batch_token_count += token_estimate
            elif item["type"] == "chapter":
                if current_batch_items:
                    if not self._process_batch_stage1(current_batch_items, book_data): return False
                    current_batch_items, current_batch_token_count = [], 0
                book_data["content_blocks"].append({
                    "block_type": "chapter_marker", "marker_text": item["text"]
                })

        if current_batch_items:
            if not self._process_batch_stage1(current_batch_items, book_data): return False

        logger.info("      -> Finalizing Stage 1 output.")
        return self._save_output_data(book_data, "COMPLETED")

    def _load_and_parse_source_file(self) -> Optional[List[Dict[str, Any]]]:
        """Reads the staged .txt file."""
        input_path = self.staged_dir / f"{self.book_stem}.txt"
        if not input_path.exists():
            logger.critical(f"CRITICAL: Source file for Stage 1 not found: {input_path}")
            return None
        
        raw_lines = input_path.read_text(encoding="utf-8").splitlines()
        all_items, sentence_regex, chapter_regex = [], re.compile(r"^{S(\d+):\s*(.*)}$"), re.compile(r"^%%CHAPTER_MARKER%%\s*(.*)$")
        
        for line in raw_lines:
            if chapter_match := chapter_regex.match(line.strip()):
                all_items.append({"type": "chapter", "text": chapter_match.group(1).strip()})
            elif sentence_match := sentence_regex.match(line.strip()):
                s_id_val = int(sentence_match.group(1))
                all_items.append({
                    "type": "sentence", "s_id": f"S{s_id_val}",
                    "llm_block_id": f"id {s_id_val}",
                    "base_text": sentence_match.group(2).strip(),
                })
        return all_items

    def _process_batch_stage1(self, batch_items: List[Dict[str, Any]], book_data: Dict[str, Any]) -> bool:
        """Processes a batch and appends the results to the main book_data object."""
        self.batch_counter += 1
        logger.info(f"      -> Processing batch #{self.batch_counter} with {len(batch_items)} sentences...")
        
        prompt_parts = [f"{item['llm_block_id']}: {item['base_text']}" for item in batch_items]
        expected_ids = [item['llm_block_id'] for item in batch_items]
        user_prompt = "\n".join(prompt_parts)

        self._write_batch_header_to_log(user_prompt)
        parsed_data = self._make_api_call_with_retries(user_prompt, expected_ids)

        if parsed_data is None:
            return False

        # Append new sentence blocks to book_data
        for item in batch_items:
            llm_id_lower = item["llm_block_id"].lower()
            adv_target_text = parsed_data.get(llm_id_lower, "")

            v2_block = {
                "block_type": "sentence",
                "s_id": item["s_id"],
                "tiers": [
                    {"tier_id": "base", "full_text": item["base_text"], "segments": []},
                    {"tier_id": "advanced_target", "full_text": adv_target_text, "segments": []}
                ],
                "mappings": {}
            }
            book_data["content_blocks"].append(v2_block)
        
        return self._save_output_data(book_data, "PARTIAL")