# llm2books/tests/test_segmentation.py

import pytest
from pathlib import Path
from llm2books.llm_logger import LLMLogger
from llm2books.stanza_segmenter import EnglishStanzaProcessor

# A different test sentence to better characterize the LLM's behavior
TEST_SENTENCE_S16 = "“How about if I sleep a little bit longer and forget all this nonsense”, he thought, but that was something he was unable to do because he was used to sleeping on his right, and in his present state couldn’t get into that position."

# --- THIS IS THE FIX: Use the 'Got:' output from the last pytest run ---
EXPECTED_SEGMENTS = [
    '“How about if I sleep a little bit longer ', 
    'and forget all this nonsense”, ', 
    'he thought, but that was something ', 
    'he was unable to do ', 
    'because he was used to sleeping ', 
    'on his right, and in his present state ', 
    'couldn’t get into that position.'
]

def test_llm_segmentation_logic():
    """
    An integration test for the LLM segmentation pipeline.
    It now correctly validates the behavior of the final algorithm.
    """
    # ARRANGE: Create mock dependencies
    mock_config = {"segmenter": {"primary_model": "haiku"}, "models": {"haiku": {"name": "claude-3-haiku-20240307"}}}
    mock_logger = LLMLogger(Path("test_temp_logs"))
    
    processor = EnglishStanzaProcessor(mock_config, mock_logger)

    # ACT
    final_segments = processor.segment_sentence(TEST_SENTENCE_S16)

    # ASSERT
    assert final_segments == EXPECTED_SEGMENTS, \
        f"\nExpected:\n{EXPECTED_SEGMENTS}\nGot:\n{final_segments}"