# In llm2books/tests/test_pre_fusion_pass.py

import pytest
from llm2books.helper import pre_fuse_word_tokens

# --- Test Suite for the New Pre-Fusion Pass ---

def test_pre_fuse_finds_and_fuses_contraction():
    # ARRANGE
    corrupted_stream = [
        {"t": "b", "v": "“"},
        {"t": "w", "v": "What"},
        {"t": "w", "v": "’s"},
        {"t": "b", "v": " "},
        {"t": "w", "v": "happened"},
        {"t": "b", "v": "."},
    ]
    # ACT
    fixed_stream = pre_fuse_word_tokens(corrupted_stream)
    # ASSERT
    word_tokens = [t['v'] for t in fixed_stream if t['t'] == 'w']
    assert word_tokens == ["What’s", "happened"]
    for i in range(len(fixed_stream) - 1):
        assert fixed_stream[i]['t'] != fixed_stream[i+1]['t'], "BWBWB invariant was not restored"

def test_pre_fuse_finds_and_fuses_with_empty_b_token():
    # ARRANGE
    corrupted_stream = [
        {"t": "w", "v": "abc"},
        {"t": "b", "v": ""},
        {"t": "w", "v": "def"},
    ]
    # ACT
    fixed_stream = pre_fuse_word_tokens(corrupted_stream)
    # ASSERT
    word_tokens = [t['v'] for t in fixed_stream if t['t'] == 'w']
    assert word_tokens == ["abcdef"]

def test_pre_fuse_does_NOT_fuse_across_a_space():
    # ARRANGE
    original_stream = [
        {"t": "w", "v": "abc"},
        {"t": "b", "v": " "},
        {"t": "w", "v": "def"},
    ]
    # ACT
    processed_stream = pre_fuse_word_tokens(original_stream)
    # ASSERT
    assert processed_stream == original_stream

def test_pre_fuse_does_NOT_fuse_across_intervening_characters():
    # ARRANGE
    original_stream = [
        {"t": "w", "v": "abc"},
        {"t": "b", "v": "k"},
        {"t": "w", "v": "def"},
    ]
    # ACT
    processed_stream = pre_fuse_word_tokens(original_stream)
    # ASSERT
    assert processed_stream == original_stream

def test_pre_fuse_handles_multiple_fusions_in_one_stream():
    # ARRANGE
    corrupted_stream = [
        {"t": "w", "v": "It"},
        {"t": "w", "v": "'s"},
        {"t": "b", "v": " "},
        {"t": "w", "v": "a"},
        {"t": "b", "v": " "},
        {"t": "w", "v": "don"},
        {"t": "w", "v": "'t"},
        {"t": "b", "v": "-"}, # This is a non-empty 'b' token, fusion should not happen here
        {"t": "w", "v": "miss"},
        {"t": "b", "v": " "},
        {"t": "w", "v": "event"},
    ]
    # ACT
    fixed_stream = pre_fuse_word_tokens(corrupted_stream)
    # ASSERT
    word_tokens = [t['v'] for t in fixed_stream if t['t'] == 'w']
    assert word_tokens == ["It's", "a", "don't", "miss", "event"] # "don't-miss" will not fuse

def test_pre_fuse_returns_original_stream_if_no_fusions_needed():
    # ARRANGE
    valid_stream = [
        {"t": "b", "v": ""},
        {"t": "w", "v": "A"},
        {"t": "b", "v": " "},
        {"t": "w", "v": "valid"},
        {"t": "b", "v": " "},
        {"t": "w", "v": "stream"},
        {"t": "b", "v": "."},
    ]
    # ACT
    processed_stream = pre_fuse_word_tokens(valid_stream)
    # ASSERT
    assert processed_stream == valid_stream