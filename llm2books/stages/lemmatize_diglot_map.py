from typing import Any, Dict

from .base import SpaCyStage, logger
from .. import helper # Import the helper

class LemmatizeDiglotMap(SpaCyStage):
    """Stage 8: Lemmatizes the 'exact_spanish_form' in each diglot map entry."""
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(book_stem, cli_args, common_resources, stage_number=8, stage_name="LemmatizeDiglotMap")

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        spacy_es = self.resources.get("spacy_models", {}).get("es")
        if not spacy_es:
            logger.critical("      -> CRITICAL: Spanish SpaCy model not found.")
            return data

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                for entry in block.get("diglot_map_entries", []):
                    # Use the English word as a fallback for PROPER_NOUN or NO_SUB.
                    spa_lemma = entry.get("english_word", "").lower()
                    if entry.get("note") == "viable":
                        spa_form = entry.get("exact_spanish_form", "")
                        if spa_form:
                            # Normalize the lemma produced by SpaCy
                            doc = spacy_es(spa_form)
                            main_token = next((t for t in doc if not t.is_punct), None)
                            if main_token:
                                spa_lemma = helper.normalize_spanish_lemma(main_token.lemma_)
                            else:
                                spa_lemma = helper.normalize_spanish_lemma(spa_form)
                    entry["spanish_lemma"] = spa_lemma
            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data