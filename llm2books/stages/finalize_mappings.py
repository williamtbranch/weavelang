# llm2books/stages/finalize_mappings.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper
from .. import validator

class FinalizeMappings(SpaCyStage):
    """
    Stage 6: Lemmatizes the target words in both the simple diglot map and the
    inverse diglot map, making them ready for the Rust engine.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=8,
            stage_name="FinalizeMappings"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                # --- Finalize the Simple Diglot Map ---
                diglot_map = block.get("mappings", {}).get("simple_target_to_base_diglot", {})
                for seg_id, entries in diglot_map.items():
                    for entry in entries:
                        # entry is [base_word_di, "TBD", "exact_target_form", is_viable_bool]
                        is_viable, target_phrase = entry[3], entry[2]
                        
                        # --- THIS IS THE DEFINITIVE FIX ---
                        lemmas_list = [] # Default to an empty list
                        if is_viable and target_phrase:
                            # Process the entire phrase with SpaCy
                            doc = spacy_target(target_phrase)
                            # Extract, normalize, and filter lemmas for all tokens
                            lemmas_list = [
                                helper.normalize_spanish_lemma(token.lemma_)
                                for token in doc if not token.is_punct and not token.is_space
                            ]
                            # Remove any empty strings that might result
                            lemmas_list = [lemma for lemma in lemmas_list if lemma]
                        
                        # Replace "TBD" with the final list of lemmas
                        entry[1] = lemmas_list
                        # --- END OF FIX ---

                # --- Finalize the Inverse Diglot Map (This logic is still single-word) ---
                inv_diglot_map = block.get("mappings", {}).get("simpler_adv_target_to_base_inv_diglot", {})
                simpler_adv_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "simpler_advanced_target"), None)
                if simpler_adv_tier and inv_diglot_map:
                    for seg_id, entries in inv_diglot_map.items():
                        seg = next((s for s in simpler_adv_tier.get("segments", []) if s["seg_id"] == seg_id), None)
                        if not seg: continue
                        
                        word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok.get("t") == "w"]
                        for entry in entries:
                            word_index = entry[0]
                            target_word = word_tokens[word_index].get("v") if word_index < len(word_tokens) else None
                            lemma_str = ""
                            if target_word:
                                doc = spacy_target(target_word)
                                main_token = next((t for t in doc if not t.is_punct and not t.is_space), None)
                                lemma_str = helper.normalize_spanish_lemma(main_token.lemma_ if main_token else target_word)
                            entry[1] = lemma_str

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