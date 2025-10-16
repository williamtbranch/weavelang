# llm2books/stages/finalize_mappings.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper
from .. import validator

class FinalizeMappings(SpaCyStage):
    """
    Stage 7: Lemmatizes the target words in both the simple diglot map and the
    inverse diglot map, making them ready for the Rust engine.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=7,
            stage_name="FinalizeMappings"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                
                # --- Process Forward Diglot Map ---
                diglot_map = block.get("mappings", {}).get("simple_target_to_base_diglot", {})
                for seg_id, entries in diglot_map.items():
                    for entry in entries:
                        # Entry format is now [di, "TBD", "form", viable, word_count, [pn_lemmas]]
                        is_viable = entry[3]
                        target_phrase = entry[2]
                        
                        lemmas_list = []
                        if is_viable and target_phrase:
                            doc = spacy_target(target_phrase)
                            lemmas_list = [
                                helper.normalize_spanish_lemma(token.lemma_)
                                for token in doc if not token.is_punct and not token.is_space
                            ]
                            lemmas_list = sorted([lemma for lemma in lemmas_list if lemma])
                        
                        # Replace the "TBD" placeholder with the final lemma list
                        entry[1] = lemmas_list

                # --- Process Inverse Diglot Map ---
                inv_diglot_map = block.get("mappings", {}).get("simple_target_to_base_inv_diglot", {})
                source_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simple_target"), None)
                
                if source_tier and inv_diglot_map:
                    for seg_id, entries in inv_diglot_map.items():
                        seg = next((s for s in source_tier.get("segments", []) if s["seg_id"] == seg_id), None)
                        if not seg: continue
                        
                        # The tokens in this tier have now been fused by Stage 6
                        fused_word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok.get("t") == "w"]
                        
                        for entry in entries:
                            # Entry format is [v_token_idx, "TBD", eng_sub, eng_word_count]
                            v_token_index = entry[0]
                            lemmas_list = []
                            if v_token_index < len(fused_word_tokens):
                                # Get lemmas from the fused token's 'l' key, which was populated in Stage 2
                                lemmas_list = fused_word_tokens[v_token_index].get("l", [])
                            
                            # Replace the "TBD" placeholder
                            entry[1] = lemmas_list

                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

        logger.info("      -> Validating mapping integrity...")    
        try:
            for block in data.get("content_blocks", []):
                if block.get("block_type") == "sentence":
                    validator.validate_exhaustive_diglot_mapping(block)
                    validator.validate_exhaustive_inverse_diglot_mapping(block)
        except validator.ValidationError as e:
            raise e
        
        return data