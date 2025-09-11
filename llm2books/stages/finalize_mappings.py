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
                
                base_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "base"), None)
                base_token_map = {}
                if base_tier:
                    for seg in base_tier.get("segments", []):
                        for token in seg.get("tokenized_text", []):
                            if token.get("t") == "w" and "di" in token:
                                base_token_map[token["di"]] = token

                diglot_map = block.get("mappings", {}).get("simple_target_to_base_diglot", {})
                for seg_id, entries in diglot_map.items():
                    new_entries = []
                    for entry in entries:
                        base_di, _, target_phrase, is_viable = entry
                        
                        lemmas_list = []
                        if is_viable and target_phrase:
                            doc = spacy_target(target_phrase)
                            lemmas_list = [
                                helper.normalize_spanish_lemma(token.lemma_)
                                for token in doc if not token.is_punct and not token.is_space
                            ]
                            lemmas_list = [lemma for lemma in lemmas_list if lemma]
                        
                        eng_word_count = 0
                        base_token = base_token_map.get(base_di)
                        if base_token:
                            eng_word_count = len(base_token.get("v", "").split())
                        
                        new_entries.append([base_di, lemmas_list, target_phrase, is_viable, eng_word_count])
                    
                    diglot_map[seg_id] = new_entries

                inv_diglot_map = block.get("mappings", {}).get("simple_target_to_base_inv_diglot", {})
                source_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simple_target"), None)
                
                if source_tier and inv_diglot_map:
                    for seg_id, entries in inv_diglot_map.items():
                        seg = next((s for s in source_tier.get("segments", []) if s["seg_id"] == seg_id), None)
                        if not seg: continue
                        
                        virtual_word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok.get("t") == "w"]
                        
                        for entry in entries:
                            v_token_index = entry[0]
                            lemmas_list = virtual_word_tokens[v_token_index].get("l", []) if v_token_index < len(virtual_word_tokens) else []
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