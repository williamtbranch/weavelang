# In llm2books/stages/finalize_base_tier.py

from typing import Any, Dict
from .base import Stage, logger # This is a simple structural stage

class FinalizeBaseTier(Stage):
    """
    A simple stage to flatten the 'base' tier's segments into a single
    segment, making it easier for the Rust engine's holistic L1 logic to process.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=8, # This will be the new stage 8
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
                if not base_tier or not base_tier.get("segments"):
                    continue

                # Don't process if it's already flattened
                if len(base_tier["segments"]) == 1:
                    continue

                all_tokens = []
                for segment in base_tier.get("segments", []):
                    all_tokens.extend(segment.get("tokenized_text", []))
                
                # Reconstruct text from the unified token stream
                new_text = "".join(token.get("v", "") for token in all_tokens)

                # Create the new single segment
                new_segment = {
                    "seg_id": "S1", # The ID is now simple and singular
                    "tokenized_text": all_tokens,
                    "text": new_text
                }

                # Replace the old segments list with the new one
                base_tier["segments"] = [new_segment]
                # The full_text of the tier should already match, but we'll ensure it does
                base_tier["full_text"] = new_text

        if self._save_output_data(input_data, "COMPLETED"):
            logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
            return True
        else:
            return False