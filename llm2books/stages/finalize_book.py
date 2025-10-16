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

        logger.info("      -> Cleaning up intermediate data for final output...")
        tiers_to_strip_tokenized_text = [
            "advanced_target",
            "moderate_target",
            "basic_target",
        ]
        target_language_tiers = tiers_to_strip_tokenized_text + ["simple_target"]

        for block in book_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                
                # --- START: NEW PROPER NOUN CLEANUP LOGIC ---
                proper_noun_lemmas_to_remove = set(block.get("_internal_proper_noun_lemmas", []))
                
                if proper_noun_lemmas_to_remove:
                    logger.debug(f"S_ID {block['s_id']}: Removing proper noun lemmas: {proper_noun_lemmas_to_remove}")
                    
                    # 1. Clean from all target language tiers
                    for tier_id in target_language_tiers:
                        tier = next((t for t in block["tiers"] if t["tier_id"] == tier_id), None)
                        if not tier: continue
                        
                        tier["lemmas"] = [l for l in tier.get("lemmas", []) if l not in proper_noun_lemmas_to_remove]
                        for seg in tier.get("segments", []):
                            seg["lemmas"] = [l for l in seg.get("lemmas", []) if l not in proper_noun_lemmas_to_remove]
                            for token in seg.get("tokenized_text", []):
                                if "l" in token:
                                    token["l"] = [l for l in token.get("l", []) if l not in proper_noun_lemmas_to_remove]

                    # 2. Clean from forward diglot map
                    diglot_map = block.get("mappings", {}).get("simple_target_to_base_diglot", {})
                    for seg_id, entries in diglot_map.items():
                        for entry in entries:
                            entry[1] = [l for l in entry[1] if l not in proper_noun_lemmas_to_remove]
                    
                    # 3. Clean from inverse diglot map
                    inv_diglot_map = block.get("mappings", {}).get("simple_target_to_base_inv_diglot", {})
                    for seg_id, entries in inv_diglot_map.items():
                        for entry in entries:
                            entry[1] = [l for l in entry[1] if l not in proper_noun_lemmas_to_remove]
                
                # 4. Delete the temporary key
                if "_internal_proper_noun_lemmas" in block:
                    del block["_internal_proper_noun_lemmas"]
                # --- END: NEW PROPER NOUN CLEANUP LOGIC ---

                # Strip temporary/unneeded keys and data for final output
                for tier in block.get("tiers", []):
                    # Reconstruct text fields from tokens one last time to ensure sync
                    for seg in tier.get("segments", []):
                        if "tokenized_text" in seg:
                            seg["text"] = "".join(t.get("v", "") for t in seg["tokenized_text"])
                    
                    tier["full_text"] = "".join(seg.get("text", "") for seg in tier.get("segments", []))

                    # Conditionally strip tokenized_text from higher tiers
                    if tier["tier_id"] in tiers_to_strip_tokenized_text:
                        for seg in tier.get("segments", []):
                            if "tokenized_text" in seg:
                                del seg["tokenized_text"]

        # Final schema version bump
        book_data.get("book_meta", {})["schema_version"] = "3.1"
        
        logger.info("      -> Running final data integrity validations...")
        try:
            for block in book_data.get("content_blocks", []):
                if block.get("block_type") == "sentence":
                    validator.validate_exhaustive_diglot_mapping(block)
                    validator.validate_exhaustive_inverse_diglot_mapping(block)
                    for tier in block.get("tiers", []):
                        validator.validate_segment_reconstruction(tier)
            logger.info("      -> All validation checks passed.")
        except validator.ValidationError as e:
            logger.error(f"      -> CRITICAL: Final data validation failed for book '{self.book_stem}'.")
            logger.error(f"         Reason: {e}")
            return False
        
        try:
            with open(self.final_output_path, 'w', encoding='utf-8') as f:
                json.dump(book_data, f, indent=2, ensure_ascii=False)
            logger.info(f"      -> Successfully saved final, cleaned output to '{self.final_output_path}'")
            return True
        except IOError as e:
            logger.error(f"Failed to write final library file: {e}")
            return False