# llm2books/tests/test_standardize.py
import pytest
from llm2books.standardize import reconstruct_and_separate_segments

def test_reconstruct_adds_missing_spaces():
    """Tests that the function adds spaces between segments that lack them."""
    # ARRANGE
    original_segments = [
        {"lookup_id": "S1_S1", "text": "Original one,"},
        {"lookup_id": "S1_S2", "text": "Original two."}
    ]
    simplified_map = {
        "S1_S1": "Simplified one,", # No trailing space
        "S1_S2": "simplified two."
    }

    # ACT
    new_segments, full_text = reconstruct_and_separate_segments(original_segments, simplified_map)

    # ASSERT
    assert new_segments[0]['text'] == "Simplified one, ", "Should add a space to the first segment"
    assert new_segments[1]['text'] == "simplified two.", "Should not add a space to the last segment"
    assert full_text == "Simplified one, simplified two."

def test_reconstruct_preserves_existing_spaces():
    """Tests that the function does not add extra spaces if they already exist."""
    # ARRANGE
    original_segments = [
        {"lookup_id": "S1_S1", "text": "Original one, "},
        {"lookup_id": "S1_S2", "text": "Original two."}
    ]
    simplified_map = {
        "S1_S1": "Simplified one, ", # Has a trailing space
        "S1_S2": "simplified two."
    }
    
    # ACT
    new_segments, full_text = reconstruct_and_separate_segments(original_segments, simplified_map)
    
    # ASSERT
    assert new_segments[0]['text'] == "Simplified one, ", "Should not add a second space"
    assert full_text == "Simplified one, simplified two."

def test_reconstruct_handles_single_segment():
    """Tests that the function works correctly with a single segment."""
    # ARRANGE
    original_segments = [{"lookup_id": "S1_S1", "text": "Original one."}]
    simplified_map = {"S1_S1": "Simplified one."}

    # ACT
    new_segments, full_text = reconstruct_and_separate_segments(original_segments, simplified_map)

    # ASSERT
    assert new_segments[0]['text'] == "Simplified one.", "Should not add a space to a single segment"
    assert full_text == "Simplified one."