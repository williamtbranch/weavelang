import pytest
from llm2books.standardize import smart_match_and_edit # This will fail until we create the function

# Helper to create a simple token stream for tests
def make_stream(b1_text, w_text, b2_text):
    return [
        {"t": "b", "v": b1_text},
        {"t": "w", "v": w_text},
        {"t": "b", "v": b2_text},
    ]

# Helper to find the index of the word token
def get_word_idx(stream):
    for i, token in enumerate(stream):
        if token["t"] == "w":
            return i
    return -1

# --- Test Suite for smart_match_and_edit ---

class TestSmartMatchAndEdit:
    # Our base case from the discussion
    initial_stream = make_stream("ab", "cdef", "gh")
    word_idx = get_word_idx(initial_stream)

    # --- Case 1: No Match ---
    def test_no_match_if_substring_not_found(self):
        result_stream = smart_match_and_edit(self.initial_stream, self.word_idx, "xyz")
        assert result_stream is None, "Should fail if the match string is not a substring of the combined text"

    # --- Case 2: Perfect Match ---
    def test_perfect_match_succeeds_with_no_change(self):
        result_stream = smart_match_and_edit(self.initial_stream, self.word_idx, "cdef")
        assert result_stream is not None, "Perfect match should succeed"
        assert result_stream == self.initial_stream, "Perfect match should not change the stream"

    # --- Case 3: Pull from Left ---
    def test_pull_from_left_succeeds(self):
        result_stream = smart_match_and_edit(self.initial_stream, self.word_idx, "bcdef")
        assert result_stream is not None, "Pull from left should succeed"
        expected_stream = make_stream("a", "bcdef", "gh")
        assert result_stream == expected_stream

    # --- Case 4: Pull from Right ---
    def test_pull_from_right_succeeds(self):
        result_stream = smart_match_and_edit(self.initial_stream, self.word_idx, "cdefg")
        assert result_stream is not None, "Pull from right should succeed"
        expected_stream = make_stream("ab", "cdefg", "h")
        assert result_stream == expected_stream

    # --- Case 5: Pull from Both ---
    def test_pull_from_both_succeeds(self):
        result_stream = smart_match_and_edit(self.initial_stream, self.word_idx, "bcdefg")
        assert result_stream is not None, "Pull from both should succeed"
        expected_stream = make_stream("a", "bcdefg", "h")
        assert result_stream == expected_stream

    # --- Case 6: Push to Both ---
    def test_push_to_both_succeeds(self):
        result_stream = smart_match_and_edit(self.initial_stream, self.word_idx, "d")
        assert result_stream is not None, "Push to both should succeed"
        expected_stream = make_stream("abc", "d", "efgh")
        assert result_stream == expected_stream

    # --- Case 7: Push to Right ---
    def test_push_to_right_succeeds(self):
        result_stream = smart_match_and_edit(self.initial_stream, self.word_idx, "cd")
        assert result_stream is not None, "Push to right should succeed"
        expected_stream = make_stream("ab", "cd", "efgh")
        assert result_stream == expected_stream
        
    # --- Case 8: Push to Left ---
    # This is your `de t -> (abc) (de) (fgh)` case, which is a "push to both"
    # Let's add a true "push to left only" case.
    def test_push_to_left_succeeds(self):
        result_stream = smart_match_and_edit(self.initial_stream, self.word_idx, "ef")
        assert result_stream is not None, "Push to left should succeed"
        expected_stream = make_stream("abcd", "ef", "gh")
        assert result_stream == expected_stream
        
    # --- Additional Edge Cases ---
    def test_fails_if_word_token_not_found(self):
        stream_no_word = [{"t": "b", "v": "abcdefgh"}]
        result_stream = smart_match_and_edit(stream_no_word, 1, "cdef")
        assert result_stream is None, "Should fail if the provided index is not a word token"

    def test_fails_on_empty_pull(self):
        # Cannot pull from an empty background token
        stream = make_stream("", "cdef", "gh")
        word_idx = get_word_idx(stream)
        result_stream = smart_match_and_edit(stream, word_idx, "acdef")
        assert result_stream is None, "Should fail if it requires pulling from an empty background token"