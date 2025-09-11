# In llm2books/stages/assemble_tiers.py

import json
from pathlib import Path
from typing import Any, Dict, Optional

# We can import from Stage directly, as we're overriding run()
from .base import Stage, logger

class AssembleTiers(Stage): # Inherit from the simpler Stage, not SpaCyStage
    """
    Stage 1: Assembles the reusable data from the Common Pool into a
    single, unified JSON file for the pipeline run. This stage has a custom
    run method to handle multiple input files.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=1,
            stage_name="AssembleTiers"
        )

    def run(self) -> bool:
        logger.info(f"Executing Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        
        # Check for resumability
        if self.output_path.exists():
            logger.info("      -> Stage is already complete. Skipping.")
            return True

        # Custom input logic for this stage
        pool_paths = self.resources.get('book_resources')
        if not pool_paths:
            logger.error("AssembleTiers stage did not receive the required pool file paths.")
            return False

        output_data = self._process_data(pool_paths)
        if output_data is None:
            return False
        
        # The base Stage class provides a standard save method
        if self._save_output_data(output_data, "COMPLETED"):
            logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
            return True
        else:
            return False

    def _process_data(self, pool_paths: Dict[str, Path]) -> Optional[Dict[str, Any]]:
        try:
            with open(pool_paths["base_std"], 'r', encoding='utf-8') as f:
                base_std_data = json.load(f)
            with open(pool_paths["target_std"], 'r', encoding='utf-8') as f:
                target_std_data = json.load(f)
            # --- NEW: Load the moderate and basic tiers ---
            with open(pool_paths["target_mod"], 'r', encoding='utf-8') as f:
                mod_data = json.load(f)
            with open(pool_paths["target_bas"], 'r', encoding='utf-8') as f:
                bas_data = json.load(f)
            with open(pool_paths["target_sim"], 'r', encoding='utf-8') as f:
                sim_data = json.load(f)
        except (IOError, json.JSONDecodeError, KeyError) as e:
            logger.error(f"Failed to read or parse one or more pool files: {e}")
            return None

        base_content_map = { item['s_id']: item for item in base_std_data.get('content', []) if item.get("block_type") == "sentence" }
        target_content_map = { item['s_id']: item for item in target_std_data.get('content', []) if item.get("block_type") == "sentence" }
        # --- NEW: Create maps for the new tiers ---
        mod_content_map = { item['s_id']: item for item in mod_data.get('content', []) if item.get("block_type") == "sentence" }
        bas_content_map = { item['s_id']: item for item in bas_data.get('content', []) if item.get("block_type") == "sentence" }
        sim_content_map = { item['s_id']: item for item in sim_data.get('content', []) if item.get("block_type") == "sentence" }

        book_data = {
            "book_meta": {
                "book_name": self.book_stem, "schema_version": "3.0-wip",
                "base_language": self.resources['language_config']['base_code'],
                "target_language": self.resources['language_config']['target_code'],
            },
            "content_blocks": []
        }

        for block in base_std_data.get('content', []):
            if block.get("block_type") == "chapter":
                book_data["content_blocks"].append(block)
                continue
            
            if block.get("block_type") == "sentence":
                s_id = block.get('s_id')
                target_sentence = target_content_map.get(s_id)
                # --- NEW: Get content for new tiers ---
                mod_sentence = mod_content_map.get(s_id)
                bas_sentence = bas_content_map.get(s_id)
                sim_sentence = sim_content_map.get(s_id)

                if not all([target_sentence, mod_sentence, bas_sentence, sim_sentence]):
                    logger.warning(f"Skipping s_id {s_id}: Missing corresponding data in one or more pool files.")
                    continue

                pipeline_block = {
                    "block_type": "sentence", "s_id": s_id, "processing_status": {},
                    # --- NEW: Assemble all five tiers in the correct order ---
                    "tiers": [
                        { "tier_id": "base", **block },
                        { "tier_id": "advanced_target", **target_sentence },
                        { "tier_id": "moderate_target", **mod_sentence },
                        { "tier_id": "basic_target", **bas_sentence },
                        { "tier_id": "simple_target", **sim_sentence }
                    ],
                    "mappings": {}
                }
                
                # This logic correctly removes the redundant keys from the tier data
                for tier in pipeline_block['tiers']:
                    tier.pop('block_type', None)
                    tier.pop('s_id', None)

                book_data["content_blocks"].append(pipeline_block)
        
        return book_data