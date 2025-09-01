import pytest
from llm2books.helper import create_golden_token_stream, fuse_tokens, preprocess_for_spacy
from llm2books.stanza_segmenter import EnglishStanzaProcessor

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
    processor = EnglishStanzaProcessor()
    doc = processor.nlp(TEST_SENTENCE)
    stanza_sentence = doc.sentences[0]
    final_tokens = create_golden_token_stream(stanza_sentence)
    reconstructed = "".join(t['v'] for t in final_tokens)
    assert reconstructed == TEST_SENTENCE
    assert final_tokens == EXPECTED_GOLDEN_STREAM, \
        f"\nExpected:\n{EXPECTED_GOLDEN_STREAM}\nGot:\n{final_tokens}"

# --- NEW TEST TO ISOLATE THE S153 BUG ---
def test_fuser_handles_em_dash_and_parenthesis(spacy_en_model):
    text = "In the car—(he cared for) a mouse lived."
    
    # Pre-process the text to ensure correct tokenization by SpaCy
    processed_text = preprocess_for_spacy(text)
    doc = spacy_en_model(processed_text)
    
    raw_stream = create_golden_token_stream(doc)
    
    expected_fused_words = ["In", "the", "car", "he", "cared", "for", "a", "mouse", "lived"]
    fused_stream = fuse_tokens(raw_stream)
    actual_fused_words = [t['v'] for t in fused_stream if t['t'] == 'w']
    
    assert actual_fused_words == expected_fused_words