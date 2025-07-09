from typing import Any, Dict

from .base import SpaCyStage, logger
from .. import helper # Import the helper

class LemmatizeSimpleSpanish(SpaCyStage):
    """Stage 6: Lemmatizes the simple Spanish (L3) text."""
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(book_stem, cli_args, common_resources, stage_number=6, stage_name="LemmatizeSimpleSpanish")

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        spacy_es = self.resources.get("spacy_models", {}).get("es")
        if not spacy_es:
            logger.critical("      -> CRITICAL: Spanish SpaCy model not found.")
            return data

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                lemmas_per_segment = {}
                for align in block.get("phrase_alignments_l3_to_english", []):
                    text_to_lemmatize = align.get("simple_spanish_text", "")
                    doc = spacy_es(text_to_lemmatize)
                    raw_lemmas = [token.lemma_ for token in doc if not token.is_punct and not token.is_space and token.pos_ != "PROPN"]
                    lemmas_per_segment[align["segment_id"]] = [norm_lemma for s in raw_lemmas if (norm_lemma := helper.normalize_spanish_lemma(s))]
                block["simple_spanish_l3_lemmas_per_segment"] = lemmas_per_segment

                l3_full_obj = block.setdefault("simple_spanish_l3_full", {})
                full_text_to_lemmatize = l3_full_obj.get("text", "")
                if full_text_to_lemmatize:
                    full_doc = spacy_es(full_text_to_lemmatize)
                    raw_lemmas = [token.lemma_ for token in full_doc if not token.is_punct and not token.is_space and token.pos_ != "PROPN"]
                    l3_full_obj["lemmas"] = [norm_lemma for s in raw_lemmas if (norm_lemma := helper.normalize_spanish_lemma(s))]
                else:
                    l3_full_obj["lemmas"] = []
            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data