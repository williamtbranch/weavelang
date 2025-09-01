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
            stage_number=9,
            stage_name="FinalizeBook"
        )
        # Define the final output path in the library directory
        self.library_dir = self.content_project_root / "library"
        self.final_output_path = self.library_dir / f"{self.book_stem}.json"

    #
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

        logger.info("      -> Running final data integrity validations...")
        try:
            for block in book_data.get("content_blocks", []):
                if block.get("block_type") == "sentence":
                    #
                    # ============================ START: REPLACEMENT CODE ============================
                    # The simple_target_to_base_diglot map no longer exists,
                    # so we remove its validation call. The inverse map validation remains.
                    # validator.validate_exhaustive_diglot_mapping(block) <- DELETE THIS LINE
                    validator.validate_exhaustive_inverse_diglot_mapping(block)
                    # ============================= END: REPLACEMENT CODE =============================
                    for tier in block.get("tiers", []):
                        validator.validate_segment_reconstruction(tier)
                        for seg in tier.get("segments", []):
                            reconstructed = "".join(t['v'] for t in seg.get("tokenized_text", []))
                            if reconstructed != seg.get("text"):
                                raise validator.ValidationError(f"Token reconstruction for s_id {block['s_id']} seg_id {seg['seg_id']} failed.")
            logger.info("      -> All validation checks passed.")
        except validator.ValidationError as e:
            logger.error(f"      -> CRITICAL: Final data validation failed for book '{self.book_stem}'.")
            logger.error(f"         Reason: {e}")
            return False

        # --- NEW: FINAL CLEANUP STEP ---
        logger.info("      -> Cleaning up intermediate data for final output...")
        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                # The golden_token_stream is no longer needed by the Rust consumer
                if "golden_token_stream" in block:
                    del block["golden_token_stream"]
                
                # Remove temporary keys from tokens that Rust doesn't need
                for tier in block.get("tiers", []):
                    for seg in tier.get("segments", []):
                        for token in seg.get("tokenized_text", []):
                            if "seg_idx" in token:
                                del token["seg_idx"]
        # --- END OF CLEANUP ---

        book_data.get("book_meta", {})["schema_version"] = "3.0"
        
        try:
            with open(self.final_output_path, 'w', encoding='utf-8') as f:
                json.dump(book_data, f, indent=2, ensure_ascii=False)
            logger.info(f"      -> Successfully saved final, cleaned output to '{self.final_output_path}'")
            return True
        except IOError as e:
            logger.error(f"Failed to write final library file: {e}")
            return False

    def _get_input_path(self):
        # This stage reads from the previous stage (Stage 5)
        prev_stage_num = self.stage_number - 1
        prev_stage_dir = self.pipeline_run_dir / f"stage{prev_stage_num}"
        return prev_stage_dir / f"{self.book_stem}.stage{prev_stage_num}.json"