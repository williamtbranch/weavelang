from typing import Any, Dict

# Use relative import to access the parent directory's modules
from .. import helper
from .base import SpaCyStage, logger


class SegmentEnglish(SpaCyStage):
    """
    Stage 5a: Segments the source 'english_text' into syntactic chunks
    to prepare for L3 simple Spanish translation.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=5,
            stage_name="SegmentEnglish",
        )
        self.is_part_a = True

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Uses the generic helper to segment the English source text and then
        performs a defensive merge of punctuation-only segments.
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
                    # Step 1: Get initial segments from the helper
                    initial_phrases = helper.segment_text(doc, language="en")

                    # Step 2: NEW - Perform defensive merge
                    if not initial_phrases:
                        final_phrases = []
                    else:
                        final_phrases = [initial_phrases[0]]
                        for i in range(1, len(initial_phrases)):
                            current_phrase = initial_phrases[i]
                            # Check if the current phrase contains any letters.
                            if any(c.isalpha() for c in current_phrase):
                                final_phrases.append(current_phrase)
                            else:
                                # If it's punctuation-only, merge it with the previous phrase.
                                # The space is important for cases like `" word` -> `,"` ` word`
                                final_phrases[-1] = f"{final_phrases[-1]}{current_phrase}"
                    
                    # Step 3: Create JSON objects from the cleaned-up final_phrases
                    for i, phrase in enumerate(final_phrases):
                        sid = f"S{i + 1}"
                        alignments.append(
                            {
                                "segment_id": sid,
                                "simple_spanish_text": "",
                                "english_span_text": phrase,
                            }
                        )
                        l3_segments.append(
                            {
                                "segment_id": sid,
                                "simple_text": "",
                            }
                        )

                block["phrase_alignments_l3_to_english"] = alignments
                block["simple_spanish_l3_segments"] = l3_segments

            block.setdefault("llm_call_status", {})["stage5a"] = "COMPLETED_SPACY"

        data["processing_status"] = "PARTIAL_5A_COMPLETE"
        return data