# analyze_cap_level.py (v2 - With Normalization Fix)
"""
A one-off utility script for the WeaveLang project.

This script analyzes the final JSON book files to determine what percentage of
'Simpler Advanced Spanish' sentences would be incomprehensible at various
vocabulary thresholds (levels).

The output of this script provides the necessary data to choose a reasonable
"English Cap Level" for the main generation pipeline, identifying a sweet spot
where the number of non-compliant sentences is manageably low.
"""
import argparse
import json
import sys
from pathlib import Path
from collections import defaultdict
# --- NEW IMPORTS FOR NORMALIZATION ---
import re
import unicodedata

# --- Configuration ---
CHECKPOINT_LEVELS = [2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000]
DEFAULT_FREQ_LIST = "E:/Bill/Documents/development/audiolingual/assets/es_master_frequency_list.txt"

# --- NEW: Canonical Normalization Function (Copied from helper.py) ---
def normalize_spanish_lemma(lemma_str: str) -> str:
    """
    Applies a series of cleaning and normalization steps to a raw lemma string.
    This logic MUST be kept in sync with the rest of the pipeline.
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
    if not s: return ""
    s = unicodedata.normalize('NFC', s)
    if re.search(r'[^a-z-]', s): return ""
    return s

def load_frequency_list(freq_list_path: Path) -> dict[str, int] | None:
    """
    Loads the master frequency list into a dictionary mapping normalized lemmas to their ranks.
    """
    if not freq_list_path.is_file():
        print(f"ERROR: Frequency list not found at '{freq_list_path}'", file=sys.stderr)
        return None

    print(f"Loading frequency list from '{freq_list_path}'...")
    lemma_to_rank = {}
    try:
        with open(freq_list_path, "r", encoding="utf-8") as f:
            next(f)  # Skip header
            for line in f:
                parts = line.strip().split("\t")
                if len(parts) >= 2:
                    # --- MODIFIED LINE: Normalize the lemma before using it as a key ---
                    lemma = normalize_spanish_lemma(parts[0])
                    if not lemma:
                        continue # Skip invalid lemmas
                    try:
                        rank = int(parts[1])
                        lemma_to_rank[lemma] = rank
                    except ValueError:
                        continue
    except IOError as e:
        print(f"ERROR: Could not read frequency list file: {e}", file=sys.stderr)
        return None

    if not lemma_to_rank:
        print("ERROR: Frequency list is empty or could not be parsed.", file=sys.stderr)
        return None

    print(f"Successfully loaded {len(lemma_to_rank):,} normalized lemmas and their ranks.")
    return lemma_to_rank


def analyze_book(json_path: Path, lemma_to_rank: dict[str, int]):
    """
    Analyzes a single book's JSON file and prints its failure rates.
    """
    print(f"\n--- Analyzing Book: {json_path.stem} ---")
    try:
        with open(json_path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (IOError, json.JSONDecodeError) as e:
        print(f"  Could not read or parse JSON file. Skipping. Error: {e}")
        return

    sentence_count = 0
    failure_counts = defaultdict(int)

    for block in data.get("content_blocks", []):
        if block.get("block_type") == "sentence":
            # --- THIS IS THE FIELD WE ARE ANALYZING ---
            lemmas = block.get("simpler_adv_spanish_full", {}).get("lemmas", [])
            if not lemmas:
                continue

            sentence_count += 1
            max_rank_in_sentence = 0
            for lemma in lemmas:
                # The lemmas from the JSON are already normalized by the pipeline,
                # so we can look them up directly.
                rank = lemma_to_rank.get(lemma, sys.maxsize)
                if rank > max_rank_in_sentence:
                    max_rank_in_sentence = rank

            for level in CHECKPOINT_LEVELS:
                if max_rank_in_sentence > level:
                    failure_counts[level] += 1

    if sentence_count == 0:
        print("  No processable sentences found in this book.")
        return

    print(f"Total Sentences Analyzed: {sentence_count}")
    print("-----------------------------------------------------------------")
    print("| Cap Level | Failing Sentences         | Failure Rate          |")
    print("-----------------------------------------------------------------")
    for level in CHECKPOINT_LEVELS:
        fails = failure_counts[level]
        percentage = (fails / sentence_count) * 100
        # Reduced the bar graph width for better readability
        print(
            f"| {level:<9} | {fails:>6} / {sentence_count:<6} sentences | {percentage:>6.2f}% ({'#' * int(percentage / 4):<25})|"
        )
    print("-----------------------------------------------------------------")


def main():
    """
    Main function to orchestrate the analysis.
    """
    parser = argparse.ArgumentParser(
        description="Analyze WeaveLang JSON books to find 'Simpler Spanish' failure rates at different vocabulary levels.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "content_project_dir",
        type=Path,
        help="The absolute path to the content project directory (e.g., 'audiolingual').",
    )
    parser.add_argument(
        "--library-subdir",
        default="library",
        help="Subdirectory within the content project containing the final .json files to analyze.",
    )
    args = parser.parse_args()

    if not args.content_project_dir.is_dir():
        print(f"ERROR: Provided content project directory not found at '{args.content_project_dir}'", file=sys.stderr)
        sys.exit(1)

    library_path = args.content_project_dir / args.library_subdir
    if not library_path.is_dir():
        print(f"ERROR: Library subdirectory not found at '{library_path}'", file=sys.stderr)
        sys.exit(1)

    freq_list_path = args.content_project_dir / DEFAULT_FREQ_LIST
    lemma_to_rank_map = load_frequency_list(freq_list_path)
    if lemma_to_rank_map is None:
        sys.exit(1)

    json_files = sorted(list(library_path.glob("*.json")))
    if not json_files:
        print(f"WARNING: No .json files found in '{library_path}'. Nothing to do.", file=sys.stderr)
        sys.exit(0)

    print(f"\nFound {len(json_files)} book(s) to analyze in '{library_path}'.")
    for json_path in json_files:
        analyze_book(json_path, lemma_to_rank_map)

    print("\n--- Analysis Complete ---")


if __name__ == "__main__":
    main()