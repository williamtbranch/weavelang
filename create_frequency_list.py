# create_frequency_list.py (v2 - Resumable and Robust)

import argparse
import sys
import pickle
from collections import Counter
from pathlib import Path
import unicodedata
import re
import os # For getting file size

# --- Normalization Function (kept the same) ---
def normalize_and_clean_lemma(lemma_str: str) -> str:
    """
    Applies a series of cleaning and normalization steps to a raw Spanish lemma string,
    handling all standard accented vowels, the ñ, and the ü with diaeresis.
    """
    # 1. Lowercase, strip whitespace, and handle multi-word lemmas by taking the first word.
    s = lemma_str.lower().strip().split(' ')[0]
    
    # 2. Explicitly replace all special Spanish characters with their base Latin equivalents.
    #    This is the core of the fix.
    s = (s.replace('á', 'a')
          .replace('é', 'e')
          .replace('í', 'i')
          .replace('ó', 'o')
          .replace('ú', 'u')
          .replace('ñ', 'n')  # Handles words like 'año' -> 'ano'
          .replace('ü', 'u')) # Handles words like 'pingüino' -> 'pinguino'

    # 3. Strip any remaining non-word characters from the start and end (handles ¿, ¡, etc.)
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    
    # 4. If the string is now empty (e.g., it was just punctuation), return.
    if not s: 
        return ""
        
    # 5. Standard Unicode normalization for consistency.
    s = unicodedata.normalize('NFC', s)
    
    # 6. Final validation. Because we converted ñ->n and ü->u, this regex now works correctly.
    #    It ensures we only have a-z and hyphens, discarding any truly strange characters.
    if re.search(r'[^a-z-]', s): 
        return ""
        
    return s

# --- Configuration ---
OUTPUT_DIR = Path("assets")
OUTPUT_FILENAME = "es_master_frequency_list.txt"
SEPARATOR = "\t"
CHECKPOINT_FILENAME = "_freq_list_checkpoint.pkl" # File to save progress

try:
    from tqdm import tqdm
except ImportError:
    tqdm = None

try:
    import spacy
except ImportError:
    print("CRITICAL ERROR: SpaCy library not found. Please run `pip install spacy`.", file=sys.stderr)
    sys.exit(1)


def save_checkpoint(filepath: Path, file_index: int, counts: Counter):
    """Saves the current progress to a pickle file."""
    state = {
        'last_processed_file_index': file_index,
        'lemma_counts': counts,
    }
    try:
        with open(filepath, 'wb') as f:
            pickle.dump(state, f)
        # Use .write() for tqdm to avoid breaking the progress bar
        if tqdm:
            tqdm.write(f"      [Checkpoint saved after processing file index {file_index}]")
    except Exception as e:
        if tqdm:
            tqdm.write(f"      [WARNING: Could not save checkpoint: {e}]")

def load_checkpoint(filepath: Path) -> tuple[int, Counter]:
    """Loads progress from a pickle file."""
    if filepath.exists():
        try:
            with open(filepath, 'rb') as f:
                state = pickle.load(f)
            print(f"--- Checkpoint found at '{filepath}'. Resuming progress. ---")
            last_index = state.get('last_processed_file_index', -1)
            counts = state.get('lemma_counts', Counter())
            print(f"Resuming from file index {last_index + 1}. Loaded {len(counts)} existing lemma counts.")
            return last_index, counts
        except Exception as e:
            print(f"Warning: Could not load checkpoint file '{filepath}': {e}. Starting fresh.")
    return -1, Counter() # Return -1 to indicate no files have been processed yet

