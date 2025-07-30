# llm2books/stages/segment_base.py

from typing import Any, Dict

from .. import helper
from .base import SpaCyStage, logger

class SegmentBase(SpaCyStage):
    """
    Stage 5a (V2): Segments the source 'base' language text into syntactic chunks
    and tokenizes them into the explicit V2 schema.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=5,
            stage_name="SegmentBase",
        )
        self.is_part_a = True

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Uses SpaCy to segment the base language text and the new V2 tokenizer
        to create the final data structure for the 'base' tier.
        """
        lang_config = self.resources["language_config"]
        base_lang_code = lang_config["base_code"]
        spacy_base = self.resources["spacy_models"][base_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                
                # Find the 'base' tier to populate. In a fresh run, it won't exist yet.
                base_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "base"), None)
                if not base_tier:
                    base_tier = {"tier_id": "base", "segments": []}
                    block.setdefault("tiers", []).append(base_tier)

                source_text = base_tier.get("full_text", "") # Get text from migrated data
                if not source_text:
                    logger.warning(f"Skipping segmentation for S_ID {block.get('s_id')}: base tier has no full_text.")
                    continue

                doc = spacy_base(source_text)
                
                # Use the helper function to get phrase strings
                phrases = helper.segment_text(doc, language=base_lang_code)
                
                # Keep track of our position in the original string
                current_pos = 0
                
                # Clear existing segments to rebuild them
                base_tier["segments"] = []

                for i, phrase_text in enumerate(phrases):
                    seg_id = f"S{i + 1}"
                    
                    # Find the phrase in the original doc to get the SpaCy Span object
                    phrase_span = None
                    for sent in doc.sents:
                        if sent.text.find(phrase_text) != -1:
                           start_char = sent.text.find(phrase_text) + sent.start_char
                           end_char = start_char + len(phrase_text)
                           phrase_span = doc.char_span(start_char, end_char)
                           break
                    
                    if phrase_span is None:
                        logger.warning(f"Could not find span for phrase: '{phrase_text}'. Tokenizing raw string instead.")
                        phrase_doc = spacy_base(phrase_text)
                        tokenized_text = helper.create_v2_token_list(phrase_text, phrase_doc)
                    else:
                        tokenized_text = helper.create_v2_token_list(phrase_span.text, phrase_span.as_doc())

                    # Determine the separator between this segment and the next
                    start_of_next_phrase = (current_pos + len(phrase_text))
                    post_separator = ""
                    if i < len(phrases) - 1:
                        end_of_this_phrase = source_text.find(phrases[i+1], start_of_next_phrase)
                        if end_of_this_phrase != -1:
                           post_separator = source_text[start_of_next_phrase:end_of_this_phrase]

                    base_tier["segments"].append({
                        "seg_id": seg_id,
                        "post_separator": post_separator,
                        "tokenized_text": tokenized_text
                    })
                    
                    current_pos += len(phrase_text) + len(post_separator)

            # Mark this sub-stage as complete
            block.setdefault("llm_call_status", {})["stage5a"] = "COMPLETED_SPACY"

        data["processing_status"] = "PARTIAL_5A_COMPLETE"
        return data