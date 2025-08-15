# llm2books/stages/finalize_book.py
import json
import shutil
from typing import Any, Dict
from .base import Stage, logger
from .. import validator

class FinalizeBook(Stage):
    """
    Stage 6: Performs final validation on the completed JSON data structure
    and copies the final output to the 'library' directory for consumption
    by the Rust engine.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=6,
            stage_name="FinalizeBook"
        )
        # Define the final output path in the library directory
        self.library_dir = self.content_project_root / "library"
        self.final_output_path = self.library_dir / f"{self.book_stem}.json"

    def run(self) -> bool:
        logger.info(f"Executing Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.library_dir.mkdir(parents=True, exist_ok=True)
        
        input_path = self._get_input_path()
        try:
            with open(input_path, 'r', encoding='utf-8') as f:
                book_data = json.load(f)
        except (IOError, json.JSONDecodeError) as e:
            logger.error(f"Could not read or parse input file {input_path.name}: {e}")
            return False

        # --- FINAL VALIDATION STEP ---
        logger.info("      -> Running final data integrity validations...")
        try:
            for block in book_data.get("content_blocks", []):
                if block.get("block_type") == "sentence":
                    # We can add more validation checks here from validator.py as needed
                    validator.validate_exhaustive_diglot_mapping(block)
                    validator.validate_exhaustive_inverse_diglot_mapping(block)
                    for tier in block.get("tiers", []):
                        validator.validate_full_text_reconstruction(tier)

            logger.info("      -> All validation checks passed.")
        except validator.ValidationError as e:
            logger.error(f"      -> CRITICAL: Final data validation failed for book '{self.book_stem}'.")
            logger.error(f"         Reason: {e}")
            return False

        # --- CLEANUP & FINAL SAVE ---
        # (No cleanup needed for now, but this is where it would go)
        
        # Change schema version to final release version
        book_data.get("book_meta", {})["schema_version"] = "3.0"
        book_data["processing_status"] = "PIPELINE_COMPLETE"
        
        try:
            # Copy the final, validated file to the library directory
            with open(self.final_output_path, 'w', encoding='utf-8') as f:
                json.dump(book_data, f, indent=2, ensure_ascii=False)
            logger.info(f"      -> Successfully saved final output to '{self.final_output_path}'")
            return True
        except IOError as e:
            logger.error(f"Failed to write final library file: {e}")
            return False

    def _get_input_path(self):
        # This stage reads from the previous stage (Stage 5)
        prev_stage_num = self.stage_number - 1
        prev_stage_dir = self.pipeline_run_dir / f"stage{prev_stage_num}"
        return prev_stage_dir / f"{self.book_stem}.stage{prev_stage_num}.json"