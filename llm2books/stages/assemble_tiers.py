import json
from pathlib import Path
from typing import Any, Dict, Optional

from .base import SpaCyStage, logger

class AssembleTiers(SpaCyStage):
    """
    The new Stage 1: Assembles the reusable, pre-processed data from the
    Common Pool into a single, unified JSON file for the pipeline run.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=1,
            stage_name="AssembleTiers"
        )

    def _get_input_path(self) -> Path:
        # This stage is special; it doesn't have a single input path.
        # It gets its input paths from the 'book_resources' passed by the orchestrator.
        return None

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        # The 'data' argument will be the dictionary of paths from the PoolManager
        pool_paths = data

        try:
            with open(pool_paths["base_std"], 'r', encoding='utf-8') as f:
                base_std_data = json.load(f)
            with open(pool_paths["target_std"], 'r', encoding='utf-8') as f:
                target_std_data = json.load(f)
            with open(pool_paths["target_sim"], 'r', encoding='utf-8') as f:
                target_sim_data = json.load(f)
        except (IOError, json.JSONDecodeError) as e:
            logger.error(f"Failed to read or parse pool files: {e}")
            # This should ideally raise an exception to halt the pipeline
            return None

        # Create maps for easy lookup
        base_content_map = {
            item['s_id']: item for item in base_std_data.get('content', [])
            if item.get("block_type") == "sentence"
        }
        target_content_map = {
            item['s_id']: item for item in target_std_data.get('content', [])
            if item.get("block_type") == "sentence"
        }
        sim_content_map = {
            item['s_id']: item for item in target_sim_data.get('content', []) # <-- CORRECT
            if item.get("block_type") == "sentence"
        }

        # Assemble the new book data structure
        book_data = {
            "book_meta": {
                "book_name": self.book_stem,
                "schema_version": "3.0-wip",
                "base_language": self.resources['language_config']['base_code'],
                "target_language": self.resources['language_config']['target_code'],
            },
            "content_blocks": []
        }

        # We loop through the base content as the source of truth for sentence order
        for block in base_std_data.get('content', []):
            if block.get("block_type") == "chapter":
                # Pass chapter blocks through directly
                book_data["content_blocks"].append(block)
                continue
            
            # If it's not a chapter, it must be a sentence
            if block.get("block_type") == "sentence":
                s_id = block.get('s_id')
                target_sentence = target_content_map.get(s_id)
                sim_sentence = sim_content_map.get(s_id)

                if not target_sentence or not sim_sentence:
                    logger.warning(f"Skipping s_id {s_id}: Missing corresponding data in target or sim files.")
                    continue

                # Assemble the final pipeline block
                pipeline_block = {
                    "block_type": "sentence",
                    "s_id": s_id,
                    "processing_status": {}, # Start with an empty status
                    "tiers": [
                        # The 'base' tier comes directly from the processed base_std block
                        {
                            "tier_id": "base",
                            "full_text": block.get('full_text', ''),
                            "segments": block.get('segments', [])
                        },
                        {
                            "tier_id": "advanced_target",
                            "full_text": target_sentence.get('full_text', ''),
                            "lemmas": target_sentence.get('lemmas', []),
                            "segments": target_sentence.get('segments', [])
                        },
                        {
                            "tier_id": "simpler_advanced_target",
                            "full_text": sim_sentence.get('full_text', ''),
                            "lemmas": sim_sentence.get('lemmas', []),
                            "segments": sim_sentence.get('segments', [])
                        }
                    ],
                    "mappings": {
                        "simpler_adv_target_to_base_inv_diglot": sim_sentence.get('inverse_diglot_map', {})
                    }
                }
                book_data["content_blocks"].append(pipeline_block)
        # --- END OF LOOP FIX ---
        
        return book_data

    def run(self) -> bool:
        logger.info(f"Executing Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        
        # We will add resumability checks later
        
        # 'book_resources' comes from the orchestrator
        input_paths = self.resources.get('book_resources')
        if not input_paths:
            logger.error("AssembleTiers stage did not receive the required pool file paths.")
            return False

        output_data = self._process_data(input_paths)
        if output_data is None:
            return False

        if self._save_output_data(output_data, "COMPLETED"):
            logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
            return True
        else:
            return False