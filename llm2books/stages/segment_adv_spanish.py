# llm2books/stages/segment_adv_spanish.py

from typing import Any, Dict

# Use relative import to access the parent directory's modules
from .. import helper
from .base import SpaCyStage, logger


class SegmentAdvSpanish(SpaCyStage):
    """
    Stage 3a: Segments the advanced Spanish text into syntactic chunks.
    This prepares the data for the simplification LLM call in Stage 3b.
    """

    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=3,
            stage_name="SegmentAdvSpanish",
        )
        self.is_part_a = True

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Uses the helper function to segment the advanced Spanish text.
        """
        spacy_es = self.resources.get("spacy_models", {}).get("es")
        if not spacy_es:
            logger.critical(
                "      -> CRITICAL: Spanish SpaCy model not found in common_resources."
            )
            return data

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                source_text = block.get("adv_spanish_full", {}).get("text", "")
                segments = []

                if source_text.strip():
                    doc = spacy_es(source_text)
                    # Use the centralized helper function with the 'es' language flag
                    final_phrases = helper.segment_text(doc, language="es")

                    for i, phrase in enumerate(final_phrases):
                        # Prepare the adv_spanish_segments object
                        segments.append(
                            {
                                "segment_id": f"A{i + 1}",
                                "advanced_text": phrase,
                                "simpler_text": "",  # To be filled by Stage 3b
                                "advanced_lemmas": [], # To be filled by Stage 4
                                "simpler_lemmas": [],  # To be filled by Stage 4
                            }
                        )

                block["adv_spanish_segments"] = segments

            # Use a specific key for the sub-stage status
            block.setdefault("llm_call_status", {})["stage3a"] = "COMPLETED_SPACY"
        
        # This stage produces a partial result for stage 3
        data["processing_status"] = "PARTIAL_3A_COMPLETE"
        return data