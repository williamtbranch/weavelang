#helper.py
import re
import unicodedata
import argparse
import logging
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import List, Dict, Any

from dotenv import load_dotenv
from spacy.tokens import Doc as SpacyDoc, Span, Token as SpacyToken
import stanza

try:
    import google.generativeai as genai
except ImportError:
    genai = None

logger = logging.getLogger("pipeline")

def preprocess_for_spacy(text: str) -> str:
    """
    Inserts spaces around em-dashes and parentheses to ensure SpaCy
    tokenizes them correctly as separate tokens.
    """
    text = re.sub(r'(\w)([—()])', r'\1 \2', text)
    text = re.sub(r'([—()])(\w)', r'\1 \2', text)
    return text

# --- THIS FUNCTION IS NOW RESTORED ---
def fuse_tokens(raw_tokens: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    if not raw_tokens:
        return []

    tokens = list(raw_tokens)
    i = 0
    while i < len(tokens) - 1: # Iterate to the second to last element
        current = tokens[i]
        next_token = tokens[i+1]

        # Simplified rule: if a word is followed by a non-space background, fuse it.
        if (current.get("t") == "w" and
            next_token.get("t") == "b" and
            " " not in next_token.get("v", "") and
            i + 2 < len(tokens) and tokens[i+2].get("t") == "w"):
            
            current["v"] += next_token["v"] + tokens[i+2]["v"]
            
            # Combine lemmas if they exist
            if "l" in tokens[i+2]:
                current.setdefault("l", []).extend(tokens[i+2]["l"])

            del tokens[i+2]
            del tokens[i+1]
            
            continue # Re-evaluate from the current index `i`
        
        i += 1
    return tokens
def fuse_nlp_components(raw_components: List[Any]) -> List[List[Any]]:
    """
    Fuses NLP components (tokens) based on whitespace and linguistic roles,
    correctly distinguishing word-level vs. sentence-level punctuation.
    """
    if not raw_components:
        return []

    fused_components = []
    current_group = []
    
    for i, token in enumerate(raw_components):
        if not current_group:
            current_group.append(token)
            continue
        
        prev_token = current_group[-1]

        # Rule 1: Always split if there is a space after the previous token.
        if ' ' in getattr(prev_token, 'whitespace_', ''):
            fused_components.append(current_group)
            current_group = [token]
            continue

        # Rule 2: If no space, check the linguistic role of the CURRENT token.
        token_text = getattr(token, 'text', '')
        is_apostrophe = token_text in ("'", "’")
        
        is_closing_quote = False
        if is_apostrophe:
            is_followed_by_punct = False
            if (i + 1) < len(raw_components):
                next_token = raw_components[i+1]
                if getattr(next_token, 'is_punct', False) and not getattr(token, 'whitespace_', ''):
                    is_followed_by_punct = True
            
            is_last_token = (i + 1) == len(raw_components)

            if is_last_token or is_followed_by_punct:
                is_closing_quote = True
        
        is_possessive_particle = getattr(token, 'pos_', '') == 'PART'
        
        # --- START OF DEFINITIVE FIX ---
        is_hyphen = getattr(token, 'tag_', '') == 'HYPH'
        # This is the new condition: check if the PREVIOUS token was a hyphen.
        prev_token_is_hyphen = getattr(prev_token, 'tag_', '') == 'HYPH'
        # --- END OF DEFINITIVE FIX ---
        
        is_common_contraction = token_text.lower() in ("'s", "n't", "'re", "'ve", "'d", "'ll")
        
        is_contraction_or_possessive = (
            (is_apostrophe and not is_closing_quote) or 
            is_common_contraction or
            is_possessive_particle
        )
        
        is_internal_apostrophe = (
            is_apostrophe and
            len(getattr(prev_token, 'text', '')) == 1 and
            not getattr(prev_token, 'is_punct', True)
        )

        # --- START OF DEFINITIVE FIX ---
        # Add `prev_token_is_hyphen` to the condition for fusing.
        if is_hyphen or prev_token_is_hyphen or is_contraction_or_possessive or is_internal_apostrophe:
            current_group.append(token)
        else:
            fused_components.append(current_group)
            current_group = [token]
        # --- END OF DEFINITIVE FIX ---

    if current_group:
        fused_components.append(current_group)
        
    return fused_components

def create_golden_token_stream(nlp_doc_or_span: Any) -> List[Dict[str, Any]]:
    """
    Creates a golden B/W token stream from a SpaCy Doc or Span.
    This is the final, robust, SpaCy-only implementation.
    """
    if not isinstance(nlp_doc_or_span, (SpacyDoc, Span)):
        # This path is now only for legacy tests that may use Stanza.
        if hasattr(nlp_doc_or_span, 'words'):
             raw_components = nlp_doc_or_span.words
        else:
            raise TypeError(f"This function now only accepts SpaCy Doc/Span objects, not {type(nlp_doc_or_span)}")
    else:
        raw_components = list(nlp_doc_or_span)

    fused_components = fuse_nlp_components(raw_components)
    source_text = nlp_doc_or_span.text
    if not source_text:
        return []

    raw_stream = []
    source_pointer = 0
    
    for component_group in fused_components:
        if not component_group:
            continue
            
        first_token = component_group[0]
        last_token = component_group[-1]
        
        start_char = getattr(first_token, 'idx', getattr(first_token, 'start_char', 0))
        end_char = getattr(last_token, 'end_char', getattr(last_token, 'idx', 0) + len(getattr(last_token, 'text', '')))
        
        if start_char > source_pointer:
            raw_stream.append({'t': 'b', 'v': source_text[source_pointer:start_char]})

        component_group_text = source_text[start_char:end_char]
        
        is_word = any(not t.is_punct for t in component_group)
        
        if is_word:
            raw_stream.append({'t': 'w', 'v': component_group_text})
        else:
            raw_stream.append({'t': 'b', 'v': component_group_text})
            
        source_pointer = end_char

    if source_pointer < len(source_text):
        raw_stream.append({'t': 'b', 'v': source_text[source_pointer:]})
        
    merged_stream = []
    for token in raw_stream:
        if token['t'] == 'b' and merged_stream and merged_stream[-1]['t'] == 'b':
            merged_stream[-1]['v'] += token['v']
        else:
            merged_stream.append(token)
            
    final_stream = merged_stream

    if not final_stream: return [{'t': 'b', 'v': ''}]
    if final_stream[0].get('t') == 'w':
        final_stream.insert(0, {'t': 'b', 'v': ''})
    if final_stream[-1].get('t') == 'w':
        final_stream.append({'t': 'b', 'v': ''})
        
    return final_stream

def get_iso_timestamp() -> str: return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
def initialize_llm_client(provider: str) -> any:
    load_dotenv(dotenv_path=Path.cwd() / ".env")
    if provider == "claude":
        try: from anthropic import Anthropic
        except ImportError: logger.critical("Anthropic SDK not found."); return None
        api_key = os.getenv("ANTHROPIC_API_KEY")
        if not api_key: logger.critical("ANTHROPIC_API_KEY not found."); return None
        return Anthropic(api_key=api_key)
    
    elif provider == "gemini":
        if not genai:
            logger.critical("Google GenAI SDK not found. Please run `pip install google-generativeai`."); return None
        api_key = os.getenv("GOOGLE_API_KEY")
        if not api_key:
            logger.critical("GOOGLE_API_KEY not found in .env file."); return None
        genai.configure(api_key=api_key)
        # We just return the configured module itself for Gemini
        return genai

    logger.critical(f"LLM provider '{provider}' is not supported."); return None

def normalize_spanish_lemma(lemma_str: str) -> str:
    s = lemma_str.lower().strip().split(' ')[0]
    s = (s.replace('á', 'a').replace('é', 'e').replace('í', 'i').replace('ó', 'o').replace('ú', 'u').replace('ñ', 'n').replace('ü', 'u'))
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