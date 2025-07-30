# llm2books/stages/segment_advanced_target.py
from typing import Any, Dict
from .. import helper
from .base import SpaCyStage, logger

class SegmentAdvancedTarget(SpaCyStage):
    """
    Stage 3a (V2): Segments the advanced target text into syntactic chunks
    and prepares the data for the simplification LLM call in Stage 3b.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=3,
            stage_name="SegmentAdvancedTarget",
        )
        self.is_part_a = True

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        lang_config = self.resources["language_config"]
        target_lang_code = lang_config["target_code"]
        spacy_target = self.resources["spacy_models"][target_lang_code]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                adv_target_tier = next((t for t in block.get("tiers", []) if t["tier_id"] == "advanced_target"), None)
                if not adv_target_tier: continue

                source_text = adv_target_tier.get("full_text", "")
                if not source_text.strip(): continue
                
                doc = spacy_target(source_text)
                phrases = helper.segment_text(doc, language=target_lang_code)

                current_pos = 0
                adv_target_tier["segments"] = [] # Clear any previous data

                for i, phrase_text in enumerate(phrases):
                    phrase_span = self._find_span_in_doc(doc, phrase_text, current_pos)
                    
                    tokenized_text = helper.create_v2_token_list(
                        phrase_span.text, phrase_span.as_doc()
                    ) if phrase_span else []
                    
                    post_separator = self._get_post_separator(source_text, phrase_text, i, phrases, current_pos)

                    adv_target_tier["segments"].append({
                        "seg_id": f"A{i + 1}",
                        "post_separator": post_separator,
                        "tokenized_text": tokenized_text
                    })
                    current_pos += len(phrase_text) + len(post_separator)
                
                # Create the empty simpler_advanced_target tier
                if not any(t["tier_id"] == "simpler_advanced_target" for t in block.get("tiers", [])):
                    block.setdefault("tiers", []).append({
                        "tier_id": "simpler_advanced_target",
                        "full_text": "",
                        "segments": []
                    })
            
            block.setdefault("llm_call_status", {})["stage3a"] = "COMPLETED_SPACY"
        
        data["processing_status"] = "PARTIAL_3A_COMPLETE"
        return data

    # Helper methods to avoid code duplication
    def _find_span_in_doc(self, doc, phrase_text, start_search_pos):
        try:
            start_char = doc.text.index(phrase_text, start_search_pos)
            end_char = start_char + len(phrase_text)
            return doc.char_span(start_char, end_char)
        except ValueError:
            logger.warning(f"Could not find exact span for phrase: '{phrase_text}'")
            return None

    def _get_post_separator(self, source_text, current_phrase, index, all_phrases, current_pos):
        if index < len(all_phrases) - 1:
            next_phrase = all_phrases[index + 1]
            start_of_next = source_text.find(next_phrase, current_pos + len(current_phrase))
            if start_of_next != -1:
                return source_text[current_pos + len(current_phrase):start_of_next]
        return ""