# In llm2books/stages/apply_phrase_mappings.py

from typing import Any, Dict, List

from .base import SpaCyStage, logger
from ..phrase_mapper_helpers import parse_llm_phrase_map_to_atoms, sanitize_atoms
from .. import validator

class ApplyPhraseMappings(SpaCyStage):
    """
    Stage 5: Rewritten to be a "base tier refactorer".
    - Parses the raw phrase map from Stage 4.
    - Sanitizes the resulting "semantic atoms" against segment and punctuation boundaries.
    - Rebuilds the `base` tier by fusing tokens into larger, multi-word "virtual tokens".
    - Performs rigorous validation to ensure the refactored tier is identical in content.
    - Creates the final, correctly structured `simple_target_to_base_diglot` map.
    - Passes all other tiers through unmodified.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=5,
            stage_name="ApplyPhraseMappings"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        for block in data.get("content_blocks", []):
            if block.get("block_type") != "sentence":
                continue

            raw_map_lines = block.get("mappings", {}).get("raw_phrase_map", [])
            if not raw_map_lines:
                # If there's no map, we just mark as complete and move on.
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                continue

            s_id = block['s_id']
            base_tier = next((t for t in block["tiers"] if t["tier_id"] == "base"), None)
            if not base_tier:
                logger.warning(f"S_ID {s_id}: Skipping ApplyPhraseMappings as no 'base' tier was found.")
                continue

            try:
                # Step A: Parse and Sanitize Atoms
                original_word_tokens = [t for seg in base_tier["segments"] for t in seg["tokenized_text"] if t['t'] == 'w']
                initial_atoms = parse_llm_phrase_map_to_atoms(raw_map_lines, original_word_tokens)
                sanitized_atoms = sanitize_atoms(s_id, initial_atoms, base_tier)

                # Step B: Reconstruct the Base Tier and create the Diglot Map
                new_base_tier, new_diglot_map = self._rebuild_base_tier_and_map(base_tier, sanitized_atoms)

                # Step C: Validation
                self._validate_reconstruction(s_id, base_tier, new_base_tier)
                
                # Step D: Replace the old base tier and update mappings
                for i, tier in enumerate(block["tiers"]):
                    if tier["tier_id"] == "base":
                        block["tiers"][i] = new_base_tier
                        break
                
                block["mappings"]["simple_target_to_base_diglot"] = new_diglot_map
                del block["mappings"]["raw_phrase_map"] # Clean up the raw map

                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

            except (ValueError, validator.ValidationError) as e:
                logger.error(f"Halting due to critical error in ApplyPhraseMappings for S_ID {s_id}: {e}")
                block.setdefault("processing_status", {})[self.stage_name] = f"FAILED: {e}"
                self._save_output_data(data, "PARTIAL_FAILED")
                raise # Re-raise to halt the entire pipeline

        return data

    def _rebuild_base_tier_and_map(self, original_base_tier, sanitized_atoms):
        new_diglot_map = {}
        new_token_stream = []
        
        flat_original_tokens = [token for seg in original_base_tier.get("segments", []) for token in seg.get("tokenized_text", [])]
        original_token_cursor = 0

        for atom in sanitized_atoms:
            is_viable = atom.es_phrase.upper() != "NO_SUB"
            # The diglot map entry is simple, but we need to create the lemma list placeholder
            # The actual lemmatization will happen in FinalizeMappings
            diglot_map_entry = [atom.di, "TBD", atom.es_phrase, is_viable]
            
            # Find the segment this atom belongs to
            first_word_di = atom.di
            seg_id_for_atom = next(
                seg['seg_id'] for seg in original_base_tier['segments'] 
                for tok in seg['tokenized_text'] if tok.get('di') == first_word_di
            )
            new_diglot_map.setdefault(seg_id_for_atom, []).append(diglot_map_entry)

            # --- Reconstruct the virtual token ---
            # Advance cursor to the start of the atom
            while (original_token_cursor < len(flat_original_tokens) and 
                   flat_original_tokens[original_token_cursor].get('di', -1) != atom.di):
                new_token_stream.append(flat_original_tokens[original_token_cursor])
                original_token_cursor += 1

            # Consume all tokens (W and B) that make up this atom
            words_to_consume = len(atom.en_words)
            words_consumed = 0
            consumed_text = ""
            start_cursor = original_token_cursor
            
            while original_token_cursor < len(flat_original_tokens) and words_consumed < words_to_consume:
                token = flat_original_tokens[original_token_cursor]
                consumed_text += token['v']
                if token['t'] == 'w':
                    words_consumed += 1
                original_token_cursor += 1
            
            new_token_stream.append({'t': 'w', 'v': consumed_text, 'di': atom.di})

        # Add any remaining tokens after the last atom
        if original_token_cursor < len(flat_original_tokens):
            new_token_stream.extend(flat_original_tokens[original_token_cursor:])

        # --- Re-assemble the new tier with original segmentation ---
        new_base_tier = {
            "tier_id": "base",
            "full_text": original_base_tier['full_text'],
            "segments": []
        }
        
        new_token_cursor = 0
        for original_seg in original_base_tier['segments']:
            num_tokens_in_original_seg = len(original_seg['tokenized_text'])
            
            # This is complex: we need to find the new tokens that correspond to the old ones.
            # A simpler way is to map new tokens back to their original segment.
            # For now, let's rebuild the segment text from the new stream.
            
            # Rebuild a new token stream segment by segment
            new_seg_tokens = []
            new_text_parts = []
            
            # Find the start/end DIs for the original segment
            original_seg_dis = {tok['di'] for tok in original_seg['tokenized_text'] if 'di' in tok}
            if not original_seg_dis: # Segment is punctuation only
                 new_base_tier['segments'].append(original_seg)
                 continue

            min_di, max_di = min(original_seg_dis), max(original_seg_dis)

            # Find corresponding atoms
            atoms_in_seg = [atom for atom in sanitized_atoms if min_di <= atom.di <= max_di]

            # Rebuild token stream for this segment
            # This is tricky. Let's simplify by re-tokenizing the original segment text
            # and then applying the fusions. For now, let's just copy the original and plan to fix.
            # TODO: This is the hard part - correctly re-bucketing the new fused tokens.
            # A simpler approach for now that is correct but less efficient:
            
        # Re-bucket the new flat stream into the original segment structure
        new_token_stream_cursor = 0
        for original_seg in original_base_tier['segments']:
            new_seg = {"seg_id": original_seg['seg_id'], "tokenized_text": [], "text": original_seg['text']}
            original_text_len = len(original_seg['text'])
            current_seg_len = 0
            
            while new_token_stream_cursor < len(new_token_stream) and current_seg_len < original_text_len:
                token = new_token_stream[new_token_stream_cursor]
                new_seg['tokenized_text'].append(token)
                current_seg_len += len(token['v'])
                new_token_stream_cursor += 1
            
            new_base_tier['segments'].append(new_seg)

        return new_base_tier, new_diglot_map

    def _validate_reconstruction(self, s_id: str, original_tier: Dict, new_tier: Dict):
        """
        Performs the critical integrity check to ensure the refactored tier
        is a perfect content match to the original.
        """
        logger.debug(f"S_ID {s_id}: Running reconstruction validation for base tier...")
        
        # 1. Validate full_text at the tier level (should be identical)
        if original_tier['full_text'] != new_tier['full_text']:
            raise validator.ValidationError(f"S_ID {s_id}: Full text mismatch after base tier refactor.")

        # 2. Validate segments text reconstruction
        if len(original_tier['segments']) != len(new_tier['segments']):
            raise validator.ValidationError(f"S_ID {s_id}: Segment count mismatch after base tier refactor.")

        for i, original_seg in enumerate(original_tier['segments']):
            new_seg = new_tier['segments'][i]
            
            # 2a. Check that seg_id matches
            if original_seg['seg_id'] != new_seg['seg_id']:
                raise validator.ValidationError(f"S_ID {s_id}: Segment ID mismatch at index {i}.")

            # 2b. Reconstruct text from new tokens and compare to original segment text
            reconstructed_text = "".join(token['v'] for token in new_seg['tokenized_text'])
            if reconstructed_text != original_seg['text']:
                raise validator.ValidationError(
                    f"S_ID {s_id}, Seg {original_seg['seg_id']}: Text reconstruction failed after refactor.\n"
                    f"  Expected: '{original_seg['text']}'\n"
                    f"  Got:      '{reconstructed_text}'"
                )
        
        logger.debug(f"S_ID {s_id}: Reconstruction validation passed.")