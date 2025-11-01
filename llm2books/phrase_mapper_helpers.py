# llm2books/phrase_mapper_helpers.py
import re
from typing import List, Dict, Any, Tuple

from .stages.base import logger
from .helper import fuse_tokens, normalize_spanish_lemma, pre_fuse_word_tokens
from .validator import ValidationError
from rapidfuzz.distance.Levenshtein import opcodes
from .standardize import smart_match_and_edit

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

#
def refactor_token_stream(original_tokens: List[Dict[str, Any]], group_strings: List[str]) -> List[Dict[str, Any]]:
    """
    Performs a three-pass process to align a token stream with LLM-defined word groups.
    Pass 1 (Pre-Fusion): Fixes structural BWBWB violations by fusing contractions.
    Pass 2 (Normalization): Uses smart_match_and_edit to correct token boundaries.
    Pass 3 (Grouping): Fuses the now-normalized tokens into multi-word groups.
    """
    structurally_sound_stream = pre_fuse_word_tokens(original_tokens)
    normalized_stream = list(structurally_sound_stream)
    
    llm_ungrouped_words = [word for group in group_strings for word in group.split()]
    word_tokens_from_stream = [t for t in normalized_stream if t.get('t') == 'w']
    word_values_from_stream = [t.get('v') for t in word_tokens_from_stream]

    if len(llm_ungrouped_words) != len(word_values_from_stream):
        # --- START: NEW DETAILED ERROR REPORTING ---
        error_msg = (
            f"Word count mismatch between LLM groups ({len(llm_ungrouped_words)} words) "
            f"and original tokens ({len(word_values_from_stream)} words).\n\n"
            "--- WORD COMPARISON ---\n"
            "  # | LLM Group Word   | Token Stream Word\n"
            "--------------------------------------------------\n"
        )
        
        # Use itertools.zip_longest to handle lists of different lengths
        from itertools import zip_longest
        
        for i, (llm_word, stream_word) in enumerate(zip_longest(llm_ungrouped_words, word_values_from_stream, fillvalue="<MISSING>")):
            marker = "  " if llm_word == stream_word else ">>"
            error_msg += f"{marker} {i+1:<2}| {llm_word:<16} | {stream_word}\n"
        
        raise ValidationError(error_msg)
        # --- END: NEW DETAILED ERROR REPORTING ---

    word_token_indices = [i for i, t in enumerate(normalized_stream) if t.get('t') == 'w']

    # ... (The rest of the function is unchanged) ...
    for i, llm_word in enumerate(llm_ungrouped_words):
        token_idx_to_check = word_token_indices[i]
        if normalized_stream[token_idx_to_check].get('v') == llm_word:
            continue
        adjusted_stream = smart_match_and_edit(normalized_stream, token_idx_to_check, llm_word)
        if adjusted_stream is None:
            raise ValidationError(
                f"Smart match failed. Could not form word '{llm_word}' at or around "
                f"token index {token_idx_to_check} ('{normalized_stream[token_idx_to_check].get('v')}')."
            )
        normalized_stream = adjusted_stream
        word_token_indices = [j for j, t in enumerate(normalized_stream) if t.get('t') == 'w']

    final_stream = []
    token_cursor = 0
    original_word_tokens_normalized = [t for t in normalized_stream if t.get('t') == 'w']
    word_tokens_consumed = 0
    for group_str in group_strings:
        group_words = group_str.split()
        if not group_words:
            continue
        num_words_in_group = len(group_words)
        start_cursor = -1
        for i in range(token_cursor, len(normalized_stream)):
            if normalized_stream[i].get('t') == 'w':
                start_cursor = i
                break
        if start_cursor == -1:
            raise ValidationError(f"Logic error: Ran out of tokens while looking for group '{group_str}'.")
        consumed_tokens_for_group = []
        words_found = 0
        end_cursor = start_cursor
        for i in range(start_cursor, len(normalized_stream)):
            token = normalized_stream[i]
            consumed_tokens_for_group.append(token)
            if token.get('t') == 'w':
                words_found += 1
            if words_found == num_words_in_group:
                end_cursor = i + 1
                break
        else:
            raise ValidationError(f"Could not form group '{group_str}': not enough word tokens remaining in stream.")
        if start_cursor > token_cursor:
            final_stream.extend(normalized_stream[token_cursor:start_cursor])
        group_word_tokens = [t for t in consumed_tokens_for_group if t.get('t') == 'w']
        word_tokens_consumed += len(group_word_tokens)
        fused_token_value = "".join(t.get('v', '') for t in consumed_tokens_for_group)
        fused_token = {
            't': 'w',
            'v': fused_token_value,
            'di': group_word_tokens[0]['di'],
            'l': sorted(list(set(lemma for t in group_word_tokens for lemma in t.get('l', []))))
        }
        final_stream.append(fused_token)
        token_cursor = end_cursor
    if token_cursor < len(normalized_stream):
        final_stream.extend(normalized_stream[token_cursor:])
    if word_tokens_consumed != len(original_word_tokens_normalized):
        raise ValidationError(f"Incomplete consumption of tokens. Expected to consume {len(original_word_tokens_normalized)} words, but consumed {word_tokens_consumed}.")
    if not final_stream: return [{'t': 'b', 'v': ''}]
    if final_stream[0]['t'] == 'w': final_stream.insert(0, {'t': 'b', 'v': ''})
    if final_stream[-1]['t'] == 'w': final_stream.append({'t': 'b', 'v': ''})
    return final_stream

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