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
from typing import List

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

def segment_text(doc: Doc, language: str) -> List[str]:
    """
    A generic text segmenter that chunks a SpaCy Doc and merges short fragments.
    """
    if language == "es":
        conjunctions = SPANISH_CONJUNCTIONS
    elif language == "en":
        conjunctions = ENGLISH_CONJUNCTIONS
    else:
        conjunctions = []
    syntactic_chunks = _get_syntactic_chunks(doc)
    merged_phrases = _merge_short_chunks(syntactic_chunks, conjunctions)
    return merged_phrases


def _get_syntactic_chunks(doc: Doc) -> List[Span]:
    """(Private) Identifies split points based on syntactic dependencies."""
    split_points = set()
    for token in doc:
        if token.dep_ == "cc":
            split_points.add(token.i)
        if token.dep_ == "mark":
            split_points.add(token.i)
        if token.pos_ == "ADP" and token.head.pos_ in ["VERB", "NOUN", "PROPN"]:
            if token.i > 0 and len([t for t in doc[0 : token.i] if not t.is_punct]) > 1:
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