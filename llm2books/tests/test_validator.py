import pytest
from llm2books.validator import (
    validate_full_text_reconstruction, 
    validate_bwbw_invariant, 
    validate_base_tier_diglot_indices,
    validate_exhaustive_diglot_mapping,
    validate_exhaustive_inverse_diglot_mapping,
    ValidationError
)

def test_reconstruction_happy_path():
    """
    Tests that no error is raised when tokens perfectly reconstruct the full_text.
    """
    mock_tier = {
        "tier_id": "base",
        "full_text": "“Who are you?”",
        "segments": [
            {
                "seg_id": "S1",
                "tokenized_text": [
                    { "t": "b", "v": "“" },
                    { "t": "w", "v": "Who" },
                    { "t": "b", "v": " " },
                    { "t": "w", "v": "are" },
                    { "t": "b", "v": " " },
                    { "t": "w", "v": "you" },
                    { "t": "b", "v": "?”" }
                ]
            }
        ]
    }
    
    # This should run without raising an exception
    try:
        validate_full_text_reconstruction(mock_tier)
    except ValidationError as e:
        pytest.fail(f"Validation raised an unexpected error: {e}")

def test_reconstruction_fails_on_mismatch():
    """
    Tests that a ValidationError is raised when token text does not match full_text.
    """
    mock_tier = {
        "tier_id": "base",
        "full_text": "This text does not match.",
        "segments": [
            {
                "seg_id": "S1",
                "tokenized_text": [
                    { "t": "b", "v": "The tokens are different." }
                ]
            }
        ]
    }
    
    # Assert that our function raises the specific error we expect
    with pytest.raises(ValidationError) as excinfo:
        validate_full_text_reconstruction(mock_tier)
    
    # Optionally, check the error message
    assert "Lossless reconstruction failed" in str(excinfo.value)
    assert "base" in str(excinfo.value) # Should mention the tier_id

def test_bwbw_happy_path():
    """Tests that valid B, BWB, and BWBWB patterns pass."""
    valid_patterns = [
        [{"t": "b", "v": ""}],
        [{"t": "b", "v": " "}, {"t": "w", "v": "word"}, {"t": "b", "v": "."}],
        [{"t": "b", "v": ""}, {"t": "w", "v": "a"}, {"t": "b", "v": " "}, {"t": "w", "v": "b"}, {"t": "b", "v": ""}],
    ]
    for pattern in valid_patterns:
        try:
            validate_bwbw_invariant(pattern, "test_tier", "S1")
        except ValidationError as e:
            pytest.fail(f"BWBWB happy path failed unexpectedly: {e}")

def test_bwbw_fails_on_starts_with_word():
    """Tests that a token list starting with a word token fails."""
    invalid_pattern = [{"t": "w", "v": "word"}, {"t": "b", "v": " "}]
    with pytest.raises(ValidationError) as excinfo:
        validate_bwbw_invariant(invalid_pattern, "test_tier", "S1")
    assert "must start with a background ('b') token" in str(excinfo.value)

def test_bwbw_fails_on_ends_with_word():
    """Tests that a token list ending with a word token fails."""
    invalid_pattern = [{"t": "b", "v": " "}, {"t": "w", "v": "word"}]
    with pytest.raises(ValidationError) as excinfo:
        validate_bwbw_invariant(invalid_pattern, "test_tier", "S1")
    assert "must end with a background ('b') token" in str(excinfo.value)

def test_bwbw_fails_on_consecutive_words():
    """Tests that two consecutive word tokens fail."""
    invalid_pattern = [{"t": "b", "v": ""}, {"t": "w", "v": "a"}, {"t": "w", "v": "b"}, {"t": "b", "v": ""}]
    with pytest.raises(ValidationError) as excinfo:
        validate_bwbw_invariant(invalid_pattern, "test_tier", "S1")
    assert "consecutive tokens of the same type ('w')" in str(excinfo.value)

def test_bwbw_fails_on_consecutive_backgrounds():
    """Tests that two consecutive background tokens fail."""
    invalid_pattern = [{"t": "b", "v": " "}, {"t": "b", "v": "."}, {"t": "w", "v": "a"}, {"t": "b", "v": ""}]
    with pytest.raises(ValidationError) as excinfo:
        validate_bwbw_invariant(invalid_pattern, "test_tier", "S1")
    assert "consecutive tokens of the same type ('b')" in str(excinfo.value)

def test_di_happy_path():
    """Tests a base tier with perfect, sequential, unique di values."""
    mock_base_tier = {
        "tier_id": "base",
        "segments": [
            {"tokenized_text": [
                {"t": "b", "v": ""}, {"t": "w", "v": "a", "di": 0},
                {"t": "b", "v": " "}, {"t": "w", "v": "b", "di": 1},
            ]},
            {"tokenized_text": [
                {"t": "b", "v": ""}, {"t": "w", "v": "c", "di": 2},
            ]}
        ]
    }
    try:
        validate_base_tier_diglot_indices(mock_base_tier)
    except ValidationError as e:
        pytest.fail(f"DI validation failed unexpectedly: {e}")

def test_di_fails_on_missing_di_key():
    """Tests that a word token missing the 'di' key raises an error."""
    mock_base_tier = {
        "tier_id": "base",
        "segments": [{"tokenized_text": [{"t": "b", "v": ""}, {"t": "w", "v": "a"}]}] # "a" is missing "di"
    }
    with pytest.raises(ValidationError) as excinfo:
        validate_base_tier_diglot_indices(mock_base_tier)
    assert "missing 'di' key" in str(excinfo.value)

