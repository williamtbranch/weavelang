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