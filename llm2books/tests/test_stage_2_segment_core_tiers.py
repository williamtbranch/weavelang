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
    """Fixture for Stage 2 resources."""
    content_project_dir = Path("/fakedir")
    
    # Create a stage1 input file that has multiple segments
    stage1_content = {
        "book_meta": {},
        "content_blocks": [
            {
                "block_type": "sentence",
                "s_id": "S1",
                "tiers": [
                    {
                        "tier_id": "base",
                        "full_text": "He asked, \"Are you coming?\""
                    },
                    {
                        "tier_id": "advanced_target",
                        "full_text": "Él preguntó: \"¿Vienes?\""
                    }
                ]
            }
        ]
    }
    fs.create_file(
        content_project_dir / "pipeline/stage1/test_book.stage1.json",
        contents=json.dumps(stage1_content)
    )

    return {
        'language_config': MOCK_LANG_CONFIG,
        'content_project_dir': str(content_project_dir),
        'tool_root_dir': Path('/weavelang'),
        'spacy_models': {"en": spacy_en_model, "es": spacy_es_model}
    }

class TestStage2SegmentCoreTiers:
    def test_stage2_produces_correct_boundaries_and_tokens(self, mock_stage2_resources):
        """
        Integration test for Stage 2.
        Asserts that it correctly segments, standardizes boundaries, and tokenizes.
        """
        # ARRANGE
        book_stem = "test_book"
        mock_args = MagicMock()
        
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

        # 1. Assert Advanced Tier (text-only, correct boundaries)
        assert adv_tier["segments"][0]["text"] == "Él preguntó: "
        assert adv_tier["segments"][1]["text"] == "\"¿Vienes?\""

        # 2. Assert Base Tier (tokenized, correct boundaries)
        base_seg1_tokens = base_tier["segments"][0]["tokenized_text"]
        base_seg2_tokens = base_tier["segments"][1]["tokenized_text"]
        
        # Check last token of first segment
        assert base_seg1_tokens[-1]["t"] == "b"
        assert base_seg1_tokens[-1]["v"] == ", "
        
        # Check first token of second segment
        assert base_seg2_tokens[0]["t"] == "b"
        assert base_seg2_tokens[0]["v"] == "\""

        # 3. Assert validation passes
        validator.validate_full_text_reconstruction(base_tier)