# llm2books/phrase_mapper_helpers.py
import re
from typing import List, Dict, Any, Tuple

from .stages.base import logger
from .helper import fuse_tokens, normalize_spanish_lemma
from .validator import ValidationError
from rapidfuzz.distance.Levenshtein import opcodes

# --- START: NEW FUNCTIONS TO FIX IMPORT ERROR ---
def parse_proper_nouns(llm_output_phrase: str, spacy_model: Any) -> Tuple[str, List[str]]:
    """
    Parses a string like "the cat {Gregor Samsa}" into a clean phrase and a list
    of normalized lemmas for the proper nouns.
    """
    proper_noun_lemmas = set()
    
    # Find all proper noun blocks, e.g., "{Gregor Samsa}"
    pn_matches = re.findall(r'\{([^{}]+)\}', llm_output_phrase)
    
    for match in pn_matches:
        doc = spacy_model(match)
        for token in doc:
            if not token.is_punct and not token.is_space:
                # Assuming Spanish, but this should be generalized if needed
                lemma = normalize_spanish_lemma(token.lemma_)
                if lemma:
                    proper_noun_lemmas.add(lemma)

    # Remove the curly braces to get the clean phrase
    clean_phrase = re.sub(r'\{|\}', '', llm_output_phrase).strip()
    
    return clean_phrase, sorted(list(proper_noun_lemmas))

def refactor_token_stream(original_tokens: List[Dict[str, Any]], group_strings: List[str]) -> List[Dict[str, Any]]:
    """
    Validates that the WORD content of `group_strings` can be losslessly 
    constructed from the `original_tokens` and returns a new token stream with
    the groups fused, preserving all interstitial punctuation.
    """
    def clean_for_comparison(s: str) -> str:
        """Normalizes quotes and strips common trailing punctuation for matching purposes."""
        return s.replace('’', "'").replace('‘', "'").replace('”', '"').replace('“', '"').rstrip('.,;:?!')

    new_token_stream = []
    token_cursor = 0
    word_tokens_consumed = 0
    original_word_tokens = [t for t in original_tokens if t.get('t') == 'w']

    for group_str in group_strings:
        cleaned_group_for_words = clean_for_comparison(group_str)
        group_words = re.findall(r"[\w'-]+", cleaned_group_for_words)
        
        if not group_words:
            continue

        first_word_of_group = group_words[0]
        
        start_cursor = -1
        for i in range(token_cursor, len(original_tokens)):
            token = original_tokens[i]
            if token.get('t') == 'w':
                if clean_for_comparison(token.get('v', '')) == first_word_of_group:
                    start_cursor = i
                    break
        
        if start_cursor == -1:
            raise ValidationError(f"Could not find start of group '{group_str}' in token stream.")

        if start_cursor > token_cursor:
            new_token_stream.extend(original_tokens[token_cursor:start_cursor])

        # --- START: NEW WORD-BASED VALIDATION LOGIC ---
        
        consumed_tokens_for_group = []
        consumed_word_values = []
        end_cursor = start_cursor

        for i in range(start_cursor, len(original_tokens)):
            token = original_tokens[i]
            consumed_tokens_for_group.append(token)
            
            if token.get('t') == 'w':
                cleaned_value = clean_for_comparison(token.get('v', ''))
                consumed_word_values.append(cleaned_value)

            # Check if the list of consumed words matches the target group words
            if consumed_word_values == group_words:
                end_cursor = i + 1
                break
        else:
            # If the loop finishes without a match, validation fails.
            raise ValidationError(f"Could not fully form group '{group_str}' from token stream. Word mismatch detected.")

        # --- END: NEW WORD-BASED VALIDATION LOGIC ---

        group_word_tokens = [t for t in consumed_tokens_for_group if t.get('t') == 'w']
        if not group_word_tokens:
            raise ValidationError(
                f"Invalid group '{group_str}': A mapping group must contain at least one word, "
                "but this group was formed exclusively from background/punctuation tokens."
            )
        
        word_tokens_consumed += len(group_word_tokens)

        fused_token_value = "".join(t.get('v', '') for t in consumed_tokens_for_group)
        fused_token = {
            't': 'w',
            'v': fused_token_value,
            'di': group_word_tokens[0]['di'],
            'l': sorted(list(set(lemma for t in group_word_tokens for lemma in t.get('l', []))))
        }
        new_token_stream.append(fused_token)
        token_cursor = end_cursor

    if token_cursor < len(original_tokens):
        new_token_stream.extend(original_tokens[token_cursor:])

    if word_tokens_consumed != len(original_word_tokens):
        raise ValidationError(f"Incomplete consumption of tokens. Expected to consume {len(original_word_tokens)} words, but consumed {word_tokens_consumed}.")

    if not new_token_stream: return [{'t': 'b', 'v': ''}]
    if new_token_stream[0]['t'] == 'w': new_token_stream.insert(0, {'t': 'b', 'v': ''})
    if new_token_stream[-1]['t'] == 'w': new_token_stream.append({'t': 'b', 'v': ''})
        
    return new_token_stream
# --- END: NEW FUNCTIONS ---

class SemanticAtom:
    def __init__(self, di: int, en_words: List[str], es_phrase: str): self.di, self.en_words, self.es_phrase = di, en_words, es_phrase
    def __repr__(self) -> str: return f"Atom(di={self.di}, en='{' '.join(self.en_words)}', es='{self.es_phrase}')"
    def __eq__(self, other):
        if not isinstance(other, SemanticAtom): return NotImplemented
        return self.di == other.di and self.en_words == other.en_words and self.es_phrase == other.es_phrase

