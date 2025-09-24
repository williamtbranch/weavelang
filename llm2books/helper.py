import re
import unicodedata
import argparse
import logging
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import List, Dict, Any

from dotenv import load_dotenv
from spacy.tokens import Doc as SpacyDoc, Span
import stanza

logger = logging.getLogger("pipeline")

def preprocess_for_spacy(text: str) -> str:
    """
    Inserts spaces around em-dashes and parentheses to ensure SpaCy
    tokenizes them correctly as separate tokens.
    """
    text = re.sub(r'(\w)([—()])', r'\1 \2', text)
    text = re.sub(r'([—()])(\w)', r'\1 \2', text)
    return text

def create_golden_token_stream(nlp_doc: Any) -> List[Dict[str, Any]]:
    source_text = nlp_doc.text
    if not source_text:
        return []

    raw_stream = []
    source_pointer = 0

    # Get the raw tokens/words from the NLP library
    components = nlp_doc.words if isinstance(nlp_doc, stanza.models.common.doc.Sentence) else nlp_doc
    
    for component in components:
        start_char = component.start_char if isinstance(nlp_doc, stanza.models.common.doc.Sentence) else component.idx
        end_char = component.end_char if isinstance(nlp_doc, stanza.models.common.doc.Sentence) else component.idx + len(component.text)

        # 1. Consume any background text between the last pointer and the start of this component.
        if start_char > source_pointer:
            raw_stream.append({'t': 'b', 'v': source_text[source_pointer:start_char]})
        
        component_text = source_text[start_char:end_char]
        
        # 2. Character-level state machine for the component's text itself.
        # This correctly handles cases like "said,".
        is_word_char = lambda char: char.isalnum() or char in "'-’"
        
        current_type = 'w' if component_text and is_word_char(component_text[0]) else 'b'
        buffer = ""
        for char in component_text:
            char_is_word = is_word_char(char)
            if (char_is_word and current_type == 'w') or (not char_is_word and current_type == 'b'):
                buffer += char
            else:
                # Type changed, flush the buffer and start a new one.
                raw_stream.append({'t': current_type, 'v': buffer})
                buffer = char
                current_type = 'w' if char_is_word else 'b'
        
        # Flush the final buffer for this component
        if buffer:
            raw_stream.append({'t': current_type, 'v': buffer})

        source_pointer = end_char

    # 3. Consume any final trailing text.
    if source_pointer < len(source_text):
        raw_stream.append({'t': 'b', 'v': source_text[source_pointer:]})

    if not raw_stream: return []

    # 4. Post-processing: Fuse first, then merge.
    fused_stream = fuse_tokens(raw_stream)

    merged_stream = [fused_stream[0]] if fused_stream else []
    for token in fused_stream[1:]:
        if token['t'] == merged_stream[-1]['t']:
            merged_stream[-1]['v'] += token['v']
        else:
            merged_stream.append(token)

    # 5. Finalize BWBWB structure.
    final_stream = merged_stream
    if not final_stream: return [{'t': 'b', 'v': ''}]
    if final_stream[0].get('t') == 'w':
        final_stream.insert(0, {'t': 'b', 'v': ''})
    if final_stream[-1].get('t') == 'w':
        final_stream.append({'t': 'b', 'v': ''})
        
    return final_stream
# def create_golden_token_stream(nlp_doc: Any) -> List[Dict[str, Any]]:
#     source_text = nlp_doc.text
#     if not source_text:
#         return []

#     raw_stream = []
#     last_idx = 0

#     # Determine the iterator and properties based on the doc type
#     if isinstance(nlp_doc, SpacyDoc):
#         # For SpaCy, we iterate through each token directly.
#         token_iterator = nlp_doc
#         get_start_char = lambda tok: tok.idx
#         get_end_char = lambda tok: tok.idx + len(tok.text)
#         # Type is based on SpaCy's boolean flags.
#         get_type = lambda tok: 'w' if not tok.is_punct and not tok.is_space else 'b'
#     elif isinstance(nlp_doc, stanza.models.common.doc.Sentence):
#         # For Stanza, we must iterate through the 'words' within each 'token'.
#         # A 'token' can be a multi-word token (e.g., "del" -> "de", "el").
#         token_iterator = nlp_doc.words 
#         get_start_char = lambda word: word.start_char
#         get_end_char = lambda word: word.end_char
#         # Type is based on the Universal Part of Speech tag.
#         get_type = lambda word: 'w' if word.upos != 'PUNCT' else 'b'
#     else:
#         raise TypeError(f"Unsupported NLP document type for golden stream: {type(nlp_doc)}")
    
#     # --- Universal stream building logic ---
#     for token in token_iterator:
#         start_char, end_char = get_start_char(token), get_end_char(token)
#         if start_char is None or end_char is None: continue
        
#         # Capture any text between the last token and this one as background.
#         if start_char > last_idx:
#             raw_stream.append({'t': 'b', 'v': source_text[last_idx:start_char]})
            
