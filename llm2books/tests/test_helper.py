# In llm2books/tests/test_helper.py

import pytest
import spacy # <-- Make sure to add this import
from llm2books.helper import create_v2_token_list, normalize_spanish_lemma # (update with your function names)

# --- Test Suite for Helper Functions ---

# --- Tests for create_v2_token_list ---

def test_create_v2_token_list_handles_punctuation_correctly(spacy_en_model):
    """
    Tests that punctuation like commas and periods are treated as background tokens,
    not word tokens.
    """
    # ARRANGE
    text = "A king had a garden,"
    doc = spacy_en_model(text)
    
    # ACT
    token_list = create_v2_token_list(doc[:])
    
    # ASSERT
    # Expected output: B, W, B, W, B, W, B, W, B
    # [B:""] [W:"A"] [B:" "] [W:"king"] [B:" "] [W:"had"] [B:" "] [W:"a"] [B:" "] [W:"garden"] [B:","]
    # The current buggy version will have 11 tokens, with the last two being W and B.
    # The corrected version should merge the last B and the comma into one.
    
    # Assert that the comma is part of the final background token
    last_token = token_list[-1]
    assert last_token["t"] == "b"
    assert "," in last_token["v"]
    
    # Assert that no word token contains punctuation
    for token in token_list:
        if token["t"] == "w":
            assert token["v"] != ","
            assert token["v"] != "."

    # A more specific assertion for the expected structure
    expected_word_values = ["A", "king", "had", "a", "garden"]
    actual_word_values = [t["v"] for t in token_list if t["t"] == "w"]
    assert actual_word_values == expected_word_values

# --- Add other tests for the helper module here ---