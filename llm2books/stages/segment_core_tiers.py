from typing import Any, Dict

from .base import SpaCyStage, logger
from .. import helper
from .. import standardize
from .. import validator

class SegmentCoreTiers(SpaCyStage):
    """
    Stage 2: Segments the core tiers from the initial full text.
    - Tokenizes the 'base' tier into a rich, detailed structure.
    - Segments the 'advanced_target' tier into simple text strings.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(
            book_stem=book_stem,
            cli_args=cli_args,
            common_resources=common_resources,
            stage_number=2,
            stage_name="SegmentCoreTiers"
        )

def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        # ... (lang_config and model loading) ...
        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                # Process advanced_target tier
                adv_target_tier = next((t for t in block.get("tiers", []) if t.get("tier_id") == "advanced_target"), None)
                if adv_target_tier and adv_target_tier.get("full_text"):
                    doc = spacy_target(adv_target_tier["full_text"])
                    segment_spans = helper.segment_text(doc, language=target_lang_code)
                    
                    adv_target_tier["segments"] = []
                    for i, span in enumerate(segment_spans):
                        adv_target_tier["segments"].append({
                            "seg_id": f"A{i+1}",
                            # The text is the span's text including its trailing whitespace.
                            "text": span.text_with_ws,
                            "lemmas": []
                        })
                
                # Process base tier (logic was already correct here)
                base_tier = next((t for t in block.get("tiers", []) if t.get("tier_id") == "base"), None)
                if base_tier and base_tier.get("full_text"):
                    doc = spacy_base(base_tier["full_text"])
                    segment_spans = helper.segment_text(doc, language=base_lang_code)
                    
                    base_tier["segments"] = []
                    sentence_di_counter = 0
                    for i, span in enumerate(segment_spans):
                        token_list = helper.create_v2_token_list(span)
                        
                        for token in token_list:
                            if token.get("t") == "w":
                                token["di"] = sentence_di_counter
                                sentence_di_counter += 1
                        
                        base_tier["segments"].append({
                            "seg_id": f"S{i+1}",
                            "tokenized_text": token_list
                        })
                
                block.setdefault("processing_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data