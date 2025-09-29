# llm2books/stages/apply_inverse_phrase_mappings.py
import re
from typing import Any, Dict, List

from .base import SpaCyStage, logger
from ..phrase_mapper_helpers import align_and_parse_to_atoms, sanitize_atoms, SemanticAtom
from .. import validator
from .. import semantic_validator 

class ApplyInversePhraseMappings(SpaCyStage):
    """
    Stage 6: Processes the raw inverse phrase map from Stage 5.
    - Parses the raw map into structured "semantic atoms".
    - Performs word-content and semantic validation on the parsed atoms.
    - Fuses Spanish tokens based on the `_internal_di_fusion_map` from Stage 4.
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
                di_fusion_map = mappings.get("_internal_di_fusion_map", {})
                tier = next((t for t in block["tiers"] if t["tier_id"] == "simple_target"), None)

                if not tier or not raw_map_data:
                    block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                    continue
                
                all_sanitized_atoms = []
                for seg in tier.get("segments", []):
                    seg_id = seg["seg_id"]
                    raw_map_lines = raw_map_data.get(seg_id, [])
                    source_word_tokens = [t for t in seg.get("tokenized_text", []) if t['t'] == 'w']
                    if not source_word_tokens: continue

                    initial_atoms = align_and_parse_to_atoms(raw_map_lines, source_word_tokens)
                    
                    source_words_from_tier_str = " ".join([t['v'] for t in source_word_tokens])
                    source_words_from_map_str = " ".join([word for atom in initial_atoms for word in atom.en_words])
                    
                    if source_words_from_tier_str != source_words_from_map_str:
                        raise validator.ValidationError(f"S_ID {s_id}_{seg_id}: Word content mismatch in inverse map.")

                    sanitized_atoms_for_seg = sanitize_atoms(f"{s_id}_{seg_id}", initial_atoms, {"segments": [seg]})
                    all_sanitized_atoms.extend(sanitized_atoms_for_seg)
                
                new_tier, new_inv_diglot_map = self._rebuild_tier_and_map(tier, all_sanitized_atoms, di_fusion_map)
                
                self._validate_reconstruction(s_id, tier, new_tier)
                
                # Temporarily place the new data on the block for the validator to use
                block["mappings"]["simple_target_to_base_inv_diglot"] = new_inv_diglot_map
                for i, t in enumerate(block["tiers"]):
                    if t["tier_id"] == "simple_target":
                        block["tiers"][i] = new_tier
                        break
                
                # --- THIS IS THE CORRECTED VALIDATION CALL ---
                is_semantically_valid = semantic_validator.validate_inverse_mappings(block)
                if not is_semantically_valid:
                    raise validator.ValidationError(f"S_ID {s_id} failed INVERSE semantic validation.")
                
                # If validation passes, mark as complete and clean up
                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
                if "raw_simple_to_base_inv_diglot_map" in mappings: del mappings["raw_simple_to_base_inv_diglot_map"]
                if "_internal_di_fusion_map" in mappings: del mappings["_internal_di_fusion_map"]

            except (validator.ValidationError, KeyError, AttributeError) as e:
                logger.error(f"Halting due to data integrity/validation error in {self.stage_name} for S_ID {s_id}: {e}")
                block.setdefault("processing_status", {})[self.stage_name] = f"RETRY_FAIL: {e}"
                self._save_output_data(data, "PARTIAL_FAILED")
                raise

        return data

    def _rebuild_tier_and_map(self, original_tier: Dict, sanitized_atoms: List[SemanticAtom], di_fusion_map: Dict[str, List[int]]):
        new_inv_diglot_map = {}
        new_tier = { "tier_id": original_tier["tier_id"], "full_text": original_tier["full_text"], "lemmas": original_tier["lemmas"], "segments": [] }
        atom_map_by_di = {atom.di: atom for atom in sanitized_atoms}
        for original_seg in original_tier["segments"]:
            new_seg_tokenized_text, map_entries_for_seg, segment_word_index = [], [], 0
            original_tokens = original_seg["tokenized_text"]
            token_cursor = 0
            while token_cursor < len(original_tokens):
                token = original_tokens[token_cursor]
                if token['t'] == 'b':
                    new_seg_tokenized_text.append(token)
                    token_cursor += 1
                    continue
                current_di = token.get('di')
                if current_di is None:
                    token_cursor += 1
                    continue
                original_dis_to_consume = di_fusion_map.get(str(current_di), [current_di])
                num_tokens_to_consume = len(original_dis_to_consume)
                consumed_text, aggregated_lemmas, start_token_for_copy = "", set(), None
                temp_cursor = token_cursor
                for _ in range(num_tokens_to_consume):
                    if temp_cursor < len(original_tokens):
                        sub_token = original_tokens[temp_cursor]
                        if start_token_for_copy is None: start_token_for_copy = sub_token
                        consumed_text += sub_token['v']
                        if 'l' in sub_token and sub_token['l']: aggregated_lemmas.update(sub_token['l'])
                        temp_cursor +=1
                if start_token_for_copy is None:
                    token_cursor += 1
                    continue
                virtual_token = start_token_for_copy.copy()
                virtual_token['v'] = consumed_text
                virtual_token['l'] = sorted(list(aggregated_lemmas))
                new_seg_tokenized_text.append(virtual_token)
                atom = atom_map_by_di.get(current_di)
                eng_substitute = atom.es_phrase if atom else "NO_SUB"
                eng_word_count = len(atom.en_words) if atom else 1
                map_entries_for_seg.append([segment_word_index, "TBD", eng_substitute, eng_word_count])
                segment_word_index += 1
                token_cursor = temp_cursor
            reconstructed_text = "".join(t['v'] for t in new_seg_tokenized_text)
            new_seg = { "seg_id": original_seg["seg_id"], "text": reconstructed_text, "lemmas": original_seg["lemmas"], "tokenized_text": new_seg_tokenized_text }
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