import pytest
from llm2books.helper import create_golden_token_stream
from llm2books.stanza_segmenter import EnglishStanzaProcessor # <-- ADD THIS IMPORT

TEST_SENTENCE = "He said,  “It’s great!”"

EXPECTED_GOLDEN_STREAM = [
    {'t': 'b', 'v': ''},
    {'t': 'w', 'v': 'He'},
    {'t': 'b', 'v': ' '},
    {'t': 'w', 'v': 'said'},
    {'t': 'b', 'v': ',  “'},
    {'t': 'w', 'v': 'It’s'},
    {'t': 'b', 'v': ' '},
    {'t': 'w', 'v': 'great'},
    {'t': 'b', 'v': '!”'},
]

# Note: We no longer need the spacy_en_model fixture here.
def test_golden_token_stream_creation():
    """
    Tests that the state machine correctly creates a perfectly spaced
    BWBWB token stream from a Stanza sentence object.
    """
    # ARRANGE
    # Use the Stanza processor to get the sentence object
    processor = EnglishStanzaProcessor()
    doc = processor.nlp(TEST_SENTENCE)
    stanza_sentence = doc.sentences[0]

    # ACT
    final_tokens = create_golden_token_stream(TEST_SENTENCE, stanza_sentence)

    # ASSERT
    reconstructed = "".join(t['v'] for t in final_tokens)
    assert reconstructed == TEST_SENTENCE

    assert final_tokens == EXPECTED_GOLDEN_STREAM, \
        f"\nExpected:\n{EXPECTED_GOLDEN_STREAM}\nGot:\n{final_tokens}"