def test_di_fails_on_non_sequential_di():
    """Tests that 'di' values that are not perfectly sequential (0, 1, 2...) raise an error."""
    mock_base_tier = {
        "tier_id": "base",
        "segments": [{"tokenized_text": [
            {"t": "b", "v": ""}, {"t": "w", "v": "a", "di": 0},
            {"t": "b", "v": " "}, {"t": "w", "v": "b", "di": 2}, # Skips 1
        ]}]
    }
    with pytest.raises(ValidationError) as excinfo:
        validate_base_tier_diglot_indices(mock_base_tier)
    assert "was not sequential" in str(excinfo.value)
    assert "Expected 1, but got 2" in str(excinfo.value)

def test_di_fails_on_duplicate_di():
    """Tests that duplicate 'di' values across segments raise an error."""
    mock_base_tier = {
        "tier_id": "base",
        "segments": [
            {"tokenized_text": [{"t": "b", "v": ""}, {"t": "w", "v": "a", "di": 0}]},
            {"tokenized_text": [{"t": "b", "v": ""}, {"t": "w", "v": "b", "di": 0}]} # Duplicate di: 0
        ]
    }
    with pytest.raises(ValidationError) as excinfo:
        validate_base_tier_diglot_indices(mock_base_tier)
    assert "Duplicate 'di' value found: 0" in str(excinfo.value)

def test_diglot_mapping_happy_path():
    """Tests that validation passes when word count matches mapping entry count."""
    mock_sentence_block = {
        "s_id": "S1",
        "tiers": [{
            "tier_id": "base",
            "segments": [{
                "seg_id": "S1",
                "tokenized_text": [
                    {"t": "b", "v": ""}, {"t": "w", "v": "word1"},
                    {"t": "b", "v": " "}, {"t": "w", "v": "word2"},
                ]
            }]
        }],
        "mappings": { "simple_target_to_base_diglot": {
            "S1": [ [0, "l1", "f1", True], [1, "l2", "f2", True] ]
        }}
    }
    try:
        validate_exhaustive_diglot_mapping(mock_sentence_block)
    except ValidationError as e:
        pytest.fail(f"Exhaustive diglot validation failed unexpectedly: {e}")

def test_diglot_mapping_fails_on_mismatched_counts():
    """Tests that an error is raised if word count and mapping count differ."""
    mock_sentence_block = {
        "s_id": "S1",
        "tiers": [{
            "tier_id": "base",
            "segments": [{
                "seg_id": "S1",
                "tokenized_text": [
                    {"t": "b", "v": ""}, {"t": "w", "v": "word1"}, # One word
                ]
            }]
        }],
        "mappings": { "simple_target_to_base_diglot": {
             # Two mapping entries
            "S1": [ [0, "l1", "f1", True], [1, "l2", "f2", True] ]
        }}
    }
    with pytest.raises(ValidationError) as excinfo:
        validate_exhaustive_diglot_mapping(mock_sentence_block)
    assert "Expected 1 mapping entries, but found 2" in str(excinfo.value)
    assert "S1.S1" in str(excinfo.value) # Should mention sentence and segment

def test_diglot_mapping_handles_missing_map_gracefully():
    """
    Tests that no error is raised if a segment has words but no entry
    in the mappings dict at all (assumes it's intentional).
    """
    mock_sentence_block = {
        "s_id": "S1",
        "tiers": [{"tier_id": "base", "segments": [{"seg_id": "S1", "tokenized_text": [{"t": "w"}]}]}],
        "mappings": { "simple_target_to_base_diglot": {
            "S2": [] # Has a mapping for a different segment, but not S1
        }}
    }
    try:
        validate_exhaustive_diglot_mapping(mock_sentence_block)
    except ValidationError as e:
        pytest.fail(f"Validation failed on a missing map entry: {e}")

def test_inverse_diglot_mapping_happy_path():
    """Tests that validation passes when word count matches inv diglot mapping."""
    mock_sentence_block = {
        "s_id": "S1",
        "tiers": [{
            "tier_id": "simpler_advanced_target",
            "segments": [{
                "seg_id": "A1",
                "tokenized_text": [
                    {"t": "w", "v": "word1"}, {"t": "w", "v": "word2"},
                ]
            }]
        }],
        "mappings": { "simpler_adv_target_to_base_inv_diglot": {
            "A1": [ [0, "l1", "sub1"], [1, "l2", "sub2"] ]
        }}
    }
    try:
        validate_exhaustive_inverse_diglot_mapping(mock_sentence_block)
    except ValidationError as e:
        pytest.fail(f"Exhaustive inverse diglot validation failed unexpectedly: {e}")

def test_inverse_diglot_mapping_fails_on_mismatch():
    """Tests that an error is raised if inv diglot word/mapping counts differ."""
    mock_sentence_block = {
        "s_id": "S1",
        "tiers": [{
            "tier_id": "simpler_advanced_target",
            "segments": [{
                "seg_id": "A1",
                "tokenized_text": [ {"t": "w", "v": "word1"} ] # One word
            }]
        }],
        "mappings": { "simpler_adv_target_to_base_inv_diglot": {
            "A1": [ [0, "l1", "sub1"], [1, "l2", "sub2"] ] # Two mappings
        }}
    }
    with pytest.raises(ValidationError) as excinfo:
        validate_exhaustive_inverse_diglot_mapping(mock_sentence_block)
    assert "Expected 1 mapping entries, but found 2" in str(excinfo.value)
    assert "S1.A1" in str(excinfo.value)