def align_and_parse_to_atoms(raw_map_lines: List[str], raw_spacy_tokens: List[Dict]) -> List[SemanticAtom]:
    fused_spacy_tokens = fuse_tokens(raw_spacy_tokens)
    spacy_word_tokens = [t for t in fused_spacy_tokens if t.get('t') == 'w']
    spacy_words = [t['v'] for t in spacy_word_tokens]
    
    llm_mappings = []
    for line in raw_map_lines:
        if '->' in line:
            parts = line.split('->', 1)
            if len(parts) == 2 and parts[0].strip():
                llm_mappings.append({'en': parts[0].strip(), 'es': re.sub(r'^[^\w\s]+|[^\w\s]+$', '', parts[1].strip()).strip()})

    llm_flat_words, llm_phrase_indices = [], []
    for i, mapping in enumerate(llm_mappings):
        words_in_phrase = mapping['en'].split()
        for word in words_in_phrase:
            llm_flat_words.append(word)
            llm_phrase_indices.append(i)

    spacy_norm = [re.sub(r'[^\w]', '', w).lower() for w in spacy_words]
    llm_norm = [re.sub(r'[^\w]', '', w).lower() for w in llm_flat_words]
    alignment_opcodes = opcodes(spacy_norm, llm_norm)

    final_atoms = []
    
    spacy_to_llm_phrase_map = {}
    for tag, src_i1, src_i2, dst_i1, dst_i2 in alignment_opcodes:
        if tag == 'equal':
            for i in range(src_i2 - src_i1):
                spacy_idx = src_i1 + i
                llm_flat_idx = dst_i1 + i
                spacy_to_llm_phrase_map[spacy_idx] = llm_phrase_indices[llm_flat_idx]

    spacy_cursor = 0
    while spacy_cursor < len(spacy_word_tokens):
        token = spacy_word_tokens[spacy_cursor]
        target_llm_phrase_idx = spacy_to_llm_phrase_map.get(spacy_cursor)
        
        if target_llm_phrase_idx is None:
            final_atoms.append(SemanticAtom(di=token['di'], en_words=[token['v']], es_phrase="NO_SUB"))
            spacy_cursor += 1
            continue

        group_end_cursor = spacy_cursor + 1
        while group_end_cursor < len(spacy_word_tokens):
            if spacy_to_llm_phrase_map.get(group_end_cursor) != target_llm_phrase_idx:
                break
            group_end_cursor += 1
            
        spacy_token_group = spacy_word_tokens[spacy_cursor:group_end_cursor]
        es_phrase = llm_mappings[target_llm_phrase_idx]['es']
        
        atom = SemanticAtom(
            di=spacy_token_group[0]['di'],
            en_words=[t['v'] for t in spacy_token_group],
            es_phrase=es_phrase
        )
        final_atoms.append(atom)
        spacy_cursor = group_end_cursor
    
    return final_atoms

def sanitize_atoms(s_id: str, atoms: List[SemanticAtom], original_tier: Dict[str, Any]) -> List[SemanticAtom]:
    flat_original_tokens = [token for seg in original_tier.get("segments", []) for token in seg.get("tokenized_text", [])]
    di_to_token: Dict[int, Dict] = {}
    for seg in original_tier.get("segments", []):
        for token in seg.get("tokenized_text", []):
            if token.get('t') == 'w':
                di_to_token[token['di']] = token

    sanitized_atoms: List[SemanticAtom] = []
    for atom in atoms:
        if len(atom.en_words) == 1 and ' ' not in atom.en_words[0]:
            sanitized_atoms.append(atom)
            continue
        
        is_valid = True
        atom_dis = []
        try:
            start_idx = next(i for i, t in enumerate(flat_original_tokens) if t.get('di') == atom.di)
            words_collected = 0
            flat_en_words = " ".join(atom.en_words).split()
            for i in range(start_idx, len(flat_original_tokens)):
                if words_collected == len(flat_en_words): break
                token = flat_original_tokens[i]
                if token.get('t') == 'w':
                    if token.get('v') == flat_en_words[words_collected]:
                        atom_dis.append(token['di'])
                        words_collected += 1
                    else:
                        is_valid = False; break
        except (StopIteration, IndexError): is_valid = False

        if not is_valid:
            logger.warning(f"S_ID {s_id}: Could not find full token sequence for atom '{' '.join(atom.en_words)}'. Invalidating.")

        if is_valid:
            start_token_idx = next((i for i, t in enumerate(flat_original_tokens) if t.get('di') == atom_dis[0]), -1)
            end_token_idx = next((i for i, t in enumerate(flat_original_tokens) if t.get('di') == atom_dis[-1]), -1)
            if start_token_idx != -1 and end_token_idx != -1:
                token_slice_with_b = flat_original_tokens[start_token_idx : end_token_idx + 1]
                for token in token_slice_with_b:
                    if token.get('t') == 'b' and token.get('v', '').strip():
                        is_valid = False
                        logger.warning(f"S_ID {s_id}: Invalidating mapping '{' '.join(atom.en_words)}' due to internal punctuation ('{token.get('v')}').")
                        break
        
        if is_valid:
            sanitized_atoms.append(atom)
        else:
            for di in atom_dis:
                original_word_token = di_to_token[di]
                sanitized_atoms.append(SemanticAtom(di=di, en_words=[original_word_token['v']], es_phrase="NO_SUB"))
    
    return sanitized_atoms