# llm2books/stages/assemble_tiers.py
import json
from pathlib import Path
from typing import Any, Dict, Optional

from .base import Stage, logger

class AssembleTiers(Stage):
    """
    Stage 1 (V11): Assembles the foundational .std.json files from the Common Pool
    into a single, unified JSON object for the pipeline run. It creates the initial
    `base` and `advanced_target` tiers and sets up placeholders for the other tiers.
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
        
        if self.output_path.exists():
            logger.info("      -> Stage is already complete. Skipping.")
            return True

        pool_paths = self.resources.get('book_resources')
        if not pool_paths or "base_std" not in pool_paths or "target_std" not in pool_paths:
            logger.error("AssembleTiers stage did not receive the required pool file paths.")
            return False

        output_data = self._process_data(pool_paths)
        if output_data is None:
            return False
        
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
            with open(pool_paths["target_mod"], 'r', encoding='utf-8') as f:
                target_mod_data = json.load(f)
        except (IOError, json.JSONDecodeError, KeyError) as e:
            logger.error(f"Failed to read or parse one or more pool files: {e}")
            return None

        # Create maps for easy lookup of sentences by s_id
        target_content_map = { item['s_id']: item for item in target_std_data.get('content', []) if item.get("block_type") == "sentence" }
        mod_content_map = { item['s_id']: item for item in target_mod_data.get('content', []) if item.get("block_type") == "sentence" }

        book_data = {
            "book_meta": {
                "book_name": self.book_stem, "schema_version": "v11.1-wip",
                "base_language": self.resources['language_config']['base_code'],
                "target_language": self.resources['language_config']['target_code'],
            },
            "content_blocks": []
        }

        # Use the base language file as the structural source of truth.
        for block in base_std_data.get('content', []):
            if block.get("block_type") == "chapter":
                book_data["content_blocks"].append(block)
                continue
            
            if block.get("block_type") == "sentence":
                s_id = block.get('s_id')
                if not s_id:
                    continue

                # Find the corresponding sentences in the other files using the maps.
                target_sentence = target_content_map.get(s_id)
                mod_sentence = mod_content_map.get(s_id)

                if not target_sentence or not mod_sentence:
                    logger.warning(f"Skipping s_id {s_id}: Missing corresponding sentence in target or moderate .json file.")
                    continue

                # Assemble the tiers from the loaded data.
                base_tier = {"tier_id": "base", **block}
                adv_target_tier = {"tier_id": "advanced_target", **target_sentence}
                mod_target_tier = {"tier_id": "moderate_target", **mod_sentence}

                pipeline_block = {
                    "block_type": "sentence", "s_id": s_id, "processing_status": {},
                    "tiers": [base_tier, adv_target_tier, mod_target_tier],
                    "mappings": {}
                }
                
                # Clean up redundant keys from the copied block data.
                for tier in pipeline_block['tiers']:
                    tier.pop('block_type', None)
                    tier.pop('s_id', None)

                book_data["content_blocks"].append(pipeline_block)
        
        return book_data