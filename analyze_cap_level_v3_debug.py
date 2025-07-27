# analyze_cap_level_v3_debug.py
"""
Analyzes JSON book files with a debug mode to inspect high-rank lemmas.
"""
import argparse
import json
import sys
from pathlib import Path
from collections import defaultdict
import re
import unicodedata

# --- Configuration ---
CHECKPOINT_LEVELS = [2000, 3000, 4000, 5000, 10000, 15000, 20000, 25000, 30000, 35000, 40000, 45000, 50000, 55000, 60000]
DEFAULT_FREQ_LIST = "assets/es_master_frequency_list.txt"

def normalize_spanish_lemma(lemma_str: str) -> str:
    s = lemma_str.lower().strip()
    s = s.replace('á', 'a').replace('é', 'e').replace('í', 'i').replace('ó', 'o').replace('ú', 'u')
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    if not s: return ""
    s = unicodedata.normalize('NFC', s)
    if re.search(r'[^a-z-]', s): return ""
    return s

def load_frequency_list(freq_list_path: Path) -> dict[str, int] | None:
    if not freq_list_path.is_file():
        print(f"ERROR: Frequency list not found at '{freq_list_path}'", file=sys.stderr)
        return None
    print(f"Loading frequency list from '{freq_list_path}'...")
    lemma_to_rank = {}
    try:
        with open(freq_list_path, "r", encoding="utf-8") as f:
            next(f)
            for line in f:
                parts = line.strip().split("\t")
                if len(parts) >= 2:
                    lemma = normalize_spanish_lemma(parts[0])
                    if not lemma: continue
                    try:
                        rank = int(parts[1])
                        lemma_to_rank[lemma] = rank
                    except ValueError:
                        continue
    except IOError as e:
        print(f"ERROR: Could not read frequency list file: {e}", file=sys.stderr)
        return None
    print(f"Successfully loaded {len(lemma_to_rank):,} normalized lemmas and their ranks.")
    return lemma_to_rank

def analyze_book(json_path: Path, lemma_to_rank: dict[str, int], target_field: str, debug_limit: int):
    print(f"\n--- Analyzing Book: {json_path.stem} (Target Field: {target_field}) ---")
    try:
        with open(json_path, "r", encoding="utf-8") as f: data = json.load(f)
    except (IOError, json.JSONDecodeError) as e:
        print(f"  Could not read or parse JSON file. Skipping. Error: {e}")
        return

    sentence_count = 0
    failure_counts = defaultdict(int)
    debug_sentences_printed = 0

    if debug_limit > 0:
        print(f"\n--- DEBUG: First {debug_limit} Sentences ---")

    for block in data.get("content_blocks", []):
        if block.get("block_type") == "sentence":
            lemmas = block.get(target_field, {}).get("lemmas", [])
            if not lemmas: continue
            sentence_count += 1
            max_rank_in_sentence = 0
            
            # --- DEBUG LOGIC ---
            if debug_sentences_printed < debug_limit:
                s_id = block.get('original_sentence_s_id', 'N/A')
                print(f"\n[S-ID: {s_id}] Text: \"{block.get(target_field, {}).get('text', '')}\"")
                problem_lemmas = []

            for lemma in lemmas:
                rank = lemma_to_rank.get(lemma, sys.maxsize)
                if rank > max_rank_in_sentence:
                    max_rank_in_sentence = rank
                
                # --- DEBUG LOGIC ---
                if debug_sentences_printed < debug_limit and rank > 5000: # Log lemmas rarer than 5000
                    problem_lemmas.append(f"'{lemma}' (Rank: {rank if rank != sys.maxsize else 'Not Found'})")

            # --- DEBUG LOGIC ---
            if debug_sentences_printed < debug_limit:
                if problem_lemmas:
                    print(f"  -> Problematic Lemmas: {', '.join(problem_lemmas)}")
                else:
                    print("  -> All lemmas are common (rank <= 5000).")
                debug_sentences_printed += 1

            for level in CHECKPOINT_LEVELS:
                if max_rank_in_sentence > level:
                    failure_counts[level] += 1
    
    if debug_limit > 0:
        print("\n--- END DEBUG ---")

    if sentence_count == 0:
        print("  No processable sentences found in this book.")
        return

    print(f"\nTotal Sentences Analyzed: {sentence_count}")
    print("-----------------------------------------------------------------")
    print("| Cap Level | Failing Sentences         | Failure Rate          |")
    print("-----------------------------------------------------------------")
    for level in CHECKPOINT_LEVELS:
        fails = failure_counts[level]
        percentage = (fails / sentence_count) * 100
        print(
            f"| {level:<9} | {fails:>6} / {sentence_count:<6} sentences | {percentage:>6.2f}% ({'#' * int(percentage / 4):<25})|"
        )
    print("-----------------------------------------------------------------")


def main():
    parser = argparse.ArgumentParser(description="Analyze WeaveLang JSON book vocabulary levels.")
    parser.add_argument("content_project_dir", type=Path)
    parser.add_argument("--library-subdir", default="library")
    parser.add_argument(
        "--target-field",
        default="simpler_adv_spanish_full",
        choices=["simpler_adv_spanish_full", "simple_spanish_l3_full"],
        help="The JSON field to analyze."
    )
    # --- NEW DEBUG ARGUMENT ---
    parser.add_argument(
        "--debug-limit",
        type=int,
        default=0,
        help="Print detailed lemma/rank analysis for the first N sentences of each book."
    )
    args = parser.parse_args()

    # (main function logic is the same as before)
    if not args.content_project_dir.is_dir():
        print(f"ERROR: Content project directory not found at '{args.content_project_dir}'", file=sys.stderr); sys.exit(1)
    library_path = args.content_project_dir / args.library_subdir
    if not library_path.is_dir():
        print(f"ERROR: Library subdirectory not found at '{library_path}'", file=sys.stderr); sys.exit(1)
    freq_list_path = args.content_project_dir / DEFAULT_FREQ_LIST
    lemma_to_rank_map = load_frequency_list(freq_list_path)
    if lemma_to_rank_map is None: sys.exit(1)
    json_files = sorted(list(library_path.glob("*.json")))
    if not json_files:
        print(f"WARNING: No .json files found in '{library_path}'.", file=sys.stderr); sys.exit(0)
    print(f"\nFound {len(json_files)} book(s) to analyze in '{library_path}'.")
    for json_path in json_files:
        analyze_book(json_path, lemma_to_rank_map, args.target_field, args.debug_limit)
    print("\n--- Analysis Complete ---")

if __name__ == "__main__":
    main()