#         # Add the current token with its determined type.
#         raw_stream.append({'t': get_type(token), 'v': source_text[start_char:end_char]})
#         last_idx = end_char

#     # Capture any trailing text.
#     if last_idx < len(source_text):
#         raw_stream.append({'t': 'b', 'v': source_text[last_idx:]})

#     if not raw_stream: return []

#     # --- Post-processing: Merge, Fuse, and ensure BWBWB ---
#     # This part of the logic is sound and remains the same.
#     merged_stream = [raw_stream[0]]
#     for token in raw_stream[1:]:
#         if token['t'] == merged_stream[-1]['t']:
#             merged_stream[-1]['v'] += token['v']
#         else:
#             merged_stream.append(token)
    
#     fused_stream = fuse_tokens(merged_stream)
    
#     if not fused_stream: return [{'t': 'b', 'v': ''}]
#     if fused_stream[0].get('t') == 'w':
#         fused_stream.insert(0, {'t': 'b', 'v': ''})
#     if fused_stream[-1].get('t') == 'w':
#         fused_stream.append({'t': 'b', 'v': ''})
        
#     return fused_stream

def fuse_tokens(raw_tokens: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """
    Revised fuser to handle W-B-W patterns AND Stanza's W-W output for contractions.
    """
    if not raw_tokens:
        return []

    tokens = list(raw_tokens)
    i = 0
    while i < len(tokens) - 1:
        current = tokens[i]
        next_tok = tokens[i+1]

        # Case 1: SpaCy's don't -> don, n't (W-B-W with empty B)
        if i + 2 < len(tokens):
            background = tokens[i+1]
            next_word = tokens[i+2]
            if (current.get("t") == "w" and background.get("t") == "b" and
                next_word.get("t") == "w" and background.get("v") in ["", "-"]):
                current["v"] += background["v"] + next_word["v"]
                del tokens[i+2]; del tokens[i+1]
                continue # Re-evaluate the same index i with the newly fused token
        
        # Case 2: Stanza's It's -> It, 's (W-W)
        if current.get("t") == "w" and next_tok.get("t") == "w" and \
           next_tok.get("v", "").startswith("'") or next_tok.get("v", "").startswith("’"):
           current["v"] += next_tok["v"]
           del tokens[i+1]
           continue

        i += 1
    return tokens

def get_iso_timestamp() -> str: return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
def initialize_llm_client(provider: str) -> any:
    load_dotenv(dotenv_path=Path.cwd() / ".env")
    if provider == "claude":
        try: from anthropic import Anthropic
        except ImportError: logger.critical("Anthropic SDK not found."); return None
        api_key = os.getenv("ANTHROPIC_API_KEY")
        if not api_key: logger.critical("ANTHROPIC_API_KEY not found."); return None
        return Anthropic(api_key=api_key)
    logger.critical(f"LLM provider '{provider}' is not supported."); return None

def normalize_spanish_lemma(lemma_str: str) -> str:
    """
    Applies a series of cleaning and normalization steps to a raw Spanish lemma string,
    handling all standard accented vowels, the ñ, and the ü with diaeresis.
    """
    s = lemma_str.lower().strip().split(' ')[0]
    
    # --- THIS IS THE DEFINITIVE FIX ---
    # Handle accented vowels, ñ, and ü.
    s = (s.replace('á', 'a')
          .replace('é', 'e')
          .replace('í', 'i')
          .replace('ó', 'o')
          .replace('ú', 'u')
          .replace('ñ', 'n')
          .replace('ü', 'u'))
    # --- END OF DEFINITIVE FIX ---

    # Strip any remaining non-word characters from the start and end
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    if not s: 
        return ""
        
    # Standard Unicode normalization
    s = unicodedata.normalize('NFC', s)
    
    # Final validation to ensure the string only contains a-z and hyphens.
    # This will now pass for words that originally had ñ or ü.
    if re.search(r'[^a-z-]', s): 
        # This can still catch unexpected characters, so it's good to keep.
        return ""
        
    return s

def create_v2_token_list(span: Span) -> List[Dict[str, Any]]:
    if not span: return [{"t": "b", "v": ""}]
    raw_tokens = []
    for token in span:
        if token.is_punct or token.is_space: raw_tokens.append({"t": "b", "v": token.text_with_ws})
        else:
            raw_tokens.append({"t": "w", "v": token.text})
            if token.whitespace_: raw_tokens.append({"t": "b", "v": token.whitespace_})
    if not raw_tokens: return [{"t": "b", "v": span.text_with_ws}]
    merged = []
    for token in raw_tokens:
        if token["t"] == "b" and merged and merged[-1]["t"] == "b": merged[-1]["v"] += token["v"]
        else: merged.append(token)
    if merged and merged[0]["t"] == "w": merged.insert(0, {"t": "b", "v": ""})
    if merged and merged[-1]["t"] == "w": merged.append({"t": "b", "v": ""})
    return merged