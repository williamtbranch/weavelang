# llm2books/stages/finalize_diglot_map.py
from typing import Any, Dict
from .base import SpaCyStage, logger
from .. import helper
from .. import validator


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

    #
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
                        
                        lemma_str = "" # Default for non-viable entries
                        if is_viable and target_form:
                            # Use SpaCy to process the specific word form.
                            # Because it's a single word, context isn't an issue here,
                            # so the simpler method is still robust.
                            doc = spacy_target(target_form)
                            main_token = next((t for t in doc if not t.is_punct and not t.is_space), None)
                            
                            if main_token:
                                lemma_str = helper.normalize_spanish_lemma(main_token.lemma_)
                            else: # Fallback for single-word forms that are their own lemma
                                lemma_str = helper.normalize_spanish_lemma(target_form)
                        
                        # Replace the "TBD" placeholder
                        entry[1] = lemma_str
            
            block.setdefault("processing_status", {})[self.stage_name] = "COMPLETED"
        
        # --- (Validation will be added in the next step) ---
        logger.info("      -> Validating diglot map integrity...")
        try:
            for block in data.get("content_blocks", []):
                if block.get("block_type") == "sentence":
                    validator.validate_exhaustive_diglot_mapping(block)
        except validator.ValidationError as e:
            # Re-raise the exception to be caught by the run method
            raise e
        
        return data