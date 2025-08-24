# In llm2books/tests/test_data_reconstruction.py

import pytest
# --- THIS IS THE FIX ---
# Import the REAL stage classes, not the placeholder
from llm2books.stages.finalize_simple_target import FinalizeSimpleTarget
from llm2books.stages.finalize_simpler_adv_target import FinalizeSimplerAdvTarget
# --- END OF FIX ---
from llm2books.validator import ValidationError

# Mock data representing the state of a block after Stage 2
@pytest.fixture
def mock_block_for_s3():
    return {
        "block_type": "sentence", "s_id": "S_TEST",
        "tiers": [
            {
                "tier_id": "simple_target",
                # The full_text is intentionally wrong to test that our stage fixes it
                "full_text": "Entonces el segundo hijorecibió la orden de vigilar;y a medianoche.",
                "segments": [
                    {"seg_id": "S1", "text": "Entonces el segundo hijo "},
                    {"seg_id": "S2", "text": "recibió la orden de vigilar; "},
                    {"seg_id": "S3", "text": "y a medianoche."}
                ]
            }
        ]
    }

# Mock data representing state after simpler_adv text is generated
@pytest.fixture
def mock_block_for_new_stage():
    return {
        "block_type": "sentence", "s_id": "S_TEST_ADV",
        "tiers": [
            {
                "tier_id": "simpler_advanced_target",
                "full_text": "Entonces se dijo al segundo hijo que cuidara.",
                "segments": [
                    {"seg_id": "A1", "text": "Entonces se dijo al segundo hijo "},
                    {"seg_id": "A2", "text": "que cuidara."}
                ]
            }
        ]
    }

# The placeholder class has been REMOVED.

# Test 1: Target the "mushed words" bug in FinalizeSimpleTarget
def test_finalize_simple_target_reconstructs_text_correctly(mock_block_for_s3, spacy_es_model):
    """
    This test now passes, confirming the fix for the text reconstruction bug.
    """
    # ARRANGE
    mock_resources = {
        "language_config": {"target_code": "es"},
        "spacy_models": {"es": spacy_es_model},
        "content_project_dir": "dummy/path"
    }
    stage = FinalizeSimpleTarget("test_book", None, mock_resources)

    # ACT
    processed_block = stage._process_data({"content_blocks": [mock_block_for_s3]})["content_blocks"][0]

    # ASSERT
    processed_tier = next((t for t in processed_block["tiers"] if t["tier_id"] == "simple_target"), None)
    assert processed_tier is not None, "simple_target tier not found after processing"
    
    reconstructed_text_from_segments = "".join(
        token['v'] 
        for seg in processed_tier['segments'] 
        for token in seg['tokenized_text']
    )
    
    # Assert that the segment text is now space-correct
    assert processed_tier['segments'][0]['text'] == "Entonces el segundo hijo "
    
    # Assert that the full_text was corrected by the stage
    assert reconstructed_text_from_segments == processed_tier['full_text']


# Test 2: Target the missing 'di' keys in the simpler_advanced_target tier
def test_finalize_simpler_adv_target_adds_di_keys(mock_block_for_new_stage, spacy_es_model):
    """
    This test now uses the real stage and should pass.
    """
    # ARRANGE
    mock_resources = {
        "language_config": {"target_code": "es"},
        "spacy_models": {"es": spacy_es_model},
        "content_project_dir": "dummy/path"
    }
    stage = FinalizeSimplerAdvTarget("test_book", None, mock_resources)

    # ACT
    processed_block = stage._process_data({"content_blocks": [mock_block_for_new_stage]})["content_blocks"][0]

    # ASSERT
    processed_tier = next((t for t in processed_block["tiers"] if t["tier_id"] == "simpler_advanced_target"), None)
    assert processed_tier is not None, "simpler_advanced_target tier not found after processing"

    all_word_tokens = [
        token for seg in processed_tier['segments'] 
        for token in seg.get('tokenized_text', []) if token['t'] == 'w'
    ]
    
    assert len(all_word_tokens) > 0, "The stage should have produced word tokens."

    for i, token in enumerate(all_word_tokens):
        assert 'di' in token, f"Word token '{token['v']}' is missing the 'di' key."
        assert token['di'] == i, f"Diglot indices are not sequential. Expected {i}, got {token['di']}."