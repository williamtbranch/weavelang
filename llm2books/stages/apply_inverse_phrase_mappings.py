from typing import Any, Dict, List

from .base import SpaCyStage, logger
# --- THIS IS THE FIX ---
# Import the new, correct parser and remove the old one.
from ..phrase_mapper_helpers import align_and_parse_to_atoms, sanitize_atoms, SemanticAtom
# --- END OF FIX ---
from .. import validator

class ApplyInversePhraseMappings(SpaCyStage):
    """
    New Stage 8: Refactors the simpler_advanced_target tier based on a phrase map.
    - Parses the raw phrase map from Stage 7 using the robust DP aligner.
    - Sanitizes the resulting "semantic atoms".
    - Rebuilds the `simpler_advanced_target` tier by fusing tokens into "virtual tokens".
    - Creates the final, correctly structured `simpler_adv_target_to_base_inv_diglot` map.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=8,
            stage_name="ApplyInversePhraseMappings"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        for block in data.get("content_blocks", []):
            if block.get("block_type") != "sentence":
                continue

            raw_map_lines = block.get("mappings", {}).get("raw_inverse_phrase_map", [])
            s_id = block['s_id']
            tier = next((t for t in block["tiers"] if t["tier_id"] == "simpler_advanced_target"), None)

            if not tier:
                logger.warning(f"S_ID {s_id}: Skipping {self.stage_name} as no 'simpler_advanced_target' tier was found.")
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                continue
            
            if not raw_map_lines:
                new_inv_diglot_map = {}
                for seg in tier.get("segments", []):
                    new_inv_diglot_map[seg['seg_id']] = []
                block["mappings"]["simpler_adv_target_to_base_inv_diglot"] = new_inv_diglot_map
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                continue

            try:
                original_word_tokens = [t for seg in tier["segments"] for t in seg["tokenized_text"] if t['t'] == 'w']
                if not original_word_tokens:
                    block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                    continue

                # --- THIS IS THE FIX ---
                # Call the new, robust aligner.
                initial_atoms = align_and_parse_to_atoms(raw_map_lines, original_word_tokens)
                # --- END OF FIX ---
                
                sanitized_atoms = sanitize_atoms(s_id, initial_atoms, tier)

                new_tier, new_inv_diglot_map = self._rebuild_tier_and_map(tier, sanitized_atoms)
                
                self._validate_reconstruction(s_id, tier, new_tier)
                
                for i, t in enumerate(block["tiers"]):
                    if t["tier_id"] == "simpler_advanced_target":
                        block["tiers"][i] = new_tier
                        break
                
                block["mappings"]["simpler_adv_target_to_base_inv_diglot"] = new_inv_diglot_map
                
                temp_validation_block = {
                    "s_id": s_id, "tiers": [new_tier],
                    "mappings": {"simpler_adv_target_to_base_inv_diglot": new_inv_diglot_map}
                }
                validator.validate_exhaustive_inverse_diglot_mapping(temp_validation_block)
                logger.debug(f"S_ID {s_id}: In-stage validation of inverse map passed.")

                if "raw_inverse_phrase_map" in block["mappings"]:
                    del block["mappings"]["raw_inverse_phrase_map"]
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

            except (ValueError, validator.ValidationError) as e:
                logger.error(f"Halting due to critical error in {self.stage_name} for S_ID {s_id}: {e}")
                block.setdefault("processing_status", {})[self.stage_name] = f"FAILED: {e}"
                self._save_output_data(data, "PARTIAL_FAILED")
                raise

        return data

    # The rest of this file (_rebuild_tier_and_map, _validate_reconstruction) remains unchanged.
    def _rebuild_tier_and_map(self, original_tier: Dict, sanitized_atoms: List[SemanticAtom]):
        new_inv_diglot_map = {}
        new_tier = {
            "tier_id": original_tier["tier_id"],
            "full_text": original_tier["full_text"],
            "lemmas": original_tier["lemmas"],
            "segments": []
        }
        atom_map_by_di = {atom.di: atom for atom in sanitized_atoms}
        for original_seg in original_tier["segments"]:
            new_seg_tokenized_text = []
            map_entries_for_seg = []
            segment_word_index = 0
            original_tokens = original_seg["tokenized_text"]
            token_cursor = 0
            while token_cursor < len(original_tokens):
                token = original_tokens[token_cursor]
                if token['t'] == 'b':
                    new_seg_tokenized_text.append(token)
                    token_cursor += 1
                    continue
                current_di = token.get('di')
                if current_di in atom_map_by_di:
                    atom = atom_map_by_di[current_di]
                    consumed_text = ""
                    words_in_atom = len(atom.en_words)
                    words_consumed = 0
                    aggregated_lemmas = set()
                    has_any_lemmas = False
                    temp_cursor = token_cursor
                    while temp_cursor < len(original_tokens) and words_consumed < words_in_atom:
                        sub_token = original_tokens[temp_cursor]
                        consumed_text += sub_token['v']
                        if sub_token['t'] == 'w':
                            words_consumed += 1
                            if 'l' in sub_token and sub_token['l']:
                                has_any_lemmas = True
                                aggregated_lemmas.update(sub_token['l'])
                        temp_cursor += 1
                    virtual_token = token.copy()
                    virtual_token['v'] = consumed_text
                    virtual_token['l'] = sorted(list(aggregated_lemmas))
                    new_seg_tokenized_text.append(virtual_token)
                    eng_substitute = atom.es_phrase
                    if not has_any_lemmas:
                        eng_substitute = "NO_SUB"
                    original_english_word_count = len(atom.en_words)
                    map_entries_for_seg.append([segment_word_index, "TBD", eng_substitute, original_english_word_count])
                    segment_word_index += 1
                    token_cursor = temp_cursor
                else:
                    new_seg_tokenized_text.append(token)
                    map_entries_for_seg.append([segment_word_index, "TBD", "NO_SUB", 1])
                    segment_word_index += 1
                    token_cursor += 1
            reconstructed_text = "".join(t['v'] for t in new_seg_tokenized_text)
            new_seg = {
                "seg_id": original_seg["seg_id"],
                "text": reconstructed_text,
                "lemmas": original_seg["lemmas"],
                "tokenized_text": new_seg_tokenized_text
            }
            new_tier["segments"].append(new_seg)
            new_inv_diglot_map[original_seg["seg_id"]] = map_entries_for_seg
        return new_tier, new_inv_diglot_map

    def _validate_reconstruction(self, s_id: str, original_tier: Dict, new_tier: Dict):
        logger.debug(f"S_ID {s_id}: Running reconstruction validation for {original_tier['tier_id']}...")
        if original_tier['full_text'] != new_tier['full_text']:
            raise validator.ValidationError(f"S_ID {s_id}: Full text mismatch after tier refactor.")
        for i, original_seg in enumerate(original_tier['segments']):
            new_seg = new_tier['segments'][i]
            reconstructed_text_from_new_tokens = "".join(token['v'] for token in new_seg['tokenized_text'])
            if reconstructed_text_from_new_tokens != new_seg['text']:
                raise validator.ValidationError(
                    f"S_ID {s_id}, Seg {original_seg['seg_id']}: Internal data inconsistency after refactor.\n"
                    f"  - Segment Text Field: '{new_seg['text']}'\n"
                    f"  - From Tokens:      '{reconstructed_text_from_new_tokens}'"
                )
        logger.debug(f"S_ID {s_id}: Reconstruction validation passed.")