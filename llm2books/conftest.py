# llm2books/tests/conftest.py
import pytest
import spacy

@pytest.fixture(scope="session")
def spacy_en_model():
    """Loads the SpaCy English model once per test session."""
    try:
        return spacy.load("en_core_web_lg", disable=["ner"])
    except IOError:
        pytest.skip("Skipping tests: English SpaCy model 'en_core_web_lg' not found. Run 'python -m spacy download en_core_web_lg'")

@pytest.fixture(scope="session")
def spacy_es_model():
    """Loads the SpaCy Spanish model once per test session."""
    try:
        return spacy.load("es_core_news_lg", disable=["ner"])
    except IOError:
        pytest.skip("Skipping tests: Spanish SpaCy model 'es_core_news_lg' not found. Run 'python -m spacy download es_core_news_lg'")