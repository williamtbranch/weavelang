# Save as: llm2books/tests/test_tokenizer.py
import pytest
from llm2books.helper import create_golden_token_stream

def test_spacy_tokenizer_produces_correct_stream(spacy_en_model):
    spacy_text = "He said, \"It's great!\""
    spacy_doc = spacy_en_model(spacy_text)
    spacy_stream = create_golden_token_stream(spacy_doc)
    spacy_reconstructed = "".join(t['v'] for t in spacy_stream)
    assert spacy_reconstructed == spacy_text

    expected_fused_stream = [
        {'t': 'b', 'v': ''},
        {'t': 'w', 'v': 'He'},
        {'t': 'b', 'v': ' '},
        {'t': 'w', 'v': 'said'},
        {'t': 'b', 'v': ', "'},
        {'t': 'w', 'v': "It's"},
        {'t': 'b', 'v': ' '},
        {'t': 'w', 'v': 'great'},
        {'t': 'b', 'v': '!"'}
    ]
    assert spacy_stream == expected_fused_stream


def test_tokenizer_handles_quote_after_colon(spacy_en_model):
    text_with_quote = "He said: 'This is a test.'"
    doc = spacy_en_model(text_with_quote)
    final_tokens = create_golden_token_stream(doc)
    
    found_malformed_token = any(t['t'] == 'w' and t['v'].startswith("'") for t in final_tokens)
    assert not found_malformed_token, "BUG DETECTED: Found a malformed word token starting with a quote."

    expected_words = ["He", "said", "This", "is", "a", "test"]
    actual_words = [t['v'] for t in final_tokens if t['t'] == 'w']
    assert actual_words == expected_words