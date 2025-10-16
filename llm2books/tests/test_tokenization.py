# llm2books/tests/test_tokenization.py
import pytest
from llm2books.helper import create_golden_token_stream

def test_tokenizer_handles_simple_possessive(spacy_en_model):
    text = "Frank's son"
    doc = spacy_en_model(text)
    golden_stream = create_golden_token_stream(doc)
    word_tokens = [t['v'] for t in golden_stream if t['t'] == 'w']
    assert word_tokens == ["Frank's", "son"]

def test_tokenizer_handles_trailing_possessive_smart_quote(spacy_en_model):
    text = "Grimms’ Fairy Tales"
    doc = spacy_en_model(text)
    golden_stream = create_golden_token_stream(doc)
    word_tokens = [t['v'] for t in golden_stream if t['t'] == 'w']
    assert word_tokens == ["Grimms’", "Fairy", "Tales"]

def test_tokenizer_handles_internal_apostrophe(spacy_en_model):
    text = "twelve o'clock"
    doc = spacy_en_model(text)
    golden_stream = create_golden_token_stream(doc)
    word_tokens = [t['v'] for t in golden_stream if t['t'] == 'w']
    assert word_tokens == ["twelve", "o'clock"]

def test_tokenizer_does_not_fuse_opening_quote(spacy_en_model):
    text = "'Go home,' he said."
    doc = spacy_en_model(text)
    golden_stream = create_golden_token_stream(doc)
    first_word_token = next(t for t in golden_stream if t['t'] == 'w')
    assert first_word_token['v'] == "Go"
    assert "'" in golden_stream[0]['v']

def test_tokenizer_handles_quote_after_colon(spacy_en_model):
    text = "He said: 'This is a test.'"
    doc = spacy_en_model(text)
    final_tokens = create_golden_token_stream(doc)
    found_malformed_token = any(t['t'] == 'w' and t['v'].startswith("'") for t in final_tokens)
    assert not found_malformed_token, "BUG DETECTED: Found a malformed word token starting with a quote."
    expected_words = ["He", "said", "This", "is", "a", "test"]
    actual_words = [t['v'] for t in final_tokens if t['t'] == 'w']
    assert actual_words == expected_words
def test_tokenizer_does_not_fuse_trailing_closing_quote(spacy_en_model):
    """
    Ensures that a word is not fused with a closing quote that follows it,
    especially when the quote is followed by other punctuation.
    """
    text = "this charming place’;"
    doc = spacy_en_model(text)
    golden_stream = create_golden_token_stream(doc)
    word_tokens = [t['v'] for t in golden_stream if t['t'] == 'w']
    
    # The word should be "place", not "place’"
    assert word_tokens == ["this", "charming", "place"]
    
    # The quote and semicolon should be in the background tokens
    background_text = "".join(t['v'] for t in golden_stream if t['t'] == 'b')
    assert "’;" in background_text