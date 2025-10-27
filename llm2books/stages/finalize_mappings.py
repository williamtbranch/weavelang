# llm2books/stages/finalize_mappings.py
from typing import Any, Dict

from .base import SpaCyStage, logger
from .. import helper, validator

class FinalizeMappings(SpaCyStage):
    """
    Stage 8 (V11): Lemmatizes the target words in both the forward and inverse
    diglot maps, replacing the "TBD" placeholders. It is updated to use the
    new `basic_*` tier and mapping names.
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
                
                # --- Process Forward Diglot Map ---
                diglot_map = block.get("mappings", {}).get("basic_spanish_to_basic_english_diglot", {})
                for seg_id, entries in diglot_map.items():
                    for entry in entries:
                        is_viable = entry[3]
                        target_phrase = entry[2]
                        
                        lemmas_list = []
                        if is_viable and target_phrase:
                            doc = spacy_target(target_phrase)
                            lemmas_list = sorted([
                                lemma for token in doc if not token.is_punct and not token.is_space
                                if (lemma := helper.normalize_spanish_lemma(token.lemma_))
                            ])
                        
                        entry[1] = lemmas_list

                # --- Process Inverse Diglot Map ---
                inv_diglot_map = block.get("mappings", {}).get("basic_target_to_basic_base_inv_diglot", {})
                source_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "basic_target"), None)
                
                if source_tier and inv_diglot_map:
                    for seg_id, entries in inv_diglot_map.items():
                        seg = next((s for s in source_tier.get("segments", []) if s["seg_id"] == seg_id), None)
                        if not seg: continue
                        
                        fused_word_tokens = [tok for tok in seg.get("tokenized_text", []) if tok.get("t") == "w"]
                        
                        for entry in entries:
                            v_token_index = entry[0]
                            lemmas_list = []
                            if v_token_index < len(fused_word_tokens):
                                lemmas_list = fused_word_tokens[v_token_index].get("l", [])
                            
                            entry[1] = lemmas_list

                block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"

        return data