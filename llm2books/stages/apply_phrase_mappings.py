# In llm2books/stages/apply_phrase_mappings.py

from typing import Any, Dict, List
import re
from .base import SpaCyStage, logger
#from ..phrase_mapper_helpers import align_and_parse_to_atoms, sanitize_atoms
from ..phrase_mapper_helpers import  refactor_token_stream, parse_proper_nouns
from .. import validator
from .. import semantic_validator

class ApplyPhraseMappings(SpaCyStage):
    """
    Stage 4: Refactors the base tier and applies phrase mappings.
    - Uses refactor_token_stream to validate LLM output and fuse the base tier tokens.
    - Parses proper noun markup from the LLM's Spanish output.
    - Creates the final, correctly structured simple_target_to_base_diglot map.
    - Performs semantic validation on the generated mappings.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=4,
            stage_name="ApplyPhraseMappings"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        spacy_es = self.resources["spacy_models"][self.resources["language_config"]["target_code"]]

        for block in data.get("content_blocks", []):
            if block.get("block_type") != "sentence":
                continue

            raw_map_lines = block.get("mappings", {}).get("raw_phrase_map", [])
            if not raw_map_lines:
                block.setdefault("mappings", {})["simple_target_to_base_diglot"] = {}
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                continue

            s_id = block['s_id']
            base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), None)
            if not base_tier:
                logger.warning(f"S_ID {s_id}: Skipping ApplyPhraseMappings as no 'base' tier was found.")
                continue

            # --- START: NEW REFACTORED LOGIC ---
            
            # 1. Parse LLM output into left-side groups and a lookup map
            llm_groups = []
            llm_map_by_group = {}
            for line in raw_map_lines:
                if '->' in line:
                    parts = line.split('->', 1)
                    if len(parts) == 2 and parts[0].strip():
                        group_str = parts[0].strip()
                        llm_groups.append(group_str)
                        llm_map_by_group[group_str] = parts[1].strip()

            # 2. Refactor the base tier token stream using the generic helper
            original_tokens = [t for seg in base_tier["segments"] for t in seg["tokenized_text"]]
            new_base_tokens = refactor_token_stream(original_tokens, llm_groups)
            
            # Rebuild the base tier with a single, fused segment
            new_base_tier_full_text = "".join(t['v'] for t in new_base_tokens)
            new_base_tier = {
                "tier_id": "base",
                "full_text": new_base_tier_full_text,
                "segments": [{"seg_id": "S1", "text": new_base_tier_full_text, "tokenized_text": new_base_tokens}]
            }

            # 3. Build the new diglot map and collect proper nouns
            new_diglot_map_entries = []
            all_proper_noun_lemmas_for_sentence = set()

            for token in new_base_tokens:
                if token['t'] == 'w':
                    # The token 'v' now represents the entire group string, e.g., "Gregor Samsa"
                    group_str = token['v']
                    llm_output_phrase = llm_map_by_group.get(group_str, "NO_SUB")

                    clean_phrase, pn_lemmas = parse_proper_nouns(llm_output_phrase, spacy_es)
                    all_proper_noun_lemmas_for_sentence.update(pn_lemmas)

                    is_viable = clean_phrase.upper() != "NO_SUB"
                    word_count = len(re.findall(r"[\w']+", token.get("v", "")))
                    
                    # The 6th element is now the list of identified proper noun lemmas
                    new_diglot_map_entries.append([
                        token["di"], "TBD", clean_phrase, is_viable, word_count, pn_lemmas
                    ])

            # 4. Update the block with the new data structures
            for i, tier in enumerate(block["tiers"]):
                if tier["tier_id"] == "base":
                    block["tiers"][i] = new_base_tier
                    break
            
            mappings = block.setdefault("mappings", {})
            mappings["simple_target_to_base_diglot"] = {"S1": new_diglot_map_entries}
            
            # Store the collected proper noun lemmas for the final cleanup stage
            if all_proper_noun_lemmas_for_sentence:
                block["_internal_proper_noun_lemmas"] = sorted(list(all_proper_noun_lemmas_for_sentence))

            if "raw_phrase_map" in mappings:
                del mappings["raw_phrase_map"] 

            # --- END: NEW REFACTORED LOGIC ---

        # --- VALIDATION GATE (Unchanged but critical) ---
        logger.info("      -> Running in-stage structural and semantic validations...")
        any_semantic_failures = False
        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                validator.validate_exhaustive_diglot_mapping(block)
                
                is_semantically_valid = semantic_validator.validate_forward_mappings(block)
                
                if is_semantically_valid:
                    block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                else:
                    block.setdefault("processing_status", {})[self.stage_name] = "RETRY_SEMANTIC_FAIL"
                    any_semantic_failures = True
        
        if any_semantic_failures:
            raise validator.ValidationError("One or more sentences failed semantic validation.")

        logger.info("      -> All validations passed.")
        return data