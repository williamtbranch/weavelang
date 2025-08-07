# llm2books/stages/segment_core_tiers.py
from typing import Any, Dict, List

from .base import SpaCyStage, logger
from .. import helper
from .. import standardize
from .. import validator

class SegmentCoreTiers(SpaCyStage):
    """
    Stage 2: Segments the core tiers (`base`, `advanced_target`) and standardizes
    the boundaries between segments according to the "Smart Space Boundary" rule.
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
        lang_config = self.resources["language_config"]
        spacy_base = self.resources["spacy_models"][lang_config["base_code"]]
        spacy_target = self.resources["spacy_models"][lang_config["target_code"]]

        for block in data.get("content_blocks", []):
            if block.get("block_type") == "sentence":
                self._process_tier(block, "base", spacy_base, lang_config["base_code"])
                self._process_tier(block, "advanced_target", spacy_target, lang_config["target_code"])
            
            # This is a SpaCy-only stage, so we can mark it as such.
            block.setdefault("processing_status", {})[f"stage{self.stage_number}"] = "COMPLETED_SPACY"
        return data
    def _process_tier(self, block: Dict[str, Any], tier_id: str, spacy_model: Any, lang_code: str):
        """Processes a single tier within a sentence block."""
        tier = next((t for t in block.get("tiers", []) if t.get("tier_id") == tier_id), None)
        if not tier or not tier.get("full_text"):
            return
        
        # --- START DEBUG PRINTS ---
        print(f"\n--- DEBUG: Processing Tier: '{tier_id}' ---")
        print(f"Input full_text: '{tier['full_text']}'")
        
        doc = spacy_model(tier["full_text"])
        segment_spans = helper.segment_text(doc, language=lang_code)

        print(f"1. After segment_text, got {len(segment_spans)} spans:")
        for i, span in enumerate(segment_spans):
            print(f"   - Span {i}: '{span.text_with_ws}'")
        # --- END DEBUG PRINTS ---

        # Create the initial, unstandardized segments
        segments: List[Dict] = []
        sentence_di_counter = 0
        for i, span in enumerate(segment_spans):
            token_list = helper.create_v2_token_list(span)
            
            if tier_id == "base":
                for token in token_list:
                    if token.get("t") == "w":
                        token["di"] = sentence_di_counter
                        sentence_di_counter += 1
            
            segments.append({
                "seg_id": f"{'S' if tier_id == 'base' else 'A'}{i+1}",
                "tokenized_text": token_list
            })
        
        # --- START DEBUG PRINTS ---
        print("2. Before standardizing boundaries:")
        if len(segments) > 1:
            print(f"   - Seg 0 final token: {segments[0]['tokenized_text'][-1]}")
            print(f"   - Seg 1 initial token: {segments[1]['tokenized_text'][0]}")
        # --- END DEBUG PRINTS ---

        # Standardize the boundaries between the newly created segments
        self._standardize_segment_boundaries(segments)

        # --- START DEBUG PRINTS ---
        print("3. After standardizing boundaries:")
        if len(segments) > 1:
            print(f"   - Seg 0 final token: {segments[0]['tokenized_text'][-1]}")
            print(f"   - Seg 1 initial token: {segments[1]['tokenized_text'][0]}")
        print("--- END DEBUG ---")
        # --- END DEBUG PRINTS ---

        tier["segments"] = segments

    def _standardize_segment_boundaries(self, segments: List[Dict[str, Any]]):
        """Applies the split_boundary rule to adjacent segments in-place."""
        for i in range(len(segments) - 1):
            seg1 = segments[i]
            seg2 = segments[i+1]
            
            tokens1 = seg1["tokenized_text"]
            tokens2 = seg2["tokenized_text"]

            if not tokens1 or not tokens2:
                continue

            # Get the boundary tokens (they are guaranteed to be 'b' type)
            b1 = tokens1[-1]
            b2 = tokens2[0]

            # Apply the rule
            new_b1_val, new_b2_val = standardize.split_boundary(b1["v"], b2["v"])

            # Update the tokens in-place
            b1["v"] = new_b1_val
            b2["v"] = new_b2_val