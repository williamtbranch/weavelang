import re
from typing import List, Dict, Any

from .stages.base import logger
from .helper import fuse_tokens
from rapidfuzz.distance.Levenshtein import opcodes

class SemanticAtom:
    def __init__(self, di: int, en_words: List[str], es_phrase: str): self.di, self.en_words, self.es_phrase = di, en_words, es_phrase
    def __repr__(self) -> str: return f"Atom(di={self.di}, en='{' '.join(self.en_words)}', es='{self.es_phrase}')"
    def __eq__(self, other):
        if not isinstance(other, SemanticAtom): return NotImplemented
        return self.di == other.di and self.en_words == other.en_words and self.es_phrase == other.es_phrase

def align_and_parse_to_atoms(raw_map_lines: List[str], raw_spacy_tokens: List[Dict]) -> List[SemanticAtom]:
    # 1. PREPARATION
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

    # 2. ALIGNMENT
    spacy_norm = [re.sub(r'[^\w]', '', w).lower() for w in spacy_words]
    llm_norm = [re.sub(r'[^\w]', '', w).lower() for w in llm_flat_words]
    alignment_opcodes = opcodes(spacy_norm, llm_norm)

    # 3. RE-GROUPING
    final_atoms = []
    
    # --- FINAL, CORRECT RE-GROUPING LOGIC ---
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
    # --- END OF FINAL FIX ---
    
    return final_atoms

# (sanitize_atoms remains unchanged)
def sanitize_atoms(s_id: str, atoms: List[SemanticAtom], original_base_tier: Dict[str, Any]) -> List[SemanticAtom]:
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
        if len(atom.en_words) == 1 and not ' ' in atom.en_words[0]:
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
        if not is_valid: logger.warning(f"S_ID {s_id}: Could not find full token sequence for atom '{' '.join(atom.en_words)}'. Invalidating.")
        if is_valid:
            segment_ids_for_atom = {di_to_seg_id.get(di) for di in atom_dis}
            if len(segment_ids_for_atom) > 1:
                is_valid = False; logger.warning(f"S_ID {s_id}: Invalidating mapping '{' '.join(atom.en_words)}' because it spans segments: {segment_ids_for_atom}")
        if is_valid:
            start_token_idx = next((i for i, t in enumerate(flat_original_tokens) if t.get('di') == atom_dis[0]), -1)
            end_token_idx = next((i for i, t in enumerate(flat_original_tokens) if t.get('di') == atom_dis[-1]), -1)
            if start_token_idx != -1 and end_token_idx != -1:
                token_slice_with_b = flat_original_tokens[start_token_idx : end_token_idx + 1]
                for token in token_slice_with_b:
                    if token.get('t') == 'b' and token.get('v', '').strip():
                        is_valid = False; logger.warning(f"S_ID {s_id}: Invalidating mapping '{' '.join(atom.en_words)}' due to internal punctuation ('{token.get('v')}').")
                        break
        if is_valid:
            sanitized_atoms.append(atom)
        else:
            for di in atom_dis:
                original_word_token = di_to_token[di]
                sanitized_atoms.append(SemanticAtom(di=di, en_words=[original_word_token['v']], es_phrase="NO_SUB"))
    return sanitized_atoms