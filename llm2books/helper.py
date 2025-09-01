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

    if isinstance(nlp_doc, SpacyDoc):
        token_iterator, get_start_char, get_end_char, get_type = nlp_doc, lambda tok: tok.idx, lambda tok: tok.idx + len(tok.text), lambda tok: 'w' if not tok.is_punct and not tok.is_space else 'b'
    elif isinstance(nlp_doc, stanza.models.common.doc.Sentence):
        token_iterator, get_start_char, get_end_char, get_type = [word for token in nlp_doc.tokens for word in token.words], lambda word: word.start_char, lambda word: word.end_char, lambda word: 'w' if word.upos != 'PUNCT' else 'b'
    else: raise TypeError(f"Unsupported NLP document type for golden stream: {type(nlp_doc)}")
    
    raw_stream = []
    last_idx = 0
    for token in token_iterator:
        start_char, end_char = get_start_char(token), get_end_char(token)
        if start_char is None or end_char is None: continue
        if start_char > last_idx:
            raw_stream.append({'t': 'b', 'v': source_text[last_idx:start_char]})
        raw_stream.append({'t': get_type(token), 'v': source_text[start_char:end_char]})
        last_idx = end_char
    if last_idx < len(source_text):
        raw_stream.append({'t': 'b', 'v': source_text[last_idx:]})

    if not raw_stream: return []

    # First merge pass for consecutive types
    merged_stream = [raw_stream[0]]
    for token in raw_stream[1:]:
        if token['t'] == merged_stream[-1]['t']:
            merged_stream[-1]['v'] += token['v']
        else:
            merged_stream.append(token)
    
    # --- THIS IS THE FINAL FIX ---
    # Now, fuse the stream to handle contractions and hyphens correctly.
    fused_stream = fuse_tokens(merged_stream)
    
    # Ensure the BWBWB invariant on the final, fused stream.
    if not fused_stream: return [{'t': 'b', 'v': ''}]
    if fused_stream[0].get('t') == 'w':
        fused_stream.insert(0, {'t': 'b', 'v': ''})
    if fused_stream[-1].get('t') == 'w':
        fused_stream.append({'t': 'b', 'v': ''})
        
    return fused_stream
    # --- END OF FINAL FIX ---


def fuse_tokens(raw_tokens: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    if not raw_tokens:
        return []

    tokens = list(raw_tokens)
    i = 0
    while i < len(tokens) - 1:
        if i + 2 < len(tokens):
            current, background, next_word = tokens[i], tokens[i+1], tokens[i+2]
            if (current.get("t") == "w" and background.get("t") == "b" and
                next_word.get("t") == "w" and background.get("v") in ["", "-"]):
                current["v"] += background["v"] + next_word["v"]
                del tokens[i+2]; del tokens[i+1]
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
    s = lemma_str.lower().strip().split(' ')[0]
    s = s.replace('á', 'a').replace('é', 'e').replace('í', 'i').replace('ó', 'o').replace('ú', 'u')
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    if not s: return ""
    s = unicodedata.normalize('NFC', s)
    if re.search(r'[^a-z-]', s): return ""
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