

import pytest  # <-- ADD THIS LINE
import json
from pathlib import Path
from unittest.mock import MagicMock
import spacy # <-- ADD THIS LINE

from llm2books.stages.segment_core_tiers import SegmentCoreTiers
from llm2books import helper, standardize, validator

# Minimal mock language_config
MOCK_LANG_CONFIG = {
    "base_code": "en", "target_code": "es",
    "base_spacy_model": "en_core_web_lg",
    "target_spacy_model": "es_core_news_lg",
}


@pytest.fixture
# --- THIS IS THE FIX ---
# We accept the globally-defined SpaCy models as arguments to this fixture.
def mock_stage2_resources(fs, spacy_en_model, spacy_es_model):
    """Fixture for Stage 2 resources using pre-loaded, shared SpaCy models."""
    content_project_dir = Path("/fakedir")
    stage1_content = {
        "book_meta": {}, "content_blocks": [{"block_type": "sentence", "s_id": "S1", "tiers": [
            {"tier_id": "base", "full_text": "He asked his friend, \"Are you coming to the party?\""},
            {"tier_id": "advanced_target", "full_text": "Él le preguntó a su amigo: \"¿Vienes a la fiesta?\""}
        ]}]
    }
    fs.create_file(content_project_dir / "pipeline/stage1/test_book.stage1.json", contents=json.dumps(stage1_content))
    
    mock_stanza_en = MagicMock()
    mock_stanza_en.segment_sentence.return_value = [
        "He asked his friend,",
        "\"Are you coming to the party?\""
    ]
    mock_stanza_es = MagicMock()
    mock_stanza_es.segment_sentence.return_value = [
        "Él le preguntó a su amigo:",
        "\"¿Vienes a la fiesta?\""
    ]

    return {
        'language_config': MOCK_LANG_CONFIG,
        'content_project_dir': str(content_project_dir),
        'tool_root_dir': Path('/weavelang'),
        # Use the models passed into the fixture instead of loading them here.
        'spacy_models': { "en": spacy_en_model, "es": spacy_es_model },
        'stanza_processors': { "en": mock_stanza_en, "es": mock_stanza_es },
        'stages_config': {}, 'models_config': {}, 'pipeline_config': {}
    }



class TestStage2SegmentCoreTiers:
    def test_stage2_produces_correct_stanza_based_segments(self, mock_stage2_resources):
        """
        Integration test for Stage 2 using the new Stanza-based segmentation.
        """
        book_stem = "test_book"
        mock_args = MagicMock(force_book=None)
        
        stage = SegmentCoreTiers(book_stem, mock_args, mock_stage2_resources)
        success = stage.run()
        assert success is True

        output_path = Path("/fakedir/pipeline/stage2/test_book.stage2.json")
        assert output_path.exists()
        
        with open(output_path, 'r') as f:
            data = json.load(f)
            
        block = data["content_blocks"][0]
        base_tier = next(t for t in block["tiers"] if t["tier_id"] == "base")
        adv_tier = next(t for t in block["tiers"] if t["tier_id"] == "advanced_target")

        # Assert that both tiers were correctly split into 2 segments by our mock
        assert len(base_tier["segments"]) == 2
        assert len(adv_tier["segments"]) == 2

        # Assert that the boundary standardization worked correctly on the English tier
        base_seg1_tokens = base_tier["segments"][0]["tokenized_text"]
        assert base_seg1_tokens[-1]["v"] == ", " # The space from the next segment is pulled in

        # Assert that the boundary standardization worked correctly on the Spanish tier
        adv_seg1_tokens = adv_tier["segments"][0]["tokenized_text"]
        assert adv_seg1_tokens[-1]["v"] == ": " # The space is pulled in

        # Final validation check
        validator.validate_full_text_reconstruction(base_tier)
        validator.validate_full_text_reconstruction(adv_tier)