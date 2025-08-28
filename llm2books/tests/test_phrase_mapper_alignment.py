import pytest
from llm2books.phrase_mapper_helpers import parse_llm_phrase_map_to_atoms, SemanticAtom
from llm2books.validator import ValidationError

# --- Test Case 1: The Possessive 's Bug ---
@pytest.fixture
def spacy_tokens_possessive():
    # Ground truth from SpaCy for "...our knight's misfortune..."
    return [
        {'t': 'w', 'v': 'our', 'di': 4},
        {'t': 'w', 'v': 'knight', 'di': 5},
        {'t': 'w', 'v': "'s", 'di': 6},
        {'t': 'w', 'v': 'misfortune', 'di': 7},
    ]

def test_aligns_possessive_phrase_correctly(spacy_tokens_possessive):
    """
    Ensures the parser can correctly align an LLM phrase like "knight's"
    with the two separate tokens SpaCy produces ('knight', "'s").
    """
    llm_map = [
        "our -> nuestro",
        "knight's -> caballero", # The problematic phrase
        "misfortune -> desgracia"
    ]
    
    atoms = parse_llm_phrase_map_to_atoms(llm_map, spacy_tokens_possessive)
    
    assert len(atoms) == 3
    assert atoms[1].en_words == ["knight", "'s"]
    assert atoms[1].es_phrase == "caballero"
    assert atoms[1].di == 5 # DI of the first token in the phrase

# --- Test Case 2: The Hyphenated Word Bug ---
@pytest.fixture
def spacy_tokens_hyphenated():
    # Ground truth from SpaCy for "...never-before-imagined adventure..."
    return [
        {'t': 'w', 'v': 'never', 'di': 10},
        {'t': 'w', 'v': 'before', 'di': 11},
        {'t': 'w', 'v': 'imagined', 'di': 12},
        {'t': 'w', 'v': 'adventure', 'di': 13},
    ]

def test_aligns_hyphenated_phrase_correctly(spacy_tokens_hyphenated):
    """
    Ensures the parser can correctly align an LLM phrase like "never-before-imagined"
    with the multiple tokens SpaCy produces.
    """
    llm_map = [
        "never-before-imagined -> jamás imaginada", # The problematic phrase
        "adventure -> aventura"
    ]
    
    atoms = parse_llm_phrase_map_to_atoms(llm_map, spacy_tokens_hyphenated)
    
    assert len(atoms) == 2
    assert atoms[0].en_words == ["never", "before", "imagined"]
    assert atoms[0].es_phrase == "jamás imaginada"
    assert atoms[0].di == 10