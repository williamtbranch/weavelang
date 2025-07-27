# llm2books/stages/finalize_simpler_spanish.py
from typing import Any, Dict

from .base import SpaCyStage, logger
from .. import helper # Import the helper

class FinalizeSimplerSpanish(SpaCyStage):
    """Stage 4: Lemmatizes both advanced and simpler segments, and aggregates."""
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(book_stem, cli_args, common_resources, stage_number=4, stage_name="FinalizeSimplerSpanish")

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        spacy_es = self.resources.get("spacy_models", {}).get("es")
        if not spacy_es:
            logger.critical("      -> CRITICAL: Spanish SpaCy model not found in common_resources.")
            return data

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                all_simpler_texts: list[str] = []
                all_simpler_lemmas: list[str] = []
                for seg in block.get("adv_spanish_segments", []):
                    # Lemmatize advanced_text
                    adv_doc = spacy_es(seg.get("advanced_text", ""))
                    # The `and token.pos_ != "PROPN"` check was removed.
                    adv_raw_lemmas =  [token.lemma_ for token in adv_doc if not token.is_punct and not token.is_space and token.pos_ != "PROPN"]
                    seg["advanced_lemmas"] = [norm_lemma for s in adv_raw_lemmas if (norm_lemma := helper.normalize_spanish_lemma(s))]

                    # Lemmatize simpler_text
                    simpler_doc = spacy_es(seg.get("simpler_text", ""))
                    # The `and token.pos_ != "PROPN"` check was removed.
                    simpler_raw_lemmas = [token.lemma_ for token in simpler_doc if not token.is_punct and not token.is_space and token.pos_ != "PROPN"]
                    simpler_lemmas = [norm_lemma for s in simpler_raw_lemmas if (norm_lemma := helper.normalize_spanish_lemma(s))]
                    seg["simpler_lemmas"] = simpler_lemmas

                    simpler_text_content = seg.get("simpler_text", "")
                    if not simpler_lemmas and any(c.isalpha() for c in simpler_text_content):
                        note = f"LemmatizationWarning: Stage 4 found no lemmas for simpler_text: '{simpler_text_content}'"
                        block.setdefault("processing_notes", []).append(note)
                        s_id = block.get('original_sentence_s_id', 'N/A')
                        seg_id = seg.get('segment_id', 'N/A')
                        logger.warning(f"      -> {note} (Book: {self.book_stem}, S_ID: {s_id}, Seg_ID: {seg_id})")
                    
                    all_simpler_texts.append(seg.get("simpler_text", ""))
                    all_simpler_lemmas.extend(simpler_lemmas)

                block["simpler_adv_spanish_full"] = {"text": " ".join(all_simpler_texts).strip(), "lemmas": all_simpler_lemmas}
            block.setdefault("llm_call_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data