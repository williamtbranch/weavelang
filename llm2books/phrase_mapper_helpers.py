import re
from typing import List, Dict, Any

from .stages.base import logger # Import the pipeline logger
from .validator import ValidationError # Import the custom exception

class SemanticAtom:
    """
    Represents a single, atomic semantic unit mapped between languages.
    This can be a single word or a multi-word phrase.
    """
    def __init__(self, di: int, en_words: List[str], es_phrase: str):
        self.di: int = di  # The diglot_index of the FIRST English word in the atom
        self.en_words: List[str] = en_words  # The list of original English words
        self.es_phrase: str = es_phrase  # The mapped Spanish phrase

    def __repr__(self) -> str:
        return f"Atom(di={self.di}, en='{' '.join(self.en_words)}', es='{self.es_phrase}')"

    def __eq__(self, other):
        if not isinstance(other, SemanticAtom):
            return NotImplemented
        return self.di == other.di and self.en_words == other.en_words and self.es_phrase == other.es_phrase

def _normalize_for_matching(text: str) -> str:
    """Helper function to strip all punctuation and normalize for matching."""
    s = re.sub(r'[^\w\s]', '', text)
    return re.sub(r'\s+', ' ', s).strip().lower()

def _strip_punctuation(text: str) -> str:
    """Strips leading/trailing punctuation and whitespace from a string."""
    # This regex removes any character that is not a word character or whitespace from the start/end.
    s = re.sub(r'^[^\w\s]+|[^\w\s]+$', '', text)
    return s.strip()

#
def _normalize_for_alignment(text: str) -> str:
    """A stricter normalization for alignment checks. Removes spaces, hyphens, and apostrophes."""
    return re.sub(r"[^\w]", '', text).lower()

def parse_llm_phrase_map_to_atoms(raw_map_lines: List[str], original_word_tokens: List[Dict]) -> List[SemanticAtom]:
    """
    Parses the raw phrase map and aligns it with the original word tokens
    using a "sliding window" token-aware matching algorithm. This is the
    definitive, robust version.
    """
    semantic_atoms = []
    token_cursor = 0

    parsed_map = []
    for line in raw_map_lines:
        if '->' in line:
            parts = line.split('->', 1)
            if len(parts) == 2:
                en_phrase = _strip_punctuation(parts[0])
                es_phrase = _strip_punctuation(parts[1])
                if en_phrase:
                    parsed_map.append({'en_phrase': en_phrase, 'es_phrase': es_phrase})

    for mapping in parsed_map:
        llm_source_phrase, target_phrase = mapping['en_phrase'], mapping['es_phrase']
        
        # Normalize the LLM phrase once for comparison
        normalized_llm_phrase = _normalize_for_alignment(llm_source_phrase)
        if not normalized_llm_phrase:
            continue

        search_substring_tokens = []
        found_match = False
        
        for i in range(token_cursor, len(original_word_tokens)):
            search_substring_tokens.append(original_word_tokens[i])
            
            # Reconstruct the text from the current window of SpaCy tokens WITHOUT adding spaces
            reconstructed_spacy_text = "".join(t['v'] for t in search_substring_tokens)
            
            # Compare the normalized, space-less, punctuation-less strings
            if _normalize_for_alignment(reconstructed_spacy_text) == normalized_llm_phrase:
                atom = SemanticAtom(
                    di=search_substring_tokens[0]['di'],
                    en_words=[t['v'] for t in search_substring_tokens],
                    es_phrase=target_phrase
                )
                semantic_atoms.append(atom)
                
                token_cursor += len(search_substring_tokens)
                found_match = True
                break

        if not found_match:
            raise ValueError(
                f"LLM phrase map does not align with token stream. "
                f"Could not find a matching token sequence for LLM phrase: '{llm_source_phrase}'"
            )

    if token_cursor != len(original_word_tokens):
        remaining_tokens = " ".join(t['v'] for t in original_word_tokens[token_cursor:])
        raise ValueError(
            f"The phrase map did not exhaustively cover all source tokens. "
            f"Processed {token_cursor}/{len(original_word_tokens)} words. "
            f"Remaining tokens: '{remaining_tokens}'"
        )

    return semantic_atoms

def sanitize_atoms(s_id: str, atoms: List[SemanticAtom], original_base_tier: Dict[str, Any]) -> List[SemanticAtom]:
    """
    Validates a list of SemanticAtoms against segment and punctuation boundaries,
    decomposing invalid atoms as required.
    """
    # Create lookup maps for efficient validation
    flat_original_tokens = [token for seg in original_base_tier.get("segments", []) for token in seg.get("tokenized_text", [])]
    di_to_seg_id: Dict[int, str] = {}
    di_to_token: Dict[int, Dict] = {}
    for seg in original_base_tier.get("segments", []):
        for token in seg.get("tokenized_text", []):
            if token.get('t') == 'w':
                di_to_seg_id[token['di']] = seg['seg_id']
                di_to_token[token['di']] = token

    sanitized_atoms: List[SemanticAtom] = []
    
    for atom in atoms:
        # Single-word atoms are always valid by definition.
        if len(atom.en_words) <= 1:
            sanitized_atoms.append(atom)
            continue

        is_valid = True
        
        # Find all original 'di' values covered by this atom
        atom_dis = []
        try:
            start_idx = next(i for i, t in enumerate(flat_original_tokens) if t.get('di') == atom.di)
            words_collected = 0
            for i in range(start_idx, len(flat_original_tokens)):
                if words_collected == len(atom.en_words): break
                token = flat_original_tokens[i]
                if token.get('t') == 'w':
                    atom_dis.append(token['di'])
                    words_collected += 1
        except (StopIteration, IndexError):
            is_valid = False
            logger.warning(f"S_ID {s_id}: Could not find full token sequence for atom '{' '.join(atom.en_words)}'. Invalidating.")

        # Rule 1: Check if the atom spans multiple segments
        if is_valid:
            segment_ids_for_atom = {di_to_seg_id.get(di) for di in atom_dis}
            if len(segment_ids_for_atom) > 1:
                logger.warning(f"S_ID {s_id}: Invalidating mapping '{' '.join(atom.en_words)}' because it spans segments: {segment_ids_for_atom}")
                is_valid = False

        # Rule 2: Check if the atom spans internal punctuation
        if is_valid:
            start_token_idx = next((i for i, t in enumerate(flat_original_tokens) if t.get('di') == atom_dis[0]), -1)
            end_token_idx = next((i for i, t in enumerate(flat_original_tokens) if t.get('di') == atom_dis[-1]), -1)

            if start_token_idx != -1 and end_token_idx != -1:
                # Get the full slice of tokens, including background tokens
                token_slice_with_b = flat_original_tokens[start_token_idx : end_token_idx + 1]
                # Check for any background token between the first and last word that is not just whitespace
                for token in token_slice_with_b:
                    if token.get('t') == 'b' and token.get('v', '').strip():
                        logger.warning(f"S_ID {s_id}: Invalidating mapping '{' '.join(atom.en_words)}' due to internal punctuation ('{token.get('v')}').")
                        is_valid = False
                        break
        
        # If the atom is valid, add it. Otherwise, decompose it.
        if is_valid:
            sanitized_atoms.append(atom)
        else:
            for di in atom_dis:
                original_word_token = di_to_token[di]
                sanitized_atoms.append(
                    SemanticAtom(di=di, en_words=[original_word_token['v']], es_phrase="NO_SUB")
                )
                
    return sanitized_atoms