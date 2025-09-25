# Save as: llm2books/tests/test_tokenizer.py
import pytest
from llm2books.helper import create_golden_token_stream, fuse_tokens

# --- This test will FAIL until we install the correct tokenizer ---
def test_spacy_and_stanza_produce_compatible_streams(spacy_en_model):
    # This test proves the tokenizer works for both libraries
    
    # Case 1: SpaCy
    spacy_text = "He said, \"It's great!\""
    spacy_doc = spacy_en_model(spacy_text)
    spacy_stream = create_golden_token_stream(spacy_doc)
    spacy_reconstructed = "".join(t['v'] for t in spacy_stream)
    assert spacy_reconstructed == spacy_text
    
    # Case 2: Stanza
    import stanza
    try:
        stanza_nlp = stanza.Pipeline('en', processors='tokenize,pos', use_gpu=False, logging_level='WARN')
    except Exception:
        pytest.skip("Stanza English model not available.")

    stanza_text = "He said, “It’s great!”" # Using smart quotes
    stanza_doc = stanza_nlp(stanza_text)
    stanza_stream = create_golden_token_stream(stanza_doc.sentences[0])
    stanza_reconstructed = "".join(t['v'] for t in stanza_stream)
    assert stanza_reconstructed == stanza_text

    # ASSERT the final, FUSED AND MERGED output is the same structure
    expected_fused_stream = [
        {'t': 'b', 'v': ''},
        {'t': 'w', 'v': 'He'},
        {'t': 'b', 'v': ' '},
        {'t': 'w', 'v': 'said'},
        {'t': 'b', 'v': ', “'}, # Note: smart quote
        {'t': 'w', 'v': 'It’s'},
        {'t': 'b', 'v': ' '},
        {'t': 'w', 'v': 'great'},
        {'t': 'b', 'v': '!”'}
    ]
    
    # We only check the final Stanza stream against the gold standard
    assert stanza_stream == expected_fused_stream

def test_tokenizer_handles_quote_after_colon(spacy_en_model):
    """
    This is a regression test for a critical bug.
    It ensures that a word starting with a quote, immediately following
    a colon-space, is tokenized correctly and not fused.
    """
    # ARRANGE
    # This text simulates the exact structure that was causing the pipeline to fail.
    text_with_quote = "He said: 'This is a test.'"
    
    # We use the SpaCy model here because it's readily available in the test
    # suite, and the tokenizer logic is supposed to be universal.
    doc = spacy_en_model(text_with_quote)

    # ACT
    # This is the function we are testing.
    final_tokens = create_golden_token_stream(doc)

    # ASSERT
    # We will check the stream for the specific malformed token.
    found_malformed_token = False
    for token in final_tokens:
        if token['t'] == 'w' and token['v'].startswith("'"):
            found_malformed_token = True
            malformed_value = token['v']
            break
    
    assert not found_malformed_token, \
        f"BUG DETECTED: Found a malformed word token starting with a quote: '{malformed_value}'"

    # Also, assert the expected correct structure for clarity.
    expected_words = ["He", "said", "This", "is", "a", "test"]
    actual_words = [t['v'] for t in final_tokens if t['t'] == 'w']
    assert actual_words == expected_words, "The word token stream was not as expected."