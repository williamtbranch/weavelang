# repair_json_lemmas.py
"""
A one-off utility script for the WeaveLang project.

This script finds completed JSON files (e.g., in the 'library' directory) and
applies the canonical 'normalize_spanish_lemma' function to every lemma found
within the file's data structure. This is necessary to fix data generated with
older pipeline versions that may not have correctly stripped accents.

**WARNING: This script modifies files in place.
          MAKE A BACKUP of your target directory before running.**
"""
import argparse
import json
import re
import unicodedata
from pathlib import Path

# --- Canonical Normalization Function (from llm2books/helper.py) ---
def normalize_spanish_lemma(lemma_str: str) -> str:
    s = lemma_str.lower().strip()
    s = s.replace('á', 'a').replace('é', 'e').replace('í', 'i').replace('ó', 'o').replace('ú', 'u')
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    if not s:
        return ""
    s = unicodedata.normalize('NFC', s)
    if re.search(r'[^a-z-]', s):
        return ""
    return s

def repair_json_file(file_path: Path) -> tuple[int, int]:
    """Loads a single JSON file, repairs all lemma lists, and saves it back."""
    lemmas_repaired = 0
    total_lemmas = 0
    
    try:
        with open(file_path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (IOError, json.JSONDecodeError) as e:
        print(f"  - Skipping {file_path.name}: Could not read/parse. Error: {e}")
        return 0, 0

    content_blocks = data.get("content_blocks", [])
    for block in content_blocks:
        if block.get("block_type") == "sentence":
            # List of paths to lemma lists within the block
            lemma_paths = [
                ("adv_spanish_full", "lemmas"),
                ("simpler_adv_spanish_full", "lemmas"),
                ("simple_spanish_l3_full", "lemmas"),
            ]
            
            for key1, key2 in lemma_paths:
                if key1 in block and key2 in block[key1]:
                    old_lemmas = block[key1][key2]
                    total_lemmas += len(old_lemmas)
                    new_lemmas = [normalize_spanish_lemma(l) for l in old_lemmas]
                    if new_lemmas != old_lemmas:
                        lemmas_repaired += sum(1 for o, n in zip(old_lemmas, new_lemmas) if o != n)
                    block[key1][key2] = [l for l in new_lemmas if l] # Filter out any empty strings

            # Repair adv_spanish_segments
            for seg in block.get("adv_spanish_segments", []):
                for key in ["advanced_lemmas", "simpler_lemmas"]:
                    if key in seg:
                        old_lemmas = seg[key]
                        total_lemmas += len(old_lemmas)
                        new_lemmas = [normalize_spanish_lemma(l) for l in old_lemmas]
                        if new_lemmas != old_lemmas:
                            lemmas_repaired += sum(1 for o, n in zip(old_lemmas, new_lemmas) if o != n)
                        seg[key] = [l for l in new_lemmas if l]

            # Repair simple_spanish_l3_lemmas_per_segment (dict)
            if "simple_spanish_l3_lemmas_per_segment" in block:
                for seg_id, old_lemmas in block["simple_spanish_l3_lemmas_per_segment"].items():
                    total_lemmas += len(old_lemmas)
                    new_lemmas = [normalize_spanish_lemma(l) for l in old_lemmas]
                    if new_lemmas != old_lemmas:
                        lemmas_repaired += sum(1 for o, n in zip(old_lemmas, new_lemmas) if o != n)
                    block["simple_spanish_l3_lemmas_per_segment"][seg_id] = [l for l in new_lemmas if l]

    if lemmas_repaired > 0:
        try:
            with open(file_path, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2, ensure_ascii=False)
        except IOError as e:
            print(f"  - FAILED to write repaired file {file_path.name}. Error: {e}")
    
    return lemmas_repaired, total_lemmas

def main():
    parser = argparse.ArgumentParser(description="Repair WeaveLang JSON files by normalizing all lemmas.")
    parser.add_argument(
        "content_project_dir",
        type=Path,
        help="The absolute path to the content project directory (e.g., 'audiolingual')."
    )
    parser.add_argument(
        "--library-subdir",
        default="library",
        help="Subdirectory containing the final .json files to repair."
    )
    args = parser.parse_args()

    library_path = args.content_project_dir / args.library_subdir
    if not library_path.is_dir():
        print(f"ERROR: Library subdirectory not found at '{library_path}'")
        return

    json_files = sorted(list(library_path.glob("*.json")))
    if not json_files:
        print(f"WARNING: No .json files found in '{library_path}'. Nothing to do.")
        return
    
    print("--- Starting JSON Lemma Repair Process ---")
    print("WARNING: This script will modify files in place. Make a backup first!")
    
    for file_path in json_files:
        print(f"Processing '{file_path.name}'...")
        repaired, total = repair_json_file(file_path)
        if repaired > 0:
            print(f"  -> Repaired {repaired} out of {total} total lemmas.")
        else:
            print("  -> No repairs needed.")

    print("\n--- Repair Process Complete ---")

if __name__ == "__main__":
    main()