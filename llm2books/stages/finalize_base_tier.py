# In llm2books/stages/finalize_base_tier.py

from typing import Any, Dict
from .base import Stage, logger # This is a simple structural stage

class FinalizeBaseTier(Stage):
    """
    Stage 8: A crucial stage that flattens the 'base' tier's segments AND
    its corresponding diglot map into a single structure, ensuring consistency
    for the final validation and the Rust engine.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=8,
            stage_name="FinalizeBaseTier"
        )

    def run(self) -> bool:
        logger.info(f"Executing Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        
        if self.output_path.exists():
            logger.info("      -> Stage is already complete. Skipping.")
            return True

        input_data = self._load_input_data()
        if input_data is None:
            return False

        for block in input_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                base_tier = next((t for t in block.get("tiers", []) if t.get("tier_id") == "base"), None)
                if not base_tier or not base_tier.get("segments") or len(base_tier["segments"]) <= 1:
                    continue

                # --- THIS IS THE FIX ---
                # 1. Gather all tokens and all map entries from the old, multi-segment structure.
                all_tokens = []
                all_map_entries = []
                diglot_map = block.get("mappings", {}).get("simple_target_to_base_diglot", {})

                for segment in base_tier.get("segments", []):
                    all_tokens.extend(segment.get("tokenized_text", []))
                    # Get the map entries for this specific segment
                    map_entries_for_seg = diglot_map.get(segment.get("seg_id"), [])
                    all_map_entries.extend(map_entries_for_seg)
                
                # 2. Reconstruct the flattened tier structure.
                new_text = "".join(token.get("v", "") for token in all_tokens)
                new_segment = {
                    "seg_id": "S1", # The ID is now simple and singular
                    "tokenized_text": all_tokens,
                    "text": new_text
                }
                base_tier["segments"] = [new_segment]
                base_tier["full_text"] = new_text

                # 3. Reconstruct the flattened map structure.
                # The new map has only one key, "S1", containing all entries.
                new_diglot_map = {"S1": all_map_entries}
                block["mappings"]["simple_target_to_base_diglot"] = new_diglot_map
                # --- END OF FIX ---

        if self._save_output_data(input_data, "COMPLETED"):
            logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
            return True
        else:
            return False