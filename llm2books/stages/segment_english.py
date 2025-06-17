from typing import Any, Dict

# Use relative import to access the parent directory's modules
from .. import helper
from .base import SpaCyStage, logger


class SegmentEnglish(SpaCyStage):
    """
    Stage 5a: Segments the source 'english_text' into syntactic chunks
    to prepare for L3 simple Spanish translation.
    """

    def __init__(self, book_stem: str, config: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            config=config,
            common_resources=common_resources,
            stage_number=5,
            stage_name="SegmentEnglish",
        )
        self.is_part_a = True

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Uses the generic helper to segment the English source text.
        """
        spacy_en = self.resources.get("spacy_models", {}).get("en")
        if not spacy_en:
            logger.critical(
                "      -> CRITICAL: English SpaCy model not found in common_resources."
            )
            return data

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                source_text = block.get("english_text", "")
                alignments = []
                l3_segments = []

                if source_text.strip():
                    doc = spacy_en(source_text)
                    # Use the centralized helper function with the 'en' language flag
                    final_phrases = helper.segment_text(doc, language="en")

                    for i, phrase in enumerate(final_phrases):
                        sid = f"S{i + 1}"
                        # Prepare the phrase_alignments object
                        alignments.append(
                            {
                                "segment_id": sid,
                                "simple_spanish_text": "",  # To be filled by Stage 5b
                                "english_span_text": phrase,
                            }
                        )
                        # Prepare the simple_spanish_l3_segments object
                        l3_segments.append(
                            {
                                "segment_id": sid,
                                "simple_text": "",  # To be filled by Stage 5b
                            }
                        )

                block["phrase_alignments_l3_to_english"] = alignments
                block["simple_spanish_l3_segments"] = l3_segments

            # Use a specific key for the sub-stage status
            block.setdefault("llm_call_status", {})["stage5a"] = "COMPLETED_SPACY"

        # This stage produces a partial result for stage 5
        data["processing_status"] = "PARTIAL_5A_COMPLETE"
        return data
