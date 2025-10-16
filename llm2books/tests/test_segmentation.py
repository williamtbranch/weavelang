# llm2books/tests/test_segmentation.py

import pytest
from pathlib import Path
from llm2books.llm_logger import LLMLogger
from llm2books.stanza_segmenter import EnglishStanzaProcessor

# --- Test Case 1 (Happy Path) ---
TEST_SENTENCE_S16 = "“How about if I sleep a little bit longer and forget all this nonsense”, he thought, but that was something he was unable to do because he was used to sleeping on his right, and in his present state couldn’t get into that position."
EXPECTED_SEGMENTS_S16 = [
    '“How about if I sleep a little bit longer ', 
    'and forget all this nonsense”, ', 
    'he thought, but that was something ', 
    'he was unable to do ', 
    'because he was used to sleeping ', 
    'on his right, and in his present state ', 
    'couldn’t get into that position.'
]

def test_llm_segmentation_logic_happy_path():
    """
    An integration test for the LLM segmentation pipeline happy path.
    """
    # ARRANGE
    mock_config = {"stages": {"Segmenter": {"primary_model": "haiku"}}, "models": {"haiku": {"name": "claude-3-haiku-20240307"}}}
    mock_logger = LLMLogger(Path("test_temp_logs"))
    
    processor = EnglishStanzaProcessor(mock_config, mock_logger)

    # ACT
    # --- FIX: Add a mock s_id to the function call ---
    final_segments = processor.segment_sentence(TEST_SENTENCE_S16, s_id="S16_TEST")

    # ASSERT
    assert final_segments == EXPECTED_SEGMENTS_S16, \
        f"\nExpected:\n{EXPECTED_SEGMENTS_S16}\nGot:\n{final_segments}"

# --- Test Case 2 (Failing Regression Test for Possessive Bug) ---
TEST_SENTENCE_FRANK = "Frank set his eldest son to watch; but about twelve o'clock Frank's son fell asleep, and in the morning another of the apples was missing."
MOCK_LLM_RESPONSE_FRANK = """
Frank set his eldest son to watch;
but about twelve o'clock
Frank's son fell asleep,
and in the morning
another of the apples was missing.
"""
# This is what the FINAL segments should look like, with punctuation and spacing preserved.
EXPECTED_SEGMENTS_FRANK = [
    "Frank set his eldest son to watch; ",
    "but about twelve o'clock ",
    "Frank's son fell asleep, and in the morning ",
    "another of the apples was missing."
]

def test_llm_segmentation_handles_possessives_at_boundary(monkeypatch):
    """
    A regression test to ensure that the "slicing" logic does not split
    words like "Frank's" across segment boundaries. It uses a mocked LLM response.
    """
    # ARRANGE
    mock_config = {"stages": {"Segmenter": {"primary_model": "haiku"}}, "models": {"haiku": {"name": "claude-3-haiku-20240307"}}}
    mock_logger = LLMLogger(Path("test_temp_logs"))
    processor = EnglishStanzaProcessor(mock_config, mock_logger)

    # Mock the LLM client's 'create' method to return a predictable response
    class MockMessage:
        def __init__(self, text):
            self.text = text
    class MockContent:
        def __init__(self, text):
            self.content = [MockMessage(text)]

    monkeypatch.setattr(processor.llm_client.messages, "create", lambda **kwargs: MockContent(MOCK_LLM_RESPONSE_FRANK))

    # ACT
    # --- FIX: Add a mock s_id to the function call ---
    final_segments = processor.segment_sentence(TEST_SENTENCE_FRANK, s_id="FRANK_TEST")

    # ASSERT
    # This assertion WILL FAIL with the current buggy slicer logic.
    assert final_segments == EXPECTED_SEGMENTS_FRANK, \
        f"\nExpected:\n{EXPECTED_SEGMENTS_FRANK}\nGot:\n{final_segments}"

def test_slicer_achieves_lossless_reconstruction(monkeypatch):
    """
    This test isolates the slicer (_get_initial_segments_from_llm).
    It asserts that joining the segments produced by the slicer perfectly
    reconstructs the original input sentence, proving that no characters
    were dropped or duplicated during the slicing process.
    """
    # ARRANGE
    mock_config = {"stages": {"Segmenter": {"primary_model": "haiku"}}, "models": {"haiku": {"name": "claude-3-haiku-20240307"}}}
    mock_logger = LLMLogger(Path("test_temp_logs"))
    processor = EnglishStanzaProcessor(mock_config, mock_logger)

    # Mock the LLM to provide a consistent, known-problematic response
    class MockMessage:
        def __init__(self, text): self.text = text
    class MockContent:
        def __init__(self, text): self.content = [MockMessage(text)]
    monkeypatch.setattr(processor.llm_client.messages, "create", lambda **kwargs: MockContent(MOCK_LLM_RESPONSE_FRANK))

    # ACT
    # We call the private slicer method directly, bypassing the merger.
    # --- FIX: Add a mock s_id to the function call ---
    initial_segments = processor._get_initial_segments_from_llm(TEST_SENTENCE_FRANK, s_id="FRANK_TEST")
    
    # Reconstruct the sentence by joining the raw sliced output.
    reconstructed_text = "".join(initial_segments)

    # ASSERT
    # This assertion will fail with the buggy word-count slicer.
    assert reconstructed_text == TEST_SENTENCE_FRANK, \
        "The sliced segments do not perfectly reconstruct the original sentence."