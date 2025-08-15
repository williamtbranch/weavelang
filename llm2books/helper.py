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
# try:
#     # --- THIS IS THE FIX ---
#     # We now specifically import the Synchronous client
#     from anthropic import SyncAnthropic
# except ImportError:
#     # Set it to None if the library isn't installed
#     SyncAnthropic = None

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
        try:
            # --- THE REAL FIX IS HERE ---
            # The correct synchronous client is just called 'Anthropic'
            from anthropic import Anthropic
        except ImportError:
            logger.critical("Anthropic provider selected, but SDK not found or is not visible to the Python interpreter. Run 'pip install anthropic'")
            return None
            
        api_key = os.getenv("ANTHROPIC_API_KEY")
        if not api_key:
            logger.critical("ANTHROPIC_API_KEY not found in environment or .env file.")
            return None
            
        # --- AND HERE ---
        # Instantiate the correct class
        return Anthropic(api_key=api_key)
        
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