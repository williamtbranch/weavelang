# In llm2books/stages/apply_phrase_mappings.py

from typing import Any, Dict, List

from .base import SpaCyStage, logger
# --- THIS IS THE FIX ---
# Import the new master function and remove the old parser import.
from ..phrase_mapper_helpers import align_and_parse_to_atoms, sanitize_atoms
# --- END OF FIX ---
from .. import validator

class ApplyPhraseMappings(SpaCyStage):
    """
    Stage 6: A robust refactorer using a DP alignment algorithm.
    - Uses a unified parser to create "semantic atoms" from noisy LLM data.
    - Sanitizes the atoms against segment and punctuation boundaries.
    - Rebuilds the `base` tier by fusing tokens into "virtual tokens".
    - Creates the final, correctly structured `simple_target_to_base_diglot` map.
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
        for block in data.get("content_blocks", []):
            if block.get("block_type") != "sentence":
                continue

            raw_map_lines = block.get("mappings", {}).get("raw_phrase_map", [])
            if not raw_map_lines:
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                continue

            s_id = block['s_id']
            base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), None)
            if not base_tier:
                logger.warning(f"S_ID {s_id}: Skipping ApplyPhraseMappings as no 'base' tier was found.")
                continue

            try:
                original_word_tokens = [t for seg in base_tier["segments"] for t in seg["tokenized_text"] if t['t'] == 'w']
                
                # --- THIS IS THE FIX ---
                # Call the new unified parser.
                initial_atoms = align_and_parse_to_atoms(raw_map_lines, original_word_tokens)
                # --- END OF FIX ---
                
                sanitized_atoms = sanitize_atoms(s_id, initial_atoms, base_tier)

                new_base_tier, new_diglot_map = self._rebuild_base_tier_and_map(base_tier, sanitized_atoms)

                self._validate_reconstruction(s_id, base_tier, new_base_tier)
                
                for i, tier in enumerate(block["tiers"]):
                    if tier["tier_id"] == "base":
                        block["tiers"][i] = new_base_tier
                        break
                
                block["mappings"]["simple_target_to_base_diglot"] = new_diglot_map
                del block["mappings"]["raw_phrase_map"] 

                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

            except (ValueError, validator.ValidationError) as e:
                logger.error(f"Halting due to critical error in ApplyPhraseMappings for S_ID {s_id}: {e}")
                block.setdefault("processing_status", {})[self.stage_name] = f"FAILED: {e}"
                self._save_output_data(data, "PARTIAL_FAILED")
                raise
        return data

    # The rest of the file (_rebuild_base_tier_and_map, _validate_reconstruction) remains unchanged.
    def _rebuild_base_tier_and_map(self, original_base_tier, sanitized_atoms):
        """
        A robust, segment-aware method to rebuild the base tier and diglot map.
        """
        new_diglot_map = {}
        new_base_tier = {
            "tier_id": "base",
            "full_text": original_base_tier['full_text'],
            "segments": []
        }
        
        # Create a lookup map for atoms by their starting diglot index
        atom_map_by_di = {atom.di: atom for atom in sanitized_atoms}

        # Iterate through the original segments, processing each one individually
        for original_seg in original_base_tier['segments']:
            new_seg_tokens = []
            map_entries_for_seg = []
            
            original_tokens = original_seg['tokenized_text']
            token_cursor = 0

            while token_cursor < len(original_tokens):
                token = original_tokens[token_cursor]
                
                # If it's a background token, just append it and continue
                if token['t'] == 'b':
                    new_seg_tokens.append(token)
                    token_cursor += 1
                    continue
                
                current_di = token.get('di')
                
                # Check if this word token is the start of a multi-word atom
                if current_di in atom_map_by_di:
                    atom = atom_map_by_di[current_di]
                    
                    # --- Create the virtual token ---
                    words_in_atom = len(atom.en_words)
                    words_consumed = 0
                    consumed_text = ""
                    temp_cursor = token_cursor
                    
                    # Consume all tokens (word and background) that make up this atom
                    while temp_cursor < len(original_tokens) and words_consumed < words_in_atom:
                        sub_token = original_tokens[temp_cursor]
                        consumed_text += sub_token['v']
                        if sub_token['t'] == 'w':
                            words_consumed += 1
                        temp_cursor += 1
                    
                    # The new virtual token inherits the 'di' from the first original token
                    new_seg_tokens.append({'t': 'w', 'v': consumed_text, 'di': atom.di})
                    
                    # --- Create the diglot map entry ---
                    is_viable = atom.es_phrase.upper() != "NO_SUB"
                    map_entries_for_seg.append([atom.di, "TBD", atom.es_phrase, is_viable])
                    
                    # Advance the main cursor past all consumed tokens
                    token_cursor = temp_cursor
                else:
                    # This should not happen if sanitize_atoms works correctly, but as a fallback:
                    # Treat as a single, unmapped word.
                    new_seg_tokens.append(token)
                    map_entries_for_seg.append([current_di, "TBD", "NO_SUB", False])
                    token_cursor += 1

            # Finalize the new segment
            reconstructed_text = "".join(t['v'] for t in new_seg_tokens)
            new_seg = {
                "seg_id": original_seg['seg_id'],
                "tokenized_text": new_seg_tokens,
                "text": reconstructed_text
            }
            new_base_tier['segments'].append(new_seg)
            new_diglot_map[original_seg['seg_id']] = map_entries_for_seg

        return new_base_tier, new_diglot_map

    def _validate_reconstruction(self, s_id: str, original_tier: Dict, new_tier: Dict):
        logger.debug(f"S_ID {s_id}: Running reconstruction validation for base tier...")
        if original_tier['full_text'] != new_tier['full_text']:
            raise validator.ValidationError(f"S_ID {s_id}: Full text mismatch after base tier refactor.")
        if len(original_tier['segments']) != len(new_tier['segments']):
            raise validator.ValidationError(f"S_ID {s_id}: Segment count mismatch after base tier refactor.")
        for i, original_seg in enumerate(original_tier['segments']):
            new_seg = new_tier['segments'][i]
            if original_seg['seg_id'] != new_seg['seg_id']:
                raise validator.ValidationError(f"S_ID {s_id}: Segment ID mismatch at index {i}.")
            reconstructed_token_text = "".join(token['v'] for token in new_seg['tokenized_text'])
            if reconstructed_token_text != new_seg['text']:
                raise validator.ValidationError(f"S_ID {s_id}, Seg {original_seg['seg_id']}: Internal inconsistency.")
        logger.debug(f"S_ID {s_id}: Reconstruction validation passed.")