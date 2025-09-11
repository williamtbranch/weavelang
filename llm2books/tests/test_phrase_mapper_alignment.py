import pytest
import pprint
from llm2books.phrase_mapper_helpers import align_and_parse_to_atoms, SemanticAtom
from llm2books.validator import ValidationError
from llm2books.helper import create_golden_token_stream, preprocess_for_spacy

@pytest.fixture
def raw_spacy_tokens_possessive():
    return [ {'t': 'w', 'v': 'our', 'di': 0}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'knight', 'di': 1}, {'t': 'b', 'v': ''}, {'t': 'w', 'v': "'s", 'di': 2}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'misfortune', 'di': 3} ]
@pytest.fixture
def raw_spacy_tokens_hyphenated():
    return [ {'t': 'w', 'v': 'never', 'di': 0}, {'t': 'b', 'v': '-'}, {'t': 'w', 'v': 'before', 'di': 1}, {'t': 'b', 'v': '-'}, {'t': 'w', 'v': 'imagined', 'di': 2}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'adventure', 'di': 3} ]
@pytest.fixture
def raw_spacy_tokens_contraction():
    return [ {'t': 'w', 'v': 'I', 'di': 0}, {'t': 'b', 'v': ''}, {'t': 'w', 'v': "'m", 'di': 1}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'half', 'di': 2}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'asleep', 'di': 3} ]

@pytest.fixture
def raw_spacy_tokens_s153(spacy_en_model):
    """A robust fixture that PRE-PROCESSES text before tokenizing."""
    text = "says old Dives, in his red silken wrapper—(he had a redder one afterwards) pooh, pooh!"
    
    processed_text = preprocess_for_spacy(text)
    doc = spacy_en_model(processed_text)
    
    raw_tokens = create_golden_token_stream(doc)
    di_counter = 0
    for token in raw_tokens:
        if token['t'] == 'w':
            token['di'] = di_counter
            di_counter += 1
            
    return raw_tokens

# --- Golden Test Case ---
def test_golden_case_from_dp_aligner():
    spacy_raw_tokens = [
        {'t': 'w', 'v': "apple", 'di': 0}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': "now", 'di': 1}, {'t': 'b', 'v': ' '},
        {'t': 'w', 'v': "and", 'di': 2}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': "go", 'di': 3}, {'t': 'b', 'v': ' '},
        {'t': 'w', 'v': "couldn't", 'di': 4}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': "sam", 'di': 5}, {'t': 'b', 'v': ' '},
        {'t': 'w', 'v': "no-go", 'di': 6}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': "and", 'di': 7},
    ]
    llm_map = [ "apple -> manzana", "now -> ahora", "and -> y", "go -> ir", "couldn't -> no podia", "deep -> profundo", "no-go -> prohibido", "and -> y" ]
    atoms = align_and_parse_to_atoms(llm_map, spacy_raw_tokens)
    assert len(atoms) == 8
    sam_atom = next((a for a in atoms if a.di == 5), None)
    nogo_atom = next((a for a in atoms if a.di == 6), None)
    assert sam_atom is not None and sam_atom.es_phrase == "NO_SUB"
    assert nogo_atom is not None and nogo_atom.es_phrase == "prohibido"

# --- Alignment & Grouping Tests ---
def test_aligns_possessive_phrase_correctly(raw_spacy_tokens_possessive):
    llm_map = ["our -> nuestro", "knight's -> caballero", "misfortune -> desgracia"]
    atoms = align_and_parse_to_atoms(llm_map, raw_spacy_tokens_possessive)
    assert len(atoms) == 3
    assert atoms[1].en_words == ["knight's"]

def test_aligns_hyphenated_phrase_correctly(raw_spacy_tokens_hyphenated):
    llm_map = ["never-before-imagined -> jamás imaginada", "adventure -> aventura"]
    atoms = align_and_parse_to_atoms(llm_map, raw_spacy_tokens_hyphenated)
    assert len(atoms) == 2
    assert atoms[0].en_words == ["never-before-imagined"]

def test_aligns_contraction_phrase_correctly(raw_spacy_tokens_contraction):
    llm_map = ["I'm -> estoy", "half asleep -> medio dormido"]
    atoms = align_and_parse_to_atoms(llm_map, raw_spacy_tokens_contraction)
    assert len(atoms) == 2
    assert atoms[0].en_words == ["I'm"]

def test_parser_handles_em_dash_adjacent_word(raw_spacy_tokens_s153):
    llm_map_lines = [ "says -> dice", "old -> viejo", "Dives -> Dives", "in -> en", "his -> su", "red -> roja", "silken -> de seda", "wrapper -> bata", "he -> tenía", "had -> NO_SUB", "a -> una", "redder -> más roja", "one -> NO_SUB", "afterwards -> después", "pooh -> bah", "pooh -> bah" ]
    atoms = align_and_parse_to_atoms(llm_map_lines, raw_spacy_tokens_s153)
    assert len(atoms) == 16
    wrapper_atom = next((a for a in atoms if a.di == 7), None)
    he_atom = next((a for a in atoms if a.di == 8), None)
    assert wrapper_atom is not None and wrapper_atom.en_words == ['wrapper']
    assert he_atom is not None and he_atom.en_words == ['he']