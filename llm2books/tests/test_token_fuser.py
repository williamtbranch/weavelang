import pytest

# This import will fail until we create the function in the next step.
from llm2books.helper import fuse_tokens

# --- Test Suite for the Token Fusing Post-Processor ---

class TestTokenFuser:

    def test_fuses_simple_contraction(self):
        """Tests fusing a common contraction like 'could' + 'n't'."""
        raw_tokens = [
            {'t': 'w', 'v': 'could', 'di': 0},
            {'t': 'b', 'v': ''}, 
            {'t': 'w', 'v': "n't", 'di': 1},
        ]
        expected_fused = [
            {'t': 'w', 'v': "couldn't", 'di': 0},
        ]
        fused = fuse_tokens(raw_tokens)
        # We only compare the 'v' and 'di' for simplicity
        fused_simple = [{'v': t['v'], 'di': t['di']} for t in fused if t['t'] == 'w']
        expected_simple = [{'v': t['v'], 'di': t['di']} for t in expected_fused if t['t'] == 'w']
        assert fused_simple == expected_simple

    def test_fuses_possessive_s(self):
        """Tests fusing a possessive like 'knight' + 's'."""
        raw_tokens = [
            {'t': 'w', 'v': 'knight', 'di': 5},
            {'t': 'b', 'v': ''},
            {'t': 'w', 'v': "'s", 'di': 6},
        ]
        expected_fused = [
            {'t': 'w', 'v': "knight's", 'di': 5},
        ]
        fused = fuse_tokens(raw_tokens)
        fused_simple = [{'v': t['v'], 'di': t['di']} for t in fused if t['t'] == 'w']
        expected_simple = [{'v': t['v'], 'di': t['di']} for t in expected_fused if t['t'] == 'w']
        assert fused_simple == expected_simple

    def test_fuses_hyphenated_words(self):
        """Tests fusing a multi-part hyphenated word."""
        raw_tokens = [
            {'t': 'w', 'v': 'state', 'di': 10},
            {'t': 'b', 'v': '-'},
            {'t': 'w', 'v': 'of', 'di': 11},
            {'t': 'b', 'v': '-'},
            {'t': 'w', 'v': 'the', 'di': 12},
            {'t': 'b', 'v': '-'},
            {'t': 'w', 'v': 'art', 'di': 13},
        ]
        expected_fused = [
            {'t': 'w', 'v': "state-of-the-art", 'di': 10},
        ]
        fused = fuse_tokens(raw_tokens)
        fused_simple = [{'v': t['v'], 'di': t['di']} for t in fused if t['t'] == 'w']
        expected_simple = [{'v': t['v'], 'di': t['di']} for t in expected_fused if t['t'] == 'w']
        assert fused_simple == expected_simple

    def test_does_not_fuse_across_spaces(self):
        """Ensures fusing stops when a space is present in the background token."""
        raw_tokens = [
            {'t': 'w', 'v': 'word1', 'di': 0},
            {'t': 'b', 'v': ' '}, # This space should prevent fusing
            {'t': 'w', 'v': 'word2', 'di': 1},
        ]
        # Expected is the same as the input, no change.
        fused = fuse_tokens(raw_tokens)
        assert len(fused) == 3
        word_tokens = [t for t in fused if t['t'] == 'w']
        assert len(word_tokens) == 2
        assert word_tokens[0]['v'] == 'word1'
        assert word_tokens[1]['v'] == 'word2'

    def test_handles_complex_sentence_correctly(self):
        """An integration test with a mix of normal words and words to be fused."""
        raw_tokens = [
            {'t': 'b', 'v': ''}, {'t': 'w', 'v': 'I', 'di': 0},
            {'t': 'b', 'v': ''}, {'t': 'w', 'v': "'m", 'di': 1},
            {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'the', 'di': 2},
            {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'king', 'di': 3},
            {'t': 'b', 'v': ''}, {'t': 'w', 'v': "'s", 'di': 4},
            {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'right', 'di': 5},
            {'t': 'b', 'v': '-'}, {'t': 'w', 'v': 'hand', 'di': 6},
            {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'man', 'di': 7},
            {'t': 'b', 'v': '.'},
        ]
        
        expected_words = ["I'm", "the", "king's", "right-hand", "man"]
        
        fused = fuse_tokens(raw_tokens)
        
        fused_words = [t['v'] for t in fused if t['t'] == 'w']
        
        assert fused_words == expected_words
        # Check that the diglot index of the first part is preserved
        assert fused[1]['di'] == 0 # I'm
        assert fused[5]['di'] == 3 # king's
        assert fused[7]['di'] == 5 # right-hand