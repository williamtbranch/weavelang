# llm2books/stages/finalize_diglot_map.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper

class FinalizeDiglotMap(SpaCyStage):
    """
    Stage 5: Lemmatizes the 'exact_target_form' in each diglot map entry,
    replacing the 'TBD' placeholder with the correct normalized lemma.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=5,
            stage_name="FinalizeDiglotMap"
        )

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                diglot_map = block.get("mappings", {}).get("simple_target_to_base_diglot", {})
                
                for seg_id, entries in diglot_map.items():
                    for entry in entries:
                        # entry is [base_word_di, "TBD", "exact_target_form", is_viable_bool]
                        is_viable = entry[3]
                        target_form = entry[2]
                        
                        lemma_str = "" # Default to an empty string for non-viable entries
                        if is_viable and target_form:
                            # Process with SpaCy to get the lemma
                            doc = spacy_target(target_form)
                            # Find the first non-punctuation token to get the primary lemma
                            main_token = next((t for t in doc if not t.is_punct), None)
                            
                            if main_token:
                                # Normalize the lemma using our canonical helper function
                                lemma_str = helper.normalize_spanish_lemma(main_token.lemma_)
                            else:
                                # Fallback for single-word, non-lemma forms
                                lemma_str = helper.normalize_spanish_lemma(target_form)
                        
                        # Replace the placeholder "TBD" with the final, normalized lemma string
                        entry[1] = lemma_str
            
            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        return data