# llm2books/stages/segment_core_tiers.py
from typing import Any, Dict, List

from .base import SpaCyStage, logger
from .. import helper
from .. import standardize
from .. import validator
from ..stanza_segmenter import StanzaLanguageProcessor

class SegmentCoreTiers(SpaCyStage):
    """
    Stage 2: Segments core tiers using a Stanza constituency parser, tokenizes
    the results, and standardizes boundaries.
    """
    def __init__(self, book_stem: str, cli_args: Any, common_resources: Dict[str, Any]):
        super().__init__(book_stem, cli_args, common_resources, stage_number=2, stage_name="SegmentCoreTiers")

    def _process_data(self, data: Dict[str, Any]) -> Dict[str, Any]:
        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                self._process_tier(block, "base")
                self._process_tier(block, "advanced_target")
            block.setdefault("processing_status", {})[f"stage{self.stage_number}"] = "COMPLETED_INTEGRATED"
        return data

    def _process_tier(self, block: Dict[str, Any], tier_id: str):
        lang_config = self.resources["language_config"]
        lang_code = lang_config['base_code'] if tier_id == 'base' else lang_config['target_code']
        
        # Get the correct Stanza and SpaCy models for the current tier's language
        stanza_processor: StanzaLanguageProcessor = self.resources["stanza_processors"].get(lang_code)
        spacy_model = self.resources["spacy_models"].get(lang_code)

        if not stanza_processor or not spacy_model:
            logger.warning(f"Skipping tier '{tier_id}' for s_id {block.get('s_id')}: "
                           f"Missing required language processor for '{lang_code}'.")
            return

        tier = next((t for t in block.get("tiers", []) if t.get("tier_id") == tier_id), None)
        if not tier or not tier.get("full_text"):
            return
        
        # 1. Segment the full text using the new Stanza-based logic
        segment_texts = stanza_processor.segment_sentence(tier["full_text"])
        
        # 2. Convert text segments into our tokenized data structure
        segments: List[Dict] = []
        sentence_di_counter = 0
        for i, seg_text in enumerate(segment_texts):
            # We need a SpaCy doc of just the segment for tokenization
            seg_doc = spacy_model(seg_text)
            token_list = helper.create_v2_token_list(seg_doc[:])
            
            if tier_id == "base":
                for token in token_list:
                    if token.get("t") == "w":
                        token["di"] = sentence_di_counter
                        sentence_di_counter += 1
            
            segments.append({
                "seg_id": f"{'S' if tier_id == 'base' else 'A'}{i+1}",
                "tokenized_text": token_list
            })
            
        # 3. Standardize boundaries (this rule is still useful)
        self._standardize_segment_boundaries(segments)

        tier["segments"] = segments

    def _standardize_segment_boundaries(self, segments: List[Dict[str, Any]]):
        for i in range(len(segments) - 1):
            tokens1 = segments[i]["tokenized_text"]
            tokens2 = segments[i+1]["tokenized_text"]
            if not tokens1 or not tokens2: continue
            
            b1 = tokens1[-1]
            b2 = tokens2[0]
            new_b1_val, new_b2_val = standardize.split_boundary(b1["v"], b2["v"])
            b1["v"] = new_b1_val
            b2["v"] = new_b2_val