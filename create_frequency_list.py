# create_frequency_list.py

import argparse
import sys
from collections import Counter
from pathlib import Path
import unicodedata
import re

def normalize_and_clean_lemma(lemma_str: str) -> str:
    """
    Applies a series of cleaning and normalization steps to a raw lemma string.
    
    Returns a cleaned lemma string or an empty string if it's invalid.
    """
    # 1. Start with the raw string and convert to lowercase
    s = lemma_str.lower().strip()

    # 2. Handle specific, known replacements for archaic characters from Gutenberg texts
    s = s.replace('á', 'a').replace('é', 'e').replace('í', 'i').replace('ó', 'o').replace('ú', 'u')

    # 3. Strip any leading or trailing non-alphabetic characters.
    # This will remove '--' from '--sigue' and '.' from 'word.'
    # We specifically keep internal hyphens for now (e.g., 'well-being').
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)

    # If after stripping, the string is empty, it's invalid.
    if not s:
        return ""
        
    # 4. Normalize Unicode to NFC (Normalization Form C).
    # This is the standard form and merges characters and their accents into single code points.
    # It ensures that "e" + "´" becomes the single character "é". While we replaced these above,
    # this is good practice for other characters like 'ñ' (n + ~).
    s = unicodedata.normalize('NFC', s)
    
    # 5. Final check: ensure the resulting string doesn't contain invalid characters
    # (e.g., if punctuation was in the middle of a word). We only want letters,
    # and we can decide to allow hyphens.
    # This regex will find anything that is NOT a lowercase letter or a hyphen.
    if re.search(r'[^a-z-]', s):
        # This is a "garbage" lemma like 'hagas—y'. We discard it.
        # Returning an empty string is a signal to the calling code to ignore this lemma.
        return ""
        
    return s

# --- Configuration ---
OUTPUT_DIR = Path("assets")
OUTPUT_FILENAME = "es_master_frequency_list.txt"
# Use a tab character as a separator. It's standard for TSV files and won't be in a lemma.
SEPARATOR = "\t"

# Try to import tqdm for a nice progress bar, but make it optional.
try:
    from tqdm import tqdm
except ImportError:
    tqdm = None

# Try to import SpaCy
try:
    import spacy
except ImportError:
    print("CRITICAL ERROR: SpaCy library not found. Please run `pip install spacy`.", file=sys.stderr)
    sys.exit(1)


def create_frequency_list(corpus_dir: Path):
    """
    Reads all .txt files in a directory, lemmatizes them using SpaCy,
    and generates a ranked, counted frequency list.
    """
    if not corpus_dir.is_dir():
        print(f"ERROR: The specified directory does not exist: {corpus_dir}", file=sys.stderr)
        sys.exit(1)

    # --- 1. Load the SpaCy Model ---
    # We disable the parser and NER components as they are not needed for
    # lemmatization, which significantly speeds up the process.
    print("Loading SpaCy model (es_core_news_lg)... This may take a moment.")
    try:
        nlp = spacy.load("es_core_news_lg", disable=["parser", "ner"])
    except IOError:
        print("\n---")
        print("ERROR: Spanish SpaCy model 'es_core_news_lg' not found.", file=sys.stderr)
        print("Please run this command to download it:", file=sys.stderr)
        print("python -m spacy download es_core_news_lg", file=sys.stderr)
        sys.exit(1)
    print("SpaCy model loaded successfully.")

    # --- 2. Find and Count Files ---
    txt_files = list(corpus_dir.glob("*.txt"))
    if not txt_files:
        print(f"ERROR: No .txt files found in '{corpus_dir}'.", file=sys.stderr)
        sys.exit(1)

    print(f"\nFound {len(txt_files)} text files to process.")

    # --- 3. Process Files and Count Lemma Frequencies ---
    lemma_counts = Counter()
    
    # Use tqdm for a progress bar if available, otherwise just print filenames.
    iterable = tqdm(txt_files, desc="Processing files") if tqdm else txt_files

    for file_path in iterable:
        if not tqdm:
            print(f"  - Processing {file_path.name}...")
        
        try:
            with open(file_path, "r", encoding="utf-8") as f:
                text = f.read()
        except Exception as e:
            print(f"Warning: Could not read file {file_path.name}. Skipping. Error: {e}")
            continue

        # Process the text in batches using nlp.pipe for memory efficiency.
        # This is crucial for handling very large text files.
        for doc in nlp.pipe(text.splitlines(), batch_size=50):
            # Create a list of valid lemmas from the document
            lemmas_in_doc_raw = [
                token.lemma_.lower()
                for token in doc
                if not token.is_punct and not token.is_space and token.pos_ != "PROPN"
            ]
            # Update the master counter with the lemmas from this document
            cleaned_lemmas = [
                cleaned for s in lemmas_in_doc_raw 
                if (cleaned := normalize_and_clean_lemma(s)) # Python 3.8+ walrus operator
            ]
            lemma_counts.update(cleaned_lemmas)

    if not lemma_counts:
        print("ERROR: No valid lemmas were found in any of the processed files.", file=sys.stderr)
        sys.exit(1)
        
    print(f"\nProcessing complete. Found {len(lemma_counts)} unique lemmas.")

    # --- 4. Sort Lemmas by Frequency ---
    # .most_common() returns a list of (element, count) tuples, sorted by count descending.
    sorted_lemmas = lemma_counts.most_common()

    # --- 5. Write the Output File ---
    OUTPUT_DIR.mkdir(exist_ok=True)
    output_path = OUTPUT_DIR / OUTPUT_FILENAME
    print(f"Writing frequency list to: {output_path}")

    try:
        with open(output_path, "w", encoding="utf-8") as f:
            f.write(f"lemma{SEPARATOR}rank{SEPARATOR}occurrences\n") # Write a header row
            for rank, (lemma, count) in enumerate(sorted_lemmas, 1):
                f.write(f"{lemma}{SEPARATOR}{rank}{SEPARATOR}{count}\n")
    except Exception as e:
        print(f"ERROR: Could not write to output file. Error: {e}", file=sys.stderr)
        sys.exit(1)
        
    print("Frequency list created successfully.")
    
    # --- 6. Calculate and Display Lexical Coverage ---
    print("\n--- Corpus Lexical Coverage Analysis ---")
    total_token_count = sum(lemma_counts.values())
    print(f"Total valid tokens in corpus: {total_token_count:,}")
    
    cumulative_count = 0
    coverage_thresholds = {
        0.80: None, 0.90: None, 0.95: None, 0.98: None, 0.99: None
    }
    
    for rank, (lemma, count) in enumerate(sorted_lemmas, 1):
        cumulative_count += count
        coverage = cumulative_count / total_token_count
        
        for threshold, value in coverage_thresholds.items():
            if value is None and coverage >= threshold:
                coverage_thresholds[threshold] = rank
    
    for threshold, rank_needed in coverage_thresholds.items():
        if rank_needed is not None:
            print(f"To understand {threshold:.0%} of the text, you need to know the top {rank_needed:,} lemmas.")
        else:
            print(f"The corpus is not large enough to calculate the {threshold:.0%} coverage.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Generate a master lemma frequency list from a corpus of text files."
    )
    parser.add_argument(
        "corpus_dir",
        type=Path,
        help="The path to the directory containing the corpus .txt files."
    )
    args = parser.parse_args()
    
    create_frequency_list(args.corpus_dir)