def create_frequency_list(corpus_dir: Path):
    """
    Reads all .txt files in a directory, lemmatizes them using SpaCy,
    and generates a ranked, counted frequency list with resumability.
    """
    if not corpus_dir.is_dir():
        print(f"ERROR: The specified directory does not exist: {corpus_dir}", file=sys.stderr)
        sys.exit(1)

    checkpoint_path = corpus_dir / CHECKPOINT_FILENAME
    start_from_index, lemma_counts = load_checkpoint(checkpoint_path)

    print("Loading SpaCy model (es_core_news_lg)... This may take a moment.")
    try:
        # Lowering batch_size for nlp.pipe as requested, to reduce memory pressure.
        nlp = spacy.load("es_core_news_lg", disable=["parser", "ner"])
    except IOError:
        print("\n---", file=sys.stderr)
        print("ERROR: Spanish SpaCy model 'es_core_news_lg' not found.", file=sys.stderr)
        print("Please run this command to download it:", file=sys.stderr)
        print("python -m spacy download es_core_news_lg", file=sys.stderr)
        sys.exit(1)
    print("SpaCy model loaded successfully.")

    txt_files = sorted(list(corpus_dir.glob("*.txt")))
    if not txt_files:
        print(f"ERROR: No .txt files found in '{corpus_dir}'.", file=sys.stderr)
        sys.exit(1)
    
    files_to_process = txt_files[start_from_index + 1:]
    if not files_to_process and start_from_index >= len(txt_files) -1 :
        print("All files have already been processed according to the checkpoint.")
    elif not files_to_process:
        print("Warning: No files left to process based on checkpoint. Consider deleting the checkpoint to restart.")
        # Proceed to finalization step with loaded data
    else:
        print(f"\nFound {len(txt_files)} total text files. Resuming with {len(files_to_process)} remaining files.")

    # --- Byte-based Progress Bar Setup ---
    total_bytes_to_process = sum(os.path.getsize(f) for f in files_to_process)
    processed_bytes = 0
    
    progress_bar = None
    if tqdm:
        progress_bar = tqdm(total=total_bytes_to_process, unit='B', unit_scale=True, desc="Processing Corpus")

    try:
        for i, file_path in enumerate(files_to_process):
            current_file_index = start_from_index + 1 + i
            file_size = os.path.getsize(file_path)

            if progress_bar:
                progress_bar.set_description(f"Processing {file_path.name}")
            else:
                print(f"  - Processing {file_path.name}...")
            
            try:
                with open(file_path, "r", encoding="utf-8", errors='ignore') as f:
                    # Use a generator to avoid loading the whole file into memory at once
                    line_generator = (line for line in f)
                    
                    for doc in nlp.pipe(line_generator, batch_size=400, n_process=-1):
                        lemmas_in_doc_raw = [
                            token.lemma_.lower()
                            for token in doc
                            if not token.is_punct and not token.is_space and token.pos_ != "PROPN"
                        ]
                        cleaned_lemmas = [
                            cleaned for s in lemmas_in_doc_raw 
                            if (cleaned := normalize_and_clean_lemma(s))
                        ]
                        lemma_counts.update(cleaned_lemmas)
                
                # After successfully processing a file, save a checkpoint.
                save_checkpoint(checkpoint_path, current_file_index, lemma_counts)

            except Exception as e:
                # On error, save progress and exit gracefully.
                print(f"\nCRITICAL ERROR: Could not read or process file {file_path.name}. Error: {e}", file=sys.stderr)
                print("Saving progress before exiting. You can fix the issue and restart the script to resume.", file=sys.stderr)
                save_checkpoint(checkpoint_path, current_file_index -1, lemma_counts) # Save progress up to the file *before* the failing one.
                sys.exit(1)

            # Update the byte-based progress bar
            if progress_bar:
                progress_bar.update(file_size)
            processed_bytes += file_size

    finally:
        if progress_bar:
            progress_bar.close()
    
    if not lemma_counts:
        print("ERROR: No valid lemmas were found in any of the processed files.", file=sys.stderr)
        sys.exit(1)
        
    print(f"\nProcessing complete. Found {len(lemma_counts)} unique lemmas in total.")

    # --- Finalization (Writing the output file) ---
    sorted_lemmas = lemma_counts.most_common()

    OUTPUT_DIR.mkdir(exist_ok=True)
    output_path = OUTPUT_DIR / OUTPUT_FILENAME
    print(f"Writing final frequency list to: {output_path}")

    try:
        with open(output_path, "w", encoding="utf-8") as f:
            f.write(f"lemma{SEPARATOR}rank{SEPARATOR}occurrences\n")
            
            write_iterable = tqdm(enumerate(sorted_lemmas, 1), total=len(sorted_lemmas), desc="Writing output") if tqdm else enumerate(sorted_lemmas, 1)
            for rank, (lemma, count) in write_iterable:
                f.write(f"{lemma}{SEPARATOR}{rank}{SEPARATOR}{count}\n")
    except Exception as e:
        print(f"ERROR: Could not write to output file. Error: {e}", file=sys.stderr)
        sys.exit(1)
        
    print("Frequency list created successfully.")
    
    # Clean up the checkpoint file on successful completion
    try:
        if checkpoint_path.exists():
            os.remove(checkpoint_path)
            print(f"Successfully removed checkpoint file: '{checkpoint_path}'")
    except Exception as e:
        print(f"Warning: Could not remove checkpoint file. You may want to delete it manually. Error: {e}")

    # (Lexical coverage analysis remains the same)
    print("\n--- Corpus Lexical Coverage Analysis ---")
    total_token_count = sum(lemma_counts.values())
    print(f"Total valid tokens in corpus: {total_token_count:,}")
    cumulative_count = 0
    coverage_thresholds = {0.80: None, 0.90: None, 0.95: None, 0.98: None, 0.99: None}
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
        description="Generate a master lemma frequency list from a corpus of text files with resumability."
    )
    parser.add_argument(
        "corpus_dir",
        type=Path,
        help="The path to the directory containing the corpus .txt files."
    )
    args = parser.parse_args()
    
    create_frequency_list(args.corpus_dir)