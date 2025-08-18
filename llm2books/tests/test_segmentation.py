# llm2books/tests/test_segmentation.py

import pytest
from llm2books.stanza_segmenter import EnglishStanzaProcessor
# The incorrect import of 'align_segment_boundaries' has been removed.

# This is our "golden" test case from the discussion.
TEST_SENTENCE = "Then the fox said, ‘Do not shoot me, for I will give you good counsel; I know what your business is, and that you want to find the golden bird."

# This is the EXACT output we expect after segmentation and boundary alignment.
EXPECTED_SEGMENTS = [
    "Then the fox said, ",
    "‘Do not shoot me, ",
    "for I will give you good counsel; ",
    "I know what your business is, ",
    "and that you want to find the golden bird."
]

def test_stanza_segmentation_and_boundary_alignment():
    """
    An integration test for the full segmentation pipeline.
    It now directly tests the output of the processor's main method.
    """
    # ARRANGE
    processor = EnglishStanzaProcessor()

    # ACT
    # The processor's method is now responsible for the full, correct segmentation.
    # The call to the deleted 'align_segment_boundaries' function has been removed.
    final_segments = processor.segment_sentence(TEST_SENTENCE)

    # ASSERT
    assert final_segments == EXPECTED_SEGMENTS, \
        f"\nExpected:\n{EXPECTED_SEGMENTS}\nGot:\n{final_segments}"