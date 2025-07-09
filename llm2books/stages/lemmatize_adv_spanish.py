from typing import Any, Dict

from .base import SpaCyStage, logger
from .. import helper # Import the helper

class LemmatizeAdvSpanish(SpaCyStage):
    """Stage 2: Lemmatizes the 'adv_spanish_full' text for each sentence block."""
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(book_stem, cli_args, common_resources, stage_number=2, stage_name="LemmatizeAdvSpanish")

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        spacy_es = self.resources.get("spacy_models", {}).get("es")
        if not spacy_es:
            logger.critical("      -> CRITICAL: Spanish SpaCy model not found in common_resources.")
            return data

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                adv_spanish_obj = block.setdefault("adv_spanish_full", {})
                adv_spanish_obj.setdefault("lemmas", [])
                source_text = adv_spanish_obj.get("text", "")
                if source_text.strip():
                    doc = spacy_es(source_text)
                    # --- MODIFIED LEMMA EXTRACTION ---
                    raw_lemmas = [token.lemma_ for token in doc if not token.is_punct and not token.is_space and token.pos_ != "PROPN"]
                    normalized_lemmas = [
                        norm_lemma for s in raw_lemmas if (norm_lemma := helper.normalize_spanish_lemma(s))
                    ]
                    adv_spanish_obj["lemmas"] = normalized_lemmas
                else:
                    adv_spanish_obj["lemmas"] = []
            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data