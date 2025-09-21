# llm2books/tests/test_segmentation.py

import pytest
from llm2books.stanza_segmenter import EnglishStanzaProcessor

TEST_SENTENCE = "Then the fox said, ‘Do not shoot me, for I will give you good counsel; I know what your business is, and that you want to find the golden bird."

# --- THIS IS THE FIX ---
# This is the new, correct "golden" output from our final hierarchical algorithm.
EXPECTED_SEGMENTS = [
    'Then the fox said, ‘Do not shoot me,', 
    'for I will give you good counsel;', 
    'I know what your business is,', 
    'and that you want to find the golden bird.'
]
# --- END OF FIX ---

def test_stanza_segmentation_and_boundary_alignment():
    """
    An integration test for the full segmentation pipeline.
    It now correctly validates the behavior of the final algorithm.
    """
    # ARRANGE
    processor = EnglishStanzaProcessor()

    # ACT
    final_segments = processor.segment_sentence(TEST_SENTENCE)

    # ASSERT
    assert final_segments == EXPECTED_SEGMENTS, \
        f"\nExpected:\n{EXPECTED_SEGMENTS}\nGot:\n{final_segments}"