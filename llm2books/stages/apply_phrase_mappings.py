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
            stage_number=6,
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

    def _rebuild_tier_and_map(self, original_base_tier, sanitized_atoms):
        # --- This version incorporates the fix for rebuilding the segment 'text' field ---
        new_diglot_map = {}
        new_tier = {
            "tier_id": "base",
            "full_text": original_base_tier['full_text'],
            "segments": []
        }
        
        # Create a flat stream of the new "virtual" tokens
        new_token_stream = []
        flat_original_tokens = [token for seg in original_base_tier.get("segments", []) for token in seg.get("tokenized_text", [])]
        original_token_cursor = 0
        atom_map_by_di = {atom.di: atom for atom in sanitized_atoms}

        while original_token_cursor < len(flat_original_tokens):
            token = flat_original_tokens[original_token_cursor]
            if token['t'] == 'b':
                new_token_stream.append(token)
                original_token_cursor += 1
                continue
            
            # It's a 'w' token
            current_di = token.get('di')
            if current_di in atom_map_by_di:
                atom = atom_map_by_di[current_di]
                
                consumed_text = ""
                words_to_consume = len(atom.en_words)
                words_consumed = 0
                temp_cursor = original_token_cursor
                
                while temp_cursor < len(flat_original_tokens) and words_consumed < words_to_consume:
                    sub_token = flat_original_tokens[temp_cursor]
                    consumed_text += sub_token['v']
                    if sub_token['t'] == 'w':
                        words_consumed += 1
                    temp_cursor += 1
                
                virtual_token = token.copy()
                virtual_token['v'] = consumed_text
                new_token_stream.append(virtual_token)
                original_token_cursor = temp_cursor
            else: # Should not happen with exhaustive mapping
                new_token_stream.append(token)
                original_token_cursor += 1
        
        # Re-bucket the new flat stream into the original segment structure
        new_token_stream_cursor = 0
        for original_seg in original_base_tier['segments']:
            new_seg_tokens = []
            original_text_len = len("".join(t['v'] for t in original_seg['tokenized_text']))
            current_seg_len = 0
            
            while new_token_stream_cursor < len(new_token_stream) and current_seg_len < original_text_len:
                token = new_token_stream[new_token_stream_cursor]
                new_seg_tokens.append(token)
                current_seg_len += len(token['v'])
                new_token_stream_cursor += 1
            
            reconstructed_text = "".join(t['v'] for t in new_seg_tokens)
            new_seg = {
                "seg_id": original_seg['seg_id'],
                "tokenized_text": new_seg_tokens,
                "text": reconstructed_text # Rebuild the text field from the new tokens
            }
            new_tier['segments'].append(new_seg)

        # Create the final diglot map
        for atom in sanitized_atoms:
            is_viable = atom.es_phrase.upper() != "NO_SUB"
            diglot_map_entry = [atom.di, "TBD", atom.es_phrase, is_viable]
            
            first_word_di = atom.di
            seg_id_for_atom = next(
                seg['seg_id'] for seg in new_tier['segments'] 
                for tok in seg['tokenized_text'] if tok.get('di') == first_word_di
            )
            new_diglot_map.setdefault(seg_id_for_atom, []).append(diglot_map_entry)

        return new_tier, new_diglot_map

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
            aggregated_lemmas = set()
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

        # 
        new_token_stream_cursor = 0
        for original_seg in original_base_tier['segments']:
            new_seg_tokens = []
            original_text_len = len("".join(t['v'] for t in original_seg['tokenized_text']))
            current_seg_len = 0
            
            while new_token_stream_cursor < len(new_token_stream) and current_seg_len < original_text_len:
                token = new_token_stream[new_token_stream_cursor]
                new_seg_tokens.append(token)
                current_seg_len += len(token['v'])
                new_token_stream_cursor += 1
            
            reconstructed_text = "".join(t['v'] for t in new_seg_tokens)
            new_seg = {
                "seg_id": original_seg['seg_id'],
                "tokenized_text": new_seg_tokens,
                "text": reconstructed_text # Rebuild the text field from the new tokens
            }
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
            reconstructed_token_text = "".join(token['v'] for token in new_seg['tokenized_text'])
            if reconstructed_token_text != new_seg['text']:
                raise validator.ValidationError(
                    f"S_ID {s_id}, Seg {original_seg['seg_id']}: Internal inconsistency. Reconstructed token text does not match the new segment text field."
                )
        
        logger.debug(f"S_ID {s_id}: Reconstruction validation passed.")