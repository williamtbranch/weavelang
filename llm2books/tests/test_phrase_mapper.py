# In llm2books/tests/test_phrase_mapper.py

import pytest
from llm2books.phrase_mapper_helpers import SemanticAtom

# --- THIS IS THE FIX ---
# Import the helper functions from their new, correct location.
from llm2books.phrase_mapper_helpers import parse_llm_phrase_map_to_atoms, _normalize_for_matching, sanitize_atoms
# --- END OF FIX ---


# --- Mock Data for Tests ---

# A realistic token stream for: "The quick brown fox, and the lazy dog."
MOCK_WORD_TOKENS = [
    {'t': 'w', 'v': 'The', 'di': 0}, {'t': 'w', 'v': 'quick', 'di': 1},
    {'t': 'w', 'v': 'brown', 'di': 2}, {'t': 'w', 'v': 'fox'}, # Punctuation is in B tokens now
    {'t': 'w', 'v': 'and', 'di': 4}, {'t': 'w', 'v': 'the', 'di': 5},
    {'t': 'w', 'v': 'lazy', 'di': 6}, {'t': 'w', 'v': 'dog'},
]

# A more realistic full token stream for validation
MOCK_BASE_TIER = {
    "tier_id": "base",
    "full_text": "The quick brown fox, and the lazy dog.",
    "segments": [
        {
            "seg_id": "S1",
            "text": "The quick brown fox, ",
            "tokenized_text": [
                {'t': 'b', 'v': ''}, {'t': 'w', 'v': 'The', 'di': 0},
                {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'quick', 'di': 1},
                {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'brown', 'di': 2},
                {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'fox', 'di': 3},
                {'t': 'b', 'v': ', '},
            ]
        },
        {
            "seg_id": "S2",
            "text": "and the lazy dog.",
            "tokenized_text": [
                {'t': 'b', 'v': ''}, {'t': 'w', 'v': 'and', 'di': 4},
                {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'the', 'di': 5},
                {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'lazy', 'di': 6},
                {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'dog', 'di': 7},
                {'t': 'b', 'v': '.'},
            ]
        },
    ]
}


# --- Test Suite for parse_llm_phrase_map_to_atoms ---

def test_happy_path_simple():
    """Tests a simple 1-to-1 mapping for an entire sentence."""
    raw_map_lines = [
        "The -> El", "quick -> rápido", "brown -> marrón", "fox -> zorro",
        "and -> y", "the -> el", "lazy -> perezoso", "dog -> perro",
    ]
    word_tokens = [t for seg in MOCK_BASE_TIER['segments'] for t in seg['tokenized_text'] if t['t'] == 'w']
    
    expected_atoms = [
        SemanticAtom(di=0, en_words=['The'], es_phrase='El'),
        SemanticAtom(di=1, en_words=['quick'], es_phrase='rápido'),
    ]
    
    result_atoms = parse_llm_phrase_map_to_atoms(raw_map_lines, word_tokens)
    
    assert result_atoms[:2] == expected_atoms
    assert len(result_atoms) == 8

def test_happy_path_multi_word():
    """Tests that multi-word phrases are correctly parsed."""
    raw_map_lines = [
        "The quick brown -> El rápido marrón", "fox -> zorro", "and -> y",
        "the lazy -> el perezoso", "dog -> perro",
    ]
    word_tokens = [t for seg in MOCK_BASE_TIER['segments'] for t in seg['tokenized_text'] if t['t'] == 'w']
    expected_atoms = [
        SemanticAtom(di=0, en_words=['The', 'quick', 'brown'], es_phrase='El rápido marrón'),
        SemanticAtom(di=3, en_words=['fox'], es_phrase='zorro'),
    ]

    result_atoms = parse_llm_phrase_map_to_atoms(raw_map_lines, word_tokens)
    
    assert result_atoms[:2] == expected_atoms
    assert len(result_atoms) == 5

# --- Test Suite for sanitize_atoms ---

def test_sanitize_valid_multiword_atom_passes():
    """A multi-word atom fully inside one segment should be unchanged."""
    atoms = [SemanticAtom(di=0, en_words=["The", "quick", "brown"], es_phrase="El rápido marrón")]
    sanitized = sanitize_atoms("S_TEST", atoms, MOCK_BASE_TIER)
    assert sanitized == atoms

def test_sanitize_single_word_atoms_always_pass():
    """Single-word atoms should never be invalidated."""
    atoms = [
        SemanticAtom(di=3, en_words=["fox"], es_phrase="zorro"),
        SemanticAtom(di=4, en_words=["and"], es_phrase="y"),
    ]
    sanitized = sanitize_atoms("S_TEST", atoms, MOCK_BASE_TIER)
    assert sanitized == atoms

def test_sanitize_cross_segment_atom_is_invalidated():
    """A multi-word atom spanning two segments must be broken down."""
    atoms = [SemanticAtom(di=3, en_words=["fox", "and"], es_phrase="zorro y")]
    expected_sanitized = [
        SemanticAtom(di=3, en_words=["fox"], es_phrase="NO_SUB"),
        SemanticAtom(di=4, en_words=["and"], es_phrase="NO_SUB"),
    ]
    sanitized = sanitize_atoms("S_TEST", atoms, MOCK_BASE_TIER)
    assert sanitized == expected_sanitized

def test_sanitize_internal_punctuation_atom_is_invalidated():
    """A multi-word atom containing a comma between words must be broken down."""
    # Original text: "quick, brown fox"
    tier_with_comma = {
        "tier_id": "base", "segments": [{
            "seg_id": "S1", "tokenized_text": [
                {'t': 'w', 'v': 'quick', 'di': 1}, {'t': 'b', 'v': ', '},
                {'t': 'w', 'v': 'brown', 'di': 2}, {'t': 'b', 'v': ' '},
                {'t': 'w', 'v': 'fox', 'di': 3}
            ]
        }]
    }

    atoms = [
        SemanticAtom(di=1, en_words=["quick", "brown"], es_phrase="rápido, marrón"),
        SemanticAtom(di=3, en_words=["fox"], es_phrase="zorro"),
    ]
    
    expected_sanitized = [
        SemanticAtom(di=1, en_words=["quick"], es_phrase="NO_SUB"),
        SemanticAtom(di=2, en_words=["brown"], es_phrase="NO_SUB"),
        SemanticAtom(di=3, en_words=["fox"], es_phrase="zorro"),
    ]

    sanitized = sanitize_atoms("S_TEST", atoms, tier_with_comma)

    assert sanitized == expected_sanitized