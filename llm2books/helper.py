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
    """
    Creates a canonical BWBWB token stream using a hybrid approach.
    1. Trusts the NLP library's high-level component boundaries.
    2. Uses a robust, character-level state machine to process the text *within* each component.
    """
    source_text = nlp_doc.text
    if not source_text:
        return []

    raw_stream = []
    source_pointer = 0

    components = nlp_doc.words if isinstance(nlp_doc, stanza.models.common.doc.Sentence) else nlp_doc
    
    for component in components:
        start_char = component.start_char if isinstance(nlp_doc, stanza.models.common.doc.Sentence) else component.idx
        end_char = component.end_char if isinstance(nlp_doc, stanza.models.common.doc.Sentence) else component.idx + len(component.text)

        if start_char > source_pointer:
            raw_stream.append({'t': 'b', 'v': source_text[source_pointer:start_char]})
        
        component_text = source_text[start_char:end_char]
        
        # --- THIS IS THE FIX ---
        # The state machine must be stricter to prevent trailing whitespace
        # from being included in a word token's value.
        is_word_char = lambda char: char.isalnum()
        
        buffer = ""
        is_in_word = False
        for char in component_text:
            if is_word_char(char):
                if not is_in_word and buffer: # Transition from background to word
                    raw_stream.append({'t': 'b', 'v': buffer})
                    buffer = ""
                is_in_word = True
                buffer += char
            else: # It's a background character
                if is_in_word and buffer: # Transition from word to background
                    raw_stream.append({'t': 'w', 'v': buffer})
                    buffer = ""
                is_in_word = False
                buffer += char
        
        # Flush the final buffer for this component
        if buffer:
            final_type = 'w' if is_in_word else 'b'
            raw_stream.append({'t': final_type, 'v': buffer})
        # --- END OF FIX ---

        source_pointer = end_char

    if source_pointer < len(source_text):
        raw_stream.append({'t': 'b', 'v': source_text[source_pointer:]})

    if not raw_stream: return []

    # Post-processing: Fuse first, then merge.
    fused_stream = fuse_tokens(raw_stream)

    merged_stream = [fused_stream[0]] if fused_stream else []
    for token in fused_stream[1:]:
        if token['t'] == merged_stream[-1]['t']:
            merged_stream[-1]['v'] += token['v']
        else:
            merged_stream.append(token)

    # Finalize BWBWB structure.
    final_stream = merged_stream
    if not final_stream: return [{'t': 'b', 'v': ''}]
    if final_stream[0].get('t') == 'w':
        final_stream.insert(0, {'t': 'b', 'v': ''})
    if final_stream[-1].get('t') == 'w':
        final_stream.append({'t': 'b', 'v': ''})
        
    return final_stream

def fuse_tokens(raw_tokens: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """
    Revised fuser to handle W-B-W patterns from SpaCy AND Stanza's W-W output for contractions.
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
    
    s = (s.replace('á', 'a')
          .replace('é', 'e')
          .replace('í', 'i')
          .replace('ó', 'o')
          .replace('ú', 'u')
          .replace('ñ', 'n')
          .replace('ü', 'u'))

    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    if not s: 
        return ""
        
    s = unicodedata.normalize('NFC', s)
    
    if re.search(r'[^a-z-]', s): 
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