# llm2books/helper.py
#
# Contains genuinely reusable, high-level functions and constants for the
# WeaveLang data generation pipeline.
import re
import unicodedata # NEW IMPORT
import argparse
import logging
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import List, Dict, Any

from dotenv import load_dotenv
from spacy.tokens import Doc, Span

# --- Attempt to import optional libraries ---
try:
    import anthropic
except ImportError:
    anthropic = None

# --- Global Constants ---
SPANISH_CONJUNCTIONS = ["y", "o", "pero", "que", "si", "cuando", "pues"]
ENGLISH_CONJUNCTIONS = ["and", "or", "but", "so", "that", "if", "when", "as"]

TITLE_SENTENCE_REGEX = re.compile(
    r"^\s*(chapter|book|part|section|preface|epilogue|prologue|contents|author|by)\b",
    re.IGNORECASE
)

# Get the pipeline's logger
logger = logging.getLogger("pipeline")


# --- General Utility Functions ---

def get_iso_timestamp() -> str:
    """Returns the current UTC time in ISO 8601 format."""
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def initialize_llm_client(provider: str) -> any:
    """
    Initializes and returns an LLM client based on the provider string.
    """
    load_dotenv(dotenv_path=Path.cwd() / ".env")
    if provider == "claude":
        if not anthropic:
            logger.critical("Anthropic provider selected, but SDK not installed. Run `pip install anthropic`")
            return None
        api_key = os.getenv("ANTHROPIC_API_KEY")
        if not api_key:
            logger.critical("ANTHROPIC_API_KEY not found in environment or .env file.")
            return None
        return anthropic.Anthropic(api_key=api_key)
    logger.critical(f"LLM provider '{provider}' is not supported.")
    return None

# --- NEW: Centralized Lemma Normalization Function ---
def normalize_spanish_lemma(lemma_str: str) -> str:
    """
    Applies a series of cleaning and normalization steps to a raw lemma string.
    This logic MUST be kept in sync with the frequency list generator.
    
    Returns a cleaned lemma string or an empty string if it's invalid.
    """
    # 1. Start with the raw string and convert to lowercase
    s = lemma_str.lower().strip()
    s = s.split(' ')[0]

    # 2. Handle specific, known replacements first.
    s = s.replace('á', 'a').replace('é', 'e').replace('í', 'i').replace('ó', 'o').replace('ú', 'u')

    # 3. Strip any leading or trailing non-alphabetic characters.
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    if not s:
        return ""
        
    # 4. Normalize Unicode to NFC (Normalization Form C).
    s = unicodedata.normalize('NFC', s)
    
    # 5. Final check: ensure the resulting string doesn't contain invalid characters.
    # We only want letters and internal hyphens.
    if re.search(r'[^a-z-]', s):
        return ""
        
    return s

# --- Text Segmentation Logic ---
def segment_text(doc: Doc, language: str, min_words: int = 2) -> List[Span]:
    """
    Segments a SpaCy Doc into syntactic chunks (Spans), enforcing a minimum
    word count per segment to prevent over-splitting.
    """
    # 1. Identify all potential split points based on syntactic rules.
    potential_split_points = set()
    for token in doc:
        # Split on conjunctions or markers
        if token.dep_ in ("cc", "mark"):
            potential_split_points.add(token.i)
        # Split before clausal complements
        if token.dep_ == "ccomp" and token.i > 0:
            potential_split_points.add(token.i)
        # Split on prepositions
        if token.pos_ == "ADP" and token.head.pos_ in ["VERB", "NOUN", "PROPN"]:
            if token.i > 0:
                potential_split_points.add(token.i)

    if not potential_split_points:
        return [doc[:]] # Return the whole sentence as one segment

    sorted_points = sorted(list(potential_split_points))

    # 2. Create the final list of spans, merging short segments on-the-fly.
    final_spans = []
    start_idx = 0
    
    for point in sorted_points:
        if point <= start_idx:
            continue
            
        candidate_span = doc[start_idx:point]
        
        # Count non-punctuation/space words in the candidate span
        word_count = sum(1 for t in candidate_span if not t.is_punct and not t.is_space)
        
        # --- This is the merge logic, simplified ---
        # If the segment we just created is too short, we don't add it.
        # We effectively extend the *next* segment to include this one by NOT
        # updating the start_idx.
        if word_count >= min_words:
            final_spans.append(candidate_span)
            start_idx = point

    # 3. Add the final segment from the last valid split point to the end.
    if start_idx < len(doc):
        final_spans.append(doc[start_idx:])
        
    return final_spans

