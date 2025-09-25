# llm2books/stages/finalize_book.py
import json
import shutil
from typing import Any, Dict
from .base import Stage, logger
from .. import validator

class FinalizeBook(Stage):
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=9,
            stage_name="FinalizeBook"
        )
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

        logger.info("      -> Running final data integrity validations...")
        try:
            for block in book_data.get("content_blocks", []):
                if block.get("block_type") == "sentence":
                    validator.validate_exhaustive_diglot_mapping(block)
                    validator.validate_exhaustive_inverse_diglot_mapping(block)
                    
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

        logger.info("      -> Cleaning up intermediate data for final output...")
        tiers_to_strip_tokenized_text = [
            "advanced_target",
            "moderate_target",
            "basic_target",
        ]

        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                if "golden_token_stream" in block:
                    del block["golden_token_stream"]
                
                #
                for tier in block.get("tiers", []):
                    for seg in tier.get("segments", []):
                        # 1. Universally rebuild the 'text' field from tokens.
                        #    This synchronizes the data just before validation.
                        if "tokenized_text" in seg:
                            seg["text"] = "".join(t.get("v", "") for t in seg["tokenized_text"])

                # This second loop now ONLY handles token stripping and key cleanup.
                for tier in block.get("tiers", []):
                    # 2. Conditionally strip tokens from the higher tiers.
                    if tier["tier_id"] in tiers_to_strip_tokenized_text:
                        for seg in tier.get("segments", []):
                            if "tokenized_text" in seg:
                                del seg["tokenized_text"]
                    
                    # 3. Universally clean up temporary keys from tokens.
                    for seg in tier.get("segments", []):
                        if "tokenized_text" in seg: # Check again in case it wasn't stripped
                            for token in seg.get("tokenized_text", []):
                                if "seg_idx" in token:
                                    del token["seg_idx"]
        
        book_data.get("book_meta", {})["schema_version"] = "3.0"
        
        try:
            with open(self.final_output_path, 'w', encoding='utf-8') as f:
                json.dump(book_data, f, indent=2, ensure_ascii=False)
            logger.info(f"      -> Successfully saved final, cleaned output to '{self.final_output_path}'")
            return True
        except IOError as e:
            logger.error(f"Failed to write final library file: {e}")
            return False