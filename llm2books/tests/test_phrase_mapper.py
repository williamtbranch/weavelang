import pytest
from llm2books.phrase_mapper_helpers import SemanticAtom, sanitize_atoms

# MOCK_BASE_TIER is still needed for the sanitize tests.
MOCK_BASE_TIER = {
    "tier_id": "base", "full_text": "The quick brown fox, and the lazy dog.", "segments": [
        { "seg_id": "S1", "text": "The quick brown fox, ", "tokenized_text": [ {'t': 'b', 'v': ''}, {'t': 'w', 'v': 'The', 'di': 0}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'quick', 'di': 1}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'brown', 'di': 2}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'fox', 'di': 3}, {'t': 'b', 'v': ', '}, ] },
        { "seg_id": "S2", "text": "and the lazy dog.", "tokenized_text": [ {'t': 'b', 'v': ''}, {'t': 'w', 'v': 'and', 'di': 4}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'the', 'di': 5}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'lazy', 'di': 6}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'dog', 'di': 7}, {'t': 'b', 'v': '.'}, ] },
    ]
}

# --- Test Suite for sanitize_atoms (These are still valid and important) ---
def test_sanitize_valid_multiword_atom_passes():
    """A multi-word atom fully inside one segment should be unchanged."""
    atoms = [SemanticAtom(di=0, en_words=["The", "quick", "brown"], es_phrase="El rápido marrón")]
    sanitized = sanitize_atoms("S_TEST", atoms, MOCK_BASE_TIER)
    assert sanitized == atoms

def test_sanitize_single_word_atoms_always_pass():
    """Single-word atoms should never be invalidated."""
    atoms = [ SemanticAtom(di=3, en_words=["fox"], es_phrase="zorro"), SemanticAtom(di=4, en_words=["and"], es_phrase="y"), ]
    sanitized = sanitize_atoms("S_TEST", atoms, MOCK_BASE_TIER)
    assert sanitized == atoms

def test_sanitize_cross_segment_atom_is_invalidated():
    """A multi-word atom spanning two segments must be broken down."""
    atoms = [SemanticAtom(di=3, en_words=["fox", "and"], es_phrase="zorro y")]
    expected_sanitized = [ SemanticAtom(di=3, en_words=["fox"], es_phrase="NO_SUB"), SemanticAtom(di=4, en_words=["and"], es_phrase="NO_SUB"), ]
    sanitized = sanitize_atoms("S_TEST", atoms, MOCK_BASE_TIER)
    assert sanitized == expected_sanitized
    
def test_sanitize_internal_punctuation_atom_is_invalidated():
    """A multi-word atom containing a comma between words must be broken down."""
    tier_with_comma = { "tier_id": "base", "segments": [{"seg_id": "S1", "tokenized_text": [ {'t': 'w', 'v': 'quick', 'di': 1}, {'t': 'b', 'v': ', '}, {'t': 'w', 'v': 'brown', 'di': 2}, {'t': 'b', 'v': ' '}, {'t': 'w', 'v': 'fox', 'di': 3} ]}] }
    atoms = [ SemanticAtom(di=1, en_words=["quick", "brown"], es_phrase="rápido, marrón"), SemanticAtom(di=3, en_words=["fox"], es_phrase="zorro"), ]
    expected_sanitized = [ SemanticAtom(di=1, en_words=["quick"], es_phrase="NO_SUB"), SemanticAtom(di=2, en_words=["brown"], es_phrase="NO_SUB"), SemanticAtom(di=3, en_words=["fox"], es_phrase="zorro"), ]
    sanitized = sanitize_atoms("S_TEST", atoms, tier_with_comma)
    assert sanitized == expected_sanitized