def _get_syntactic_chunks(doc: Doc) -> List[Span]:
    """(Private) Identifies split points based on syntactic dependencies and returns Spans."""
    split_points = set()
    for token in doc:
        # Splitting on conjunctions and subordinate conjunctions
        if token.dep_ in ("cc", "mark"):
            split_points.add(token.i)
            
        # --- THIS IS THE CRUCIAL RULE ---
        # Splitting before clausal complements handles "He said, 'Go away'"
        if token.dep_ == "ccomp" and token.i > 0:
            split_points.add(token.i)
            
        # Splitting on prepositions that introduce significant clauses
        if token.pos_ == "ADP" and token.head.pos_ in ["VERB", "NOUN", "PROPN"]:
            if token.i > 0:
                split_points.add(token.i)
    
    sorted_split_points = sorted(list(split_points))
    
    final_chunks = []
    start = 0
    for point in sorted_split_points:
        if start < point:
            final_chunks.append(doc[start:point])
        start = point
    if start < len(doc):
        final_chunks.append(doc[start:])
        
    return final_chunks


def _merge_short_spans(
    chunks: List[Span], conjunctions: List[str], min_words: int = 2
) -> List[Span]:
    """(Private) Merges overly short SpaCy Spans for better coherence."""
    if len(chunks) < 2:
        return chunks

    spans = list(chunks)
    
    # Merge conjunctions first. e.g., ["He saw", "and", "ran away"] -> ["He saw and", "ran away"]
    i = 0
    while i < len(spans):
        # A span is a conjunction span if it contains only one token and that token is a conjunction
        is_conjunction_span = len(spans[i]) == 1 and spans[i].text.lower() in conjunctions
        
        if is_conjunction_span and i > 0:
            # Merge with the PREVIOUS span
            merged_span = spans[i].doc[spans[i-1].start : spans[i].end]
            spans[i-1] = merged_span
            spans.pop(i)
            # Do not increment i, as the new merged span might need to be evaluated again
        else:
            i += 1

    # Iteratively merge short fragments. e.g. ["He ran", "away"] -> ["He ran away"]
    made_a_merge = True
    while made_a_merge:
        made_a_merge = False
        i = 1
        if len(spans) < 2:
            break
        while i < len(spans):
            word_count = sum(1 for token in spans[i] if not token.is_punct and not token.is_space)
            if word_count < min_words:
                # Merge with the PREVIOUS span
                merged_span = spans[i].doc[spans[i-1].start : spans[i].end]
                spans[i-1] = merged_span
                spans.pop(i)
                made_a_merge = True
                break # Restart the inner loop
            else:
                i += 1
                
    return spans


def _merge_short_chunks(
    chunks: List[Span], conjunctions: List[str], min_words: int = 2
) -> List[str]:
    """(Private) Merges overly short chunks into preceding chunks for better coherence."""
    if not chunks:
        return []
    texts = [chunk.text.strip() for chunk in chunks]
    i = 0
    while i < len(texts) - 1:
        cleaned_chunk = "".join(c for c in texts[i] if c.isalnum()).lower()
        if cleaned_chunk in conjunctions:
            texts[i + 1] = f"{texts[i]} {texts[i + 1]}"
            texts.pop(i)
        else:
            i += 1
    made_a_merge = True
    while made_a_merge:
        made_a_merge = False
        i = 1
        if len(texts) <= 1:
            break
        while i < len(texts):
            if len(texts[i].split()) < min_words:
                texts[i - 1] = f"{texts[i - 1]} {texts[i]}"
                texts.pop(i)
                made_a_merge = True
                break
            else:
                i += 1
    return texts
def create_v2_token_list(span: Span) -> List[Dict[str, Any]]:
    """
    Creates the V2 token list from a SpaCy Span object by classifying each
    token and then merging consecutive background elements. This is the
    definitive, correct implementation.
    """
    if not span:
        return [{"t": "b", "v": ""}]
    
    # Step 1: Classify every single token from the span.
    raw_tokens = []
    for token in span:
        # Punctuation and spaces are background; everything else is a word.
        if token.is_punct or token.is_space:
            raw_tokens.append({"t": "b", "v": token.text_with_ws})
        else:
            raw_tokens.append({"t": "w", "v": token.text})
            # Also capture the word's trailing space as a separate background token.
            if token.whitespace_:
                raw_tokens.append({"t": "b", "v": token.whitespace_})

    if not raw_tokens:
        return [{"t": "b", "v": span.text_with_ws}]

    # Step 2: Merge any consecutive background tokens.
    # This is the key step that combines things like [word, ' ', ','] into [word, ' ,']
    merged = []
    for token in raw_tokens:
        if token["t"] == "b" and merged and merged[-1]["t"] == "b":
            merged[-1]["v"] += token["v"]
        else:
            merged.append(token)

    # Step 3: Ensure the BWBWB invariant is met.
    if merged and merged[0]["t"] == "w":
        merged.insert(0, {"t": "b", "v": ""})
    if merged and merged[-1]["t"] == "w":
        merged.append({"t": "b", "v": ""})
        
    return merged