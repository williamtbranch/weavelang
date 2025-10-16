# llm2books/stages/apply_inverse_phrase_mappings.py
import re
from typing import Any, Dict, List

from .base import SpaCyStage, logger
from ..phrase_mapper_helpers import refactor_token_stream
from .. import validator
from .. import semantic_validator 

class ApplyInversePhraseMappings(SpaCyStage):
    """
    Stage 6: Processes the raw inverse phrase map from Stage 5.
    - Uses refactor_token_stream to validate the LLM's groupings and fuse the simple_target tokens.
    - Creates the final, correctly structured simple_target_to_base_inv_diglot map.
    - Performs semantic validation on the newly created inverse mappings.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=6,
            stage_name="ApplyInversePhraseMappings"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        for block in data.get("content_blocks", []):
            if block.get("block_type") != "sentence":
                continue

            s_id = block['s_id']
            try:
                mappings = block.get("mappings", {})
                raw_map_data = mappings.get("raw_simple_to_base_inv_diglot_map", {})
                simple_target_tier = next((t for t in block["tiers"] if t["tier_id"] == "simple_target"), None)

                if not simple_target_tier or not raw_map_data:
                    block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                    continue
                
                new_inv_diglot_map = {}

                for seg in simple_target_tier.get("segments", []):
                    seg_id = seg["seg_id"]
                    original_tokens = seg.get("tokenized_text", [])
                    raw_map_lines = raw_map_data.get(seg_id, [])
                    
                    if not original_tokens or not raw_map_lines:
                        new_inv_diglot_map[seg_id] = []
                        continue

                    # 1. Parse LLM output for this segment
                    llm_groups = []
                    llm_map_by_group = {}
                    for line in raw_map_lines:
                        if '->' in line:
                            parts = line.split('->', 1)
                            if len(parts) == 2 and parts[0].strip():
                                group_str = parts[0].strip()
                                llm_groups.append(group_str)
                                llm_map_by_group[group_str] = parts[1].strip()
                    
                    # 2. Refactor token stream (this also validates the LLM's left-side groupings)
                    new_tokens_for_seg = refactor_token_stream(original_tokens, llm_groups)
                    seg["tokenized_text"] = new_tokens_for_seg

                    # 3. Build the new inverse diglot map for this segment
                    map_entries_for_seg = []
                    word_token_idx = 0
                    for token in new_tokens_for_seg:
                        if token['t'] == 'w':
                            group_str = token['v']
                            eng_substitute = llm_map_by_group.get(group_str, "NO_SUB")
                            eng_word_count = len(re.findall(r"[\w']+", eng_substitute))
                            
                            # The map format is [v_token_idx, lemmas (TBD), eng_sub, eng_word_count]
                            map_entries_for_seg.append([word_token_idx, "TBD", eng_substitute, eng_word_count])
                            word_token_idx += 1
                    
                    new_inv_diglot_map[seg_id] = map_entries_for_seg
                
                # Reconstruct full_text for the tier after all its segments have been refactored
                simple_target_tier["full_text"] = "".join(
                    "".join(t['v'] for t in seg["tokenized_text"]) for seg in simple_target_tier["segments"]
                )

                # Store the newly created map
                block["mappings"]["simple_target_to_base_inv_diglot"] = new_inv_diglot_map
                
                # Temporarily attach for validation
                is_semantically_valid = semantic_validator.validate_inverse_mappings(block)
                if not is_semantically_valid:
                    raise validator.ValidationError(f"S_ID {s_id} failed INVERSE semantic validation.")
                
                # If validation passes, mark as complete and clean up
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                if "raw_simple_to_base_inv_diglot_map" in mappings: 
                    del mappings["raw_simple_to_base_inv_diglot_map"]

            except (validator.ValidationError, KeyError, IndexError) as e:
                logger.error(f"Halting due to data integrity/validation error in {self.stage_name} for S_ID {s_id}: {e}")
                block.setdefault("processing_status", {})[self.stage_name] = f"RETRY_FAIL: {e}"
                self._save_output_data(data, "PARTIAL_FAILED")
                raise

        return data