# llm2books/stages/apply_phrase_mappings.py
from typing import Any, Dict, List
import re

from .base import Stage, logger # Inherit from the simpler Stage, not LLMStage
from ..phrase_mapper_helpers import refactor_token_stream, parse_proper_nouns
from .. import validator

# --- Configuration for the Human-in-the-Loop workflow ---
HUMAN_REVIEW_DIR_NAME = "human_review"
HUMAN_REVIEW_MARKER = "%%HUMAN_REVIEW_APPROVED%%"


class ApplyPhraseMappings(Stage):
    """
    Stage 6 (V11): Consumes the human-approved `*.dig.txt` file. It validates the
    structural integrity of the user's edits, refactors the `basic_base` tier,
    and builds the final `basic_spanish_to_basic_english_diglot` map in the JSON.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=6,
            stage_name="ApplyPhraseMappings"
        )
        self.human_review_dir = self.pipeline_run_dir / HUMAN_REVIEW_DIR_NAME
        self.input_map_path = self.human_review_dir / f"{self.book_stem}.dig.txt"

        # This stage needs the target language SpaCy model for parsing proper nouns
        self.spacy_target = self.resources["spacy_models"][self.resources["language_config"]["target_code"]]

    #
    def _load_and_validate_approved_map(self, input_data: Dict[str, Any]) -> Dict[str, List[str]]:
        """
        Gate 2 Validator: Loads the approved map and validates its entire
        content for completeness and structural integrity against the input data.
        """
        logger.info(f"      -> Loading and validating approved map: {self.input_map_path.name}")
        
        try:
            with open(self.input_map_path, 'r', encoding='utf-8') as f:
                content = f.read()
            if not content.strip().startswith(HUMAN_REVIEW_MARKER):
                raise validator.ValidationError("File is not approved. The approval marker is missing or commented out.")
        except (IOError, FileNotFoundError) as e:
            raise validator.ValidationError(f"Cannot read or find approved mapping file: {e}")

        # Parse the file content into a dictionary of {s_id: [mapping_lines]}
        parsed_map: Dict[str, List[str]] = {}
        current_sid = None
        for line in content.splitlines():
            line = line.strip()
            if not line or line.startswith('#'):
                continue
            
            match = re.match(r"^(S\d+):$", line)
            if match:
                current_sid = match.group(1)
                parsed_map[current_sid] = []
            elif current_sid and "->" in line:
                parsed_map[current_sid].append(line)
        
        # --- NEW VALIDATION LOGIC ---
        # 1. Completeness Check: Ensure all sentences are present
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

        # 2. Structural Integrity Check: Validate every sentence's mappings
        for block in input_data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                s_id = block['s_id']
                raw_map_lines = parsed_map.get(s_id, [])
                base_tier = next((t for t in block["tiers"] if t["tier_id"] == "basic_base"), None)
                
                if not raw_map_lines or not base_tier: continue
                
                original_tokens = [t for seg in base_tier["segments"] for t in seg["tokenized_text"]]
                
                llm_groups = []
                for line in raw_map_lines:
                    if '->' in line:
                        parts = line.split('->', 1)
                        if len(parts) == 2 and parts[0].strip():
                            llm_groups.append(parts[0].strip())
                
                try:
                    refactor_token_stream(original_tokens, llm_groups)
                except validator.ValidationError as e:
                    # Re-raise with more context for the user
                    raise validator.ValidationError(f"Validation failed for S_ID '{s_id}': {e}")
        
        logger.info("      -> Approved map passed all validation checks.")
        return parsed_map

    def run(self) -> bool:
        """
        Custom run method for this consumption stage.
        """
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
            
            # Now, process the data using the approved map
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
        """
        The main processing logic that applies the validated mappings.
        """
        for block in data.get("content_blocks", []):
            if block.get("block_type") != "sentence":
                continue

            s_id = block['s_id']
            raw_map_lines = approved_map.get(s_id)
            base_tier = next((t for t in block["tiers"] if t["tier_id"] == "basic_base"), None)

            if not raw_map_lines or not base_tier:
                block.setdefault("mappings", {})["basic_spanish_to_basic_english_diglot"] = {}
                continue

            # Parse LLM output into left-side groups and a lookup map
            llm_groups = []
            llm_map_by_group = {}
            for line in raw_map_lines:
                if '->' in line:
                    parts = line.split('->', 1)
                    if len(parts) == 2 and parts[0].strip():
                        group_str = parts[0].strip()
                        llm_groups.append(group_str)
                        llm_map_by_group[group_str] = parts[1].strip()

            # Refactor the base tier token stream using the generic helper
            original_tokens = [t for seg in base_tier["segments"] for t in seg["tokenized_text"]]
            
            # This call now serves as the final, critical validation of the user's edits
            #
            new_base_tokens = refactor_token_stream(original_tokens, llm_groups)
            
            # --- START: CRITICAL FIX FOR DI SEQUENTIALITY ---
            old_di_to_new_di = {}
            new_di_counter = 0
            for token in new_base_tokens:
                if token['t'] == 'w':
                    original_di = token['di'] # This is the first di of the fused group
                    old_di_to_new_di[original_di] = new_di_counter
                    token['di'] = new_di_counter # Re-assign the di to be sequential
                    new_di_counter += 1
            # --- END: CRITICAL FIX ---

            # Rebuild the base tier with a single, fused segment
            new_base_tier_full_text = "".join(t['v'] for t in new_base_tokens)
            new_base_tier = {
                "tier_id": "basic_base", "full_text": new_base_tier_full_text,
                "segments": [{"seg_id": "S1", "text": new_base_tier_full_text, "tokenized_text": new_base_tokens}]
            }

            # Build the new diglot map and collect proper nouns
            new_diglot_map_entries = []
            all_proper_noun_lemmas = set()

            # Note: We now iterate through the original (pre-fusion) word tokens to build the map,
            # ensuring every original word has a corresponding map entry.
            original_word_tokens = [t for t in original_tokens if t['t'] == 'w']
            
            # This logic implicitly handles finding the correct group for each original token.
            current_new_token_idx = 0
            for original_token in original_word_tokens:
                if current_new_token_idx < len(new_base_tokens):
                    # Find which fused token this original token belongs to
                    fused_token_for_word = next((t for t in new_base_tokens if t.get('t') == 'w' and t.get('di') == old_di_to_new_di.get(original_token['di'])), None)
                    
                    if fused_token_for_word:
                        group_str = fused_token_for_word['v']
                        llm_output_phrase = llm_map_by_group.get(group_str, "NO_SUB")
                        clean_phrase, pn_lemmas = parse_proper_nouns(llm_output_phrase, self.spacy_target)
                        all_proper_noun_lemmas.update(pn_lemmas)
                        is_viable = clean_phrase.upper() != "NO_SUB"
                        word_count = len(re.findall(r"[\w']+", original_token.get("v", "")))
                        
                        # Use the NEW, sequential di for the map
                        new_di = old_di_to_new_di[original_token['di']]

                        # Ensure we only add one entry per new_di
                        if not any(entry[0] == new_di for entry in new_diglot_map_entries):
                             new_diglot_map_entries.append([
                                new_di, "TBD", clean_phrase, is_viable, word_count, pn_lemmas
                            ])
            # Update the block with the new data structures
            for i, tier in enumerate(block["tiers"]):
                if tier["tier_id"] == "basic_base":
                    block["tiers"][i] = new_base_tier
                    break
            
            mappings = block.setdefault("mappings", {})
            mappings["basic_spanish_to_basic_english_diglot"] = {"S1": new_diglot_map_entries}
            
            if all_proper_noun_lemmas:
                block["_internal_proper_noun_lemmas"] = sorted(list(all_proper_noun_lemmas))

            # Delete the temporary raw map from the previous stage
            if "raw_phrase_map" in mappings:
                del mappings["raw_phrase_map"] 

            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

        return data