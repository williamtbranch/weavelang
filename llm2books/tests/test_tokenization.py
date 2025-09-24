# llm2books/tests/test_tokenization.py

import pytest
# --- NEW/CHANGED IMPORTS ---
import stanza 
from llm2books.helper import create_golden_token_stream, fuse_tokens, preprocess_for_spacy
# --- END NEW/CHANGED IMPORTS ---

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

def test_golden_token_stream_creation():
    # --- ARRANGE: Load the Stanza model directly ---
    # This removes the dependency on the now-unrelated EnglishStanzaProcessor
    try:
        nlp = stanza.Pipeline('en', processors='tokenize', use_gpu=False, logging_level='WARN')
    except Exception:
        pytest.skip("Stanza English model not available.")
    
    doc = nlp(TEST_SENTENCE)
    stanza_sentence = doc.sentences[0]

    # ACT
    final_tokens = create_golden_token_stream(stanza_sentence)
    
    # ASSERT
    reconstructed = "".join(t['v'] for t in final_tokens)
    assert reconstructed == TEST_SENTENCE
    assert final_tokens == EXPECTED_GOLDEN_STREAM, \
        f"\nExpected:\n{EXPECTED_GOLDEN_STREAM}\nGot:\n{final_tokens}"

# --- The test for the fuser is still relevant and should be kept as is ---
def test_fuser_handles_em_dash_and_parenthesis(spacy_en_model):
    text = "In the car—(he cared for) a mouse lived."
    
    processed_text = preprocess_for_spacy(text)
    doc = spacy_en_model(processed_text)
    
    raw_stream = create_golden_token_stream(doc)
    
    expected_fused_words = ["In", "the", "car", "he", "cared", "for", "a", "mouse", "lived"]
    fused_stream = fuse_tokens(raw_stream)
    actual_fused_words = [t['v'] for t in fused_stream if t['t'] == 'w']
    
    assert actual_fused_words == expected_fused_words