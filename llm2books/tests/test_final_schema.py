# llm2books/tests/test_final_schema.py
import pytest
from llm2books.validator import validate_precomputed_word_counts, ValidationError

@pytest.fixture
def mock_valid_sentence_block():
    """Provides a sentence block with correctly structured mappings."""
    return {
        "s_id": "S1",
        "block_type": "sentence",
        "mappings": {
            "basic_spanish_to_basic_english_diglot": {
                "S1": [
                    # [di, [lemmas], form, viable, eng_wc, [pn_lemmas]]
                    [0, ["uno"], "un", True, 1, []],
                    [1, ["cierto"], "cierto", True, 2, []],
                ]
            },
            "basic_target_to_basic_base_inv_diglot": {
                "S1": [
                    # [v_idx, [lemmas], sub, eng_wc, spa_wc]
                    [0, ["un"], "a", 1, 1],
                    [1, ["cierto", "rey"], "certain king", 2, 2],
                ]
            }
        }
    }

def test_validator_passes_on_correct_word_counts(mock_valid_sentence_block):
    """
    Tests that the validator passes when all mapping tuples have the correct length.
    """
    try:
        validate_precomputed_word_counts(mock_valid_sentence_block)
    except ValidationError as e:
        pytest.fail(f"Validation failed unexpectedly on valid data: {e}")

def test_validator_fails_on_missing_forward_map_count(mock_valid_sentence_block):
    """
    Tests that the validator fails if a forward diglot tuple is missing the word count.
    """
    # Corrupt the data: change the 6-tuple to a 5-tuple
    mock_valid_sentence_block["mappings"]["basic_spanish_to_basic_english_diglot"]["S1"][0] = [0, ["uno"], "un", True, []]
    
    with pytest.raises(ValidationError) as excinfo:
        validate_precomputed_word_counts(mock_valid_sentence_block)
    
    # --- THIS IS THE FIX ---
    # We just check that the error message contains the name of the map, which is specific enough.
    assert "basic_spanish_to_basic_english_diglot" in str(excinfo.value)
    assert "expected 6" in str(excinfo.value)

def test_validator_fails_on_missing_inverse_map_count(mock_valid_sentence_block):
    """
    Tests that the validator fails if an inverse diglot tuple is missing the word count.
    """
    # Corrupt the data: change the 5-tuple to a 4-tuple
    mock_valid_sentence_block["mappings"]["basic_target_to_basic_base_inv_diglot"]["S1"][0] = [0, ["un"], "a", 1]

    with pytest.raises(ValidationError) as excinfo:
        validate_precomputed_word_counts(mock_valid_sentence_block)
        
    # --- THIS IS THE FIX ---
    assert "basic_target_to_basic_base_inv_diglot" in str(excinfo.value)
    assert "expected 5" in str(excinfo.value)