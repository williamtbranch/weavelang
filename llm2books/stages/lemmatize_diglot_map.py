# llm2books/stages/lemmatize_diglot_map.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper

class LemmatizeDiglotMap(SpaCyStage):
    """
    Stage 8 (V2): Lemmatizes the 'exact_target_form' in each diglot map entry.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(book_stem, cli_args, common_resources, stage_number=8, stage_name="LemmatizeDiglotMap")

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                diglot_map = block.get("mappings", {}).get("simple_target_to_base_diglot", {})
                for seg_id, entries in diglot_map.items():
                    for entry in entries:
                        # entry is [base_word_di, 0, "exact_target_form", is_viable_bool]
                        is_viable = entry[3]
                        target_form = entry[2]
                        
                        lemma_str = "" # Default to empty
                        if is_viable and target_form:
                            doc = spacy_target(target_form)
                            main_token = next((t for t in doc if not t.is_punct), None)
                            if main_token:
                                lemma_str = helper.normalize_spanish_lemma(main_token.lemma_)
                            else:
                                lemma_str = helper.normalize_spanish_lemma(target_form)
                        
                        # Replace the placeholder 0 with the lemma string
                        entry[1] = lemma_str
            
            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data