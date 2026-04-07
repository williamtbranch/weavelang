# llm2books/stages/apply_inverse_phrase_mappings.py
import re
from typing import Any, Dict, List

from .base import Stage, logger
from ..phrase_mapper_helpers import refactor_token_stream
from ..llm_utils import fix_possessive_splits_in_lines
from .. import validator

# --- Configuration for the Human-in-the-Loop workflow ---
HUMAN_REVIEW_DIR_NAME = "human_review"
HUMAN_REVIEW_MARKER = "%%HUMAN_REVIEW_APPROVED%%"


class ApplyInversePhraseMappings(Stage):
    """
    Stage 7 (V11): Consumes the human-approved `*.invdig.txt` file. It validates
    the user's edits, refactors the `basic_target` tier tokens, and builds the
    final `basic_target_to_basic_base_inv_diglot` map in the JSON.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=7,
            stage_name="ApplyInversePhraseMappings"
        )
        self.human_review_dir = self.pipeline_run_dir / HUMAN_REVIEW_DIR_NAME
        self.input_map_path = self.human_review_dir / f"{self.book_stem}.invdig.txt"

    def _load_and_validate_approved_map(self, input_data: Dict[str, Any]) -> Dict[str, List[str]]:
        """
        Gate 2 Validator: Loads the approved inverse mapping file and validates
        its entire content for completeness and structural integrity.
        """
        logger.info(f"      -> Loading and validating approved map: {self.input_map_path.name}")
        
        try:
            with open(self.input_map_path, 'r', encoding='utf-8') as f:
                content = f.read()
            if not content.strip().startswith(HUMAN_REVIEW_MARKER):
                raise validator.ValidationError("File is not approved. The approval marker is missing or commented out.")
        except (IOError, FileNotFoundError) as e:
            raise validator.ValidationError(f"Cannot read or find approved mapping file: {e}")

        # Parse the file content into a dictionary
        parsed_map: Dict[str, List[str]] = {}
        current_sid = None
        for line in content.splitlines():
            line = line.strip()
            if not line or line.startswith('#'): continue
            match = re.match(r"^(S\d+):$", line)
            if match:
                current_sid = match.group(1)
                parsed_map[current_sid] = []
            elif current_sid and "->" in line:
                parsed_map[current_sid].append(line)
        
        # Fix common LLM error: possessives split across two lines
        for s_id in parsed_map:
            parsed_map[s_id] = fix_possessive_splits_in_lines(parsed_map[s_id])
        
        # --- NEW VALIDATION LOGIC ---
        # 1. Completeness Check
        source_s_ids = {
            block['s_id'] for block in input_data.get("content_blocks", [])
            if block.get("block_type") == "sentence"
        }
        edited_s_ids = set(parsed_map.keys())

        if source_s_ids != edited_s_ids:
            missing = sorted(list(source_s_ids - edited_s_ids))
            extra = sorted(list(edited_s_ids - source_s_ids))
            error_parts = []
            if missing: error_parts.append(f"Missing sentences: {missing}")
            if extra: error_parts.append(f"Extra/unexpected sentences: {extra}")
            raise validator.ValidationError(f"Sentence ID mismatch in approved file. {'; '.join(error_parts)}")

        # 2. Structural Integrity Check
        for block in input_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                s_id = block['s_id']
                raw_map_lines = parsed_map.get(s_id, [])
                target_tier = next((t for t in block["tiers"] if t["tier_id"] == "basic_target"), None)
                
                if not raw_map_lines or not target_tier: continue
                
                original_tokens = [t for seg in target_tier["segments"] for t in seg["tokenized_text"]]
                
                llm_groups = []
                for line in raw_map_lines:
                    if '->' in line:
                        parts = line.split('->', 1)
                        if len(parts) == 2 and parts[0].strip():
                            llm_groups.append(parts[0].strip())
                
                try:
                    refactor_token_stream(original_tokens, llm_groups)
                except validator.ValidationError as e:
                    raise validator.ValidationError(f"Validation failed for S_ID '{s_id}': {e}")
        
        logger.info("      -> Approved map passed all validation checks.")
        return parsed_map

    def run(self) -> bool:
        """Custom run method for this consumption stage."""
        logger.info(f"Executing Stage {self.stage_number}: {self.stage_name} for '{self.book_stem}'")
        self.stage_output_dir.mkdir(parents=True, exist_ok=True)
        
        if self.output_path.exists():
            logger.info("      -> Stage is already complete. Skipping.")
            return True

        input_data = self._load_input_data()
        if not input_data: return False

        try:
            # The validation is now part of the loading function
            approved_map = self._load_and_validate_approved_map(input_data)
            output_data = self._process_data(input_data, approved_map)
            
            if self._save_output_data(output_data, "COMPLETED"):
                logger.info(f"      -> Successfully completed Stage {self.stage_number}.")
                return True
            else:
                return False
        except validator.ValidationError as e:
            logger.error(f"      -> CRITICAL: Validation failed for human-edited file '{self.input_map_path.name}'.")
            logger.error(f"         Reason: {e}")
            logger.error("         Please correct the error in the file and re-run the pipeline.")
            return False

    def _process_data(self, data: Dict[str, Any], approved_map: Dict[str, List[str]]) -> Dict[str, Any]:
        """Processes the data using the validated inverse mappings."""
        for block in data.get("content_blocks", []):
            if block.get("block_type") != "sentence":
                continue

            s_id = block['s_id']
            raw_map_lines = approved_map.get(s_id)
            target_tier = next((t for t in block["tiers"] if t["tier_id"] == "basic_target"), None)

            if not raw_map_lines or not target_tier:
                block.setdefault("mappings", {})["basic_target_to_basic_base_inv_diglot"] = {}
                continue

            # Parse the approved file content
            llm_groups = []
            llm_map_by_group = {}
            for line in raw_map_lines:
                if '->' in line:
                    parts = line.split('->', 1)
                    if len(parts) == 2 and parts[0].strip():
                        group_str = parts[0].strip()
                        llm_groups.append(group_str)
                        llm_map_by_group[group_str] = parts[1].strip()
            
            original_tokens = [t for seg in target_tier["segments"] for t in seg["tokenized_text"]]
            new_target_tokens = refactor_token_stream(original_tokens, llm_groups)
            
            new_target_tier_full_text = "".join(t['v'] for t in new_target_tokens)
            # Rebuild the target tier with a single, fused segment
            target_tier["segments"] = [{"seg_id": "S1", "text": new_target_tier_full_text, "tokenized_text": new_target_tokens}]
            target_tier["full_text"] = new_target_tier_full_text
            
            # Build the final inverse diglot map
            map_entries_for_seg = []
            word_token_idx = 0
            for token in new_target_tokens:
                if token['t'] == 'w':
                    group_str = token['v']
                    spa_word_count = len(re.findall(r"[\w']+", group_str))
                    
                    eng_substitute = llm_map_by_group.get(group_str, "NO_SUB")
                    eng_word_count = len(re.findall(r"[\w']+", eng_substitute))
                    
                    map_entries_for_seg.append([word_token_idx, "TBD", eng_substitute, eng_word_count, spa_word_count])
                    word_token_idx += 1
            
            new_inv_diglot_map = {"S1": map_entries_for_seg}
            
            mappings = block.setdefault("mappings", {})
            mappings["basic_target_to_basic_base_inv_diglot"] = new_inv_diglot_map

            if "raw_simple_to_base_inv_diglot_map" in mappings:
                del mappings["raw_simple_to_base_inv_diglot_map"]

            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

        return data