# llm2books/tests/test_stage_2_segment_core_tiers.py

import pytest
import json
from pathlib import Path
from unittest.mock import MagicMock

from llm2books.stages.segment_core_tiers import SegmentCoreTiers
from llm2books import helper, standardize, validator

# Minimal mock language_config
MOCK_LANG_CONFIG = {
    "base_code": "en", "target_code": "es",
    "base_spacy_model": "en_core_web_lg",
    "target_spacy_model": "es_core_news_lg",
}

@pytest.fixture
def mock_stage2_resources(fs, spacy_en_model, spacy_es_model):
    """Fixture for Stage 2 resources with multi-word segments."""
    content_project_dir = Path("/fakedir")
    
    # --- THIS IS THE CRITICAL CHANGE ---
    # New sentences that will not produce single-word segments.
    stage1_content = {
        "book_meta": {},
        "content_blocks": [
            {
                "block_type": "sentence",
                "s_id": "S1",
                "tiers": [
                    {
                        "tier_id": "base",
                        "full_text": "He asked his friend, \"Are you coming to the party?\""
                    },
                    {
                        "tier_id": "advanced_target",
                        "full_text": "Él le preguntó a su amigo: \"¿Vienes a la fiesta?\""
                    }
                ]
            }
        ]
    }
    # Create the prerequisite stage 1 file for the test to read
    fs.create_file(
        content_project_dir / "pipeline/stage1/test_book.stage1.json",
        contents=json.dumps(stage1_content)
    )

    return {
        'language_config': MOCK_LANG_CONFIG,
        'content_project_dir': str(content_project_dir),
        'tool_root_dir': Path('/weavelang'),
        'spacy_models': {"en": spacy_en_model, "es": spacy_es_model},
        'stages_config': {}, 'models_config': {}, 'pipeline_config': {}
    }

class TestStage2SegmentCoreTiers:
    def test_stage2_produces_correct_boundaries_and_tokens(self, mock_stage2_resources):
        """
        Integration test for Stage 2.
        Asserts that it correctly segments, standardizes boundaries, and tokenizes,
        now expecting a 3-segment split for the Spanish sentence.
        """
        # ARRANGE
        book_stem = "test_book"
        mock_args = MagicMock()
        mock_args.force_book = None
        
        # ACT
        stage = SegmentCoreTiers(book_stem, mock_args, mock_stage2_resources)
        success = stage.run()
        assert success is True

        # ASSERT
        output_path = Path("/fakedir/pipeline/stage2/test_book.stage2.json")
        assert output_path.exists()
        
        with open(output_path, 'r') as f:
            data = json.load(f)
            
        block = data["content_blocks"][0]
        base_tier = next(t for t in block["tiers"] if t["tier_id"] == "base")
        adv_tier = next(t for t in block["tiers"] if t["tier_id"] == "advanced_target")

        # --- UPDATED ASSERTIONS ---

        # 1. Assert Advanced Tier (Spanish) - EXPECTING 3 SEGMENTS
        # The logic splits on "a su amigo" (preposition) and then merges the single-word
        # segment "¿Vienes?" back into the previous one.
        # Original split: ["Él le preguntó", "a su amigo:", "\"¿Vienes", "a la fiesta?\""]
        # After merge of single-word "Vienes":
        # -> ["Él le preguntó", "a su amigo: \"¿Vienes", "a la fiesta?\""]
        assert len(adv_tier["segments"]) == 3, "Spanish tier should be split into 3 segments"
        
        adv_seg1_tokens = adv_tier["segments"][0]["tokenized_text"]
        adv_seg2_tokens = adv_tier["segments"][1]["tokenized_text"]
        adv_seg3_tokens = adv_tier["segments"][2]["tokenized_text"]

        # Check boundary between seg1 and seg2
        assert adv_seg1_tokens[-1]["v"].endswith(" "), "Boundary between seg1/seg2 should end with a space"
        assert not adv_seg2_tokens[0]["v"].startswith(" "), "Boundary between seg1/seg2 should not start with a space"
        
        # 2. Assert Base Tier (English) - EXPECTING 2 SEGMENTS
        # The English sentence does not have the same prepositional structure,
        # so it should still only split into 2 segments.
        assert len(base_tier["segments"]) == 2, "English tier should be split into 2 segments"
        base_seg1_tokens = base_tier["segments"][0]["tokenized_text"]
        base_seg2_tokens = base_tier["segments"][1]["tokenized_text"]
        
        # Check boundary standardization
        assert base_seg1_tokens[-1]["v"] == ", " # Space is pulled into the first segment
        assert base_seg2_tokens[0]["v"] == "\""  # Second segment starts with the quote

        # 3. Assert validation passes (most critical check)
        validator.validate_full_text_reconstruction(base_tier)
        validator.validate_full_text_reconstruction(adv_tier)
        validator.validate_base_tier_diglot_indices(base_tier)