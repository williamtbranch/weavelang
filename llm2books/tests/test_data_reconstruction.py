# In llm2books/tests/test_data_reconstruction.py

import pytest
from llm2books.stages.finalize_simple_target import FinalizeSimpleTarget
from llm2books.stages.base import SpaCyStage # A new stage we will create
from llm2books.validator import ValidationError

# Mock data representing the state of a block after Stage 2
@pytest.fixture
def mock_block_for_s3():
    return {
        "block_type": "sentence", "s_id": "S_TEST",
        "tiers": [
            {
                "tier_id": "simple_target",
                "full_text": "Entonces el segundo hijorecibió la orden de vigilar;y a medianoche.", # The flawed text
                "segments": [
                    {"seg_id": "S1", "text": "Entonces el segundo hijo"},
                    {"seg_id": "S2", "text": "recibió la orden de vigilar;"},
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

# This is a placeholder for the new stage we need to create.
# We will create the file llm2books/stages/finalize_simpler_adv_target.py for it.
class FinalizeSimplerAdvTarget(SpaCyStage):
    def __init__(self, book_stem, cli_args, common_resources):
        super().__init__(book_stem, cli_args, common_resources, 4, "FinalizeSimplerAdvTarget")
    
    def _process_data(self, data):
        # The real logic will go here.
        return data


# Test 1: Target the "mushed words" bug in FinalizeSimpleTarget
def test_finalize_simple_target_reconstructs_text_correctly(mock_block_for_s3, spacy_es_model):
    """
    This test will fail until we fix the text reconstruction in Stage 3.
    It simulates running the stage and then checks if the output has correct spacing.
    """
    # ARRANGE
    # We need to mock the resources the stage expects
    mock_resources = {
        "language_config": {"target_code": "es"},
        "spacy_models": {"es": spacy_es_model}
    }
    # Instantiate the stage we want to test
    stage = FinalizeSimpleTarget("test_book", None, mock_resources)

    # ACT
    # Run the stage's processing logic on our mock data
    processed_block = stage._process_data({"content_blocks": [mock_block_for_s3]})["content_blocks"][0]

    # ASSERT
    processed_tier = processed_block["tiers"][0]
    reconstructed_text_from_segments = "".join(seg['text'] for seg in processed_tier['segments'])
    
    # This assertion will fail because the current logic produces mushed text.
    assert reconstructed_text_from_segments == processed_tier['full_text'], \
        "The concatenation of segment texts should perfectly match the tier's full_text."


# Test 2: Target the missing 'di' keys in the simpler_advanced_target tier
def test_finalize_simpler_adv_target_adds_di_keys(mock_block_for_new_stage, spacy_es_model):
    """
    This test will fail until we create and implement a new stage that adds
    the diglot indices to the simpler_advanced_target tier.
    """
    # ARRANGE
    mock_resources = {
        "language_config": {"target_code": "es"},
        "spacy_models": {"es": spacy_es_model}
    }
    # This stage doesn't exist yet, but we'll create it.
    stage = FinalizeSimplerAdvTarget("test_book", None, mock_resources)

    # ACT
    processed_block = stage._process_data({"content_blocks": [mock_block_for_new_stage]})["content_blocks"][0]

    # ASSERT
    processed_tier = processed_block["tiers"][0]
    all_word_tokens = [
        token for seg in processed_tier['segments'] 
        for token in seg.get('tokenized_text', []) if token['t'] == 'w'
    ]
    
    # Assert that there are word tokens to check
    assert len(all_word_tokens) > 0, "Test setup error: No word tokens found in processed output."

    # This is the assertion that will fail.
    for i, token in enumerate(all_word_tokens):
        assert 'di' in token, f"Word token '{token['v']}' is missing the 'di' key."
        assert token['di'] == i, f"Diglot indices are not sequential. Expected {i}, got {token['di']}."