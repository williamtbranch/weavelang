import pytest
from llm2books.llm_utils import _parse_structured_llm_response, validate_parsed_llm_response

# --- Test for Forward Phrase Map (Stage 3) ---

def test_validator_fails_on_empty_spanish_forward_mapping():
    """
    Tests that our validator catches forward phrase map lines where the Spanish
    translation is missing (e.g., 'talk -> ').
    """
    # ARRANGE: Simulate a bad LLM response for a forward map job (GeneratePhraseMap)
    bad_llm_response = """
S18:
MAPPINGS:
In spite of -> A pesar de
this -> este
talk -> 
VALIDATION: In spite of this talk
"""
    parsed_data = _parse_structured_llm_response(bad_llm_response, ["S18"])
    
    # ACT & ASSERT
    with pytest.raises(ValueError) as excinfo:
        validate_parsed_llm_response(parsed_data, "multi_line")
    
    assert "Found 1 mapping lines with empty translations" in str(excinfo.value)
    assert "talk ->" in str(excinfo.value)

# --- NEW Test for Inverse Phrase Map (Stage 5) ---

def test_validator_fails_on_empty_english_inverse_mapping():
    """
    Tests that our validator also catches inverse phrase map lines where the English
    substitute is missing (e.g., 'conversación -> ').
    """
    # ARRANGE: Simulate a bad LLM response for an inverse map job (GenerateInverseDiglotMap)
    bad_llm_response = """
S18_A1:
MAPPINGS:
A pesar de -> In spite of
este -> this
matrimonio -> marriage
conversación -> 
VALIDATION: some validation text here
"""
    # Note: The ID for inverse maps is per-segment, like "S18_A1"
    parsed_data = _parse_structured_llm_response(bad_llm_response, ["S18_A1"])
    
    # ACT & ASSERT
    with pytest.raises(ValueError) as excinfo:
        validate_parsed_llm_response(parsed_data, "multi_line")
    
    assert "Validation failed for S_ID 'S18_A1'" in str(excinfo.value)
    assert "Found 1 mapping lines with empty translations" in str(excinfo.value)
    assert "conversación ->" in str(excinfo.value)