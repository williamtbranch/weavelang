# llm2books/stages/generate_basic_base.py
import re
from typing import Any, Dict, List
import json

from .base import LLMStage, logger
from .. import llm_prompts

# --- Configuration for the Human-in-the-Loop workflow ---
HUMAN_REVIEW_DIR_NAME = "human_review"
HUMAN_REVIEW_MARKER = "%%HUMAN_REVIEW_APPROVED%%"


class GenerateBasicBase(LLMStage):
    """
    Stage 2 (V11): Simplifies the original literary `base` text to a "basic" but
    natural base language. Its output is a single, human-editable text file. The pipeline
    will pause after this stage until the user approves the file.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=2,
            stage_name="GenerateBasicBase"
        )
        self.human_review_dir = self.pipeline_run_dir / HUMAN_REVIEW_DIR_NAME
        # The final output is the review file, but the intermediate JSON is still saved in stage_output_dir
        self.review_file_path = self.human_review_dir / f"{self.book_stem}.basic_en.txt"
        self.parser_type = "single_line"

    def get_system_prompt(self) -> str:
        return llm_prompts.get_system_prompt("simplify_to_basic_english", self.resources["language_config"])

    def prepare_llm_items(self, book_data: Dict) -> List[Dict]:
        items_to_process = []
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                base_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "base"), None)
                if not base_tier: continue
                prompt_text = " ".join(base_tier.get("full_text", "").strip().split())
                if prompt_text:
                    items_to_process.append({ "id": block['s_id'], "text": prompt_text })
        return items_to_process

    def process_llm_results_for_block(self, block: Dict, llm_results: Dict[str, str]) -> Dict:
        s_id = block['s_id']
        if s_id in llm_results:
            block["_temp_basic_english"] = llm_results[s_id]
        block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return block

    def run(self) -> bool:
        # --- THIS IS THE FIX ---
        # 1. Ensure the directory exists BEFORE any file operations.
        self.human_review_dir.mkdir(parents=True, exist_ok=True)

        # 2. Run the LLM logic using the base class's `run` method.
        #    This will load Stage 1 data, process items, and save the results
        #    (including our `_temp_basic_english` key) to the Stage 2 JSON file.
        if not super().run():
            return False

        # 3. Now, load the data WE JUST SAVED from the current stage's output path.
        logger.info(f"      -> Assembling human review file: {self.review_file_path.name}")
        
        # We load from self.output_path (the stage's JSON), not the previous stage's.
        output_data = self._load_current_stage_output()
        if not output_data:
            logger.error("      -> Could not reload data after LLM processing to write final review file.")
            return False

        try:
            with open(self.review_file_path, 'w', encoding='utf-8') as f:
                f.write(f"# {HUMAN_REVIEW_MARKER}\n")
                f.write(f"# File: {self.review_file_path.name}\n")
                f.write("# Instructions: Review and edit the sentences below for clarity and naturalness.\n")
                f.write("# When finished, remove the '#' from the first line to approve.\n\n")

                #
                for block in output_data.get("content_blocks", []):
                    if block.get("block_type") == "sentence":
                        s_id = block['s_id']
                        simplified_text = block.get("_temp_basic_english", "").strip()
                        
                        # Find the original literary English text
                        base_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "base"), None)
                        original_text = base_tier.get("full_text", "ORIGINAL TEXT NOT FOUND") if base_tier else "ORIGINAL TIER NOT FOUND"

                        if simplified_text:
                            # --- THIS IS THE FIX ---
                            # Write the original text as a commented-out reference line
                            f.write(f"# ORIGINAL: {original_text}\n")
                            # Write the editable simplified text
                            f.write(f"{{{s_id}: {simplified_text}}}\n\n") # Added extra newline for readability
                        # Clean up the temporary key before the next stage
                        if "_temp_basic_english" in block:
                            del block["_temp_basic_english"]

        except IOError as e:
            logger.error(f"      -> CRITICAL: Failed to write human review file: {e}")
            return False
        
        # 4. Save the cleaned JSON data (without the temp key) back to the stage output.
        #    This ensures the next stage gets a clean input file.
        self._save_output_data(output_data, "COMPLETED")

        logger.info(f"      -> Successfully created review file. The pipeline will now pause.")
        return True

    def _load_current_stage_output(self):
        """Helper to load the JSON file this stage just created."""
        try:
            with open(self.output_path, 'r', encoding='utf-8') as f:
                return json.load(f)
        except (IOError, json.JSONDecodeError):
            return None