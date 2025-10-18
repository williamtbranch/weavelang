# Add this to llm2books/tests/test_phrase_mapper_helpers.py

import pytest
from llm2books.phrase_mapper_helpers import refactor_token_stream
from llm2books.validator import ValidationError

# ... any other tests ...

def test_refactor_handles_hyphenated_words_correctly():
    """
    Ensures that a hyphenated word from the LLM is correctly matched
    against a single hyphenated token from the original stream.
    """
    # ARRANGE
    original_tokens = [
        {'t': 'b', 'v': ''},
        {'t': 'w', 'v': 'his', 'di': 3, 'l': ['his']},
        {'t': 'b', 'v': ' '},
        # This is the single token produced by the tokenizer
        {'t': 'w', 'v': 'armour-like', 'di': 4, 'l': ['armour-like']},
        {'t': 'b', 'v': ' '},
        {'t': 'w', 'v': 'back', 'di': 5, 'l': ['back']},
        {'t': 'b', 'v': '.'}
    ]
    # This is the group string from the LLM
    group_strings = ["his", "armour-like", "back"]

    # ACT
    # This call should succeed and not raise a ValidationError
    try:
        new_stream = refactor_token_stream(original_tokens, group_strings)
    except ValidationError as e:
        pytest.fail(f"Validation failed unexpectedly for hyphenated word: {e}")

    # ASSERT
    # The new stream should have fused the tokens correctly.
    # We check that the number of word tokens is as expected.
    word_tokens = [t for t in new_stream if t['t'] == 'w']
    assert len(word_tokens) == 3
    assert word_tokens[1]['v'] == 'armour-like'

def test_refactor_handles_group_with_internal_punctuation():
    """
    A regression test for the S19 'Ay, Dios' bug.
    
    This tests the critical scenario where the LLM provides a punctuation-free
    group (e.g., "Ay Dios") that corresponds to a sequence of tokens in the
    original stream containing punctuation (e.g., 'Ay', ', ', 'Dios').
    
    The validator must correctly match based on word content while preserving
    the interstitial punctuation in the final fused token.
    """
    # ARRANGE
    # A token stream representing: "Ay, Dios! He thought."
    original_tokens = [
        {'t': 'b', 'v': ''},
        {'t': 'w', 'v': 'Ay', 'di': 0, 'l': ['ay']},
        {'t': 'b', 'v': ', '},
        {'t': 'w', 'v': 'Dios', 'di': 1, 'l': ['dios']},
        {'t': 'b', 'v': '! '},
        {'t': 'w', 'v': 'He', 'di': 2, 'l': ['he']},
        {'t': 'b', 'v': ' '},
        {'t': 'w', 'v': 'thought', 'di': 3, 'l': ['think']},
        {'t': 'b', 'v': '.'}
    ]

    # The LLM correctly omits punctuation from its mapping groups.
    group_strings = ["Ay Dios", "He", "thought"]

    # ACT
    # This should succeed with the new word-based validation logic.
    try:
        new_stream = refactor_token_stream(original_tokens, group_strings)
    except ValidationError as e:
        pytest.fail(f"Validation failed on a group with internal punctuation: {e}")

    # ASSERT
    word_tokens = [t for t in new_stream if t['t'] == 'w']
    
    # 1. Assert the structure is correct (3 groups -> 3 word tokens)
    assert len(word_tokens) == 3, "The stream should have been fused into 3 word tokens."

    # 2. Assert that the fused token's value PRESERVED the internal punctuation.
    assert word_tokens[0]['v'] == 'Ay, Dios', "The fused token must contain the original interstitial punctuation."
    
    # 3. Assert that the other tokens are correct.
    assert word_tokens[1]['v'] == 'He'
    assert word_tokens[2]['v'] == 'thought'
    
    # 4. Assert that the final stream can losslessly reconstruct the ORIGINAL text.
    reconstructed_text = "".join(t['v'] for t in new_stream)
    assert reconstructed_text == "Ay, Dios! He thought."