# repair_stage10_json.py (v2 - More Robust)
"""
A one-off utility script for the WeaveLang project.

This script finds completed JSON files from the final pipeline stage (stage10)
and repairs the data structure of the `inverse_diglot_map`. It converts the
old `HashMap<String, String>` format into the new, correct
`Vec<JsonInverseDiglotMapEntry>` format.

**WARNING: This script modifies files in place.
          MAKE A BACKUP of your target directory before running.**
"""
import argparse
import json
import re
from pathlib import Path
import sys

# --- Dependency Check and Import ---
try:
    import spacy
    from tqdm import tqdm
    import toml
except ImportError as e:
    print(f"CRITICAL: A required library is missing. {e}", file=sys.stderr)
    print("Please run: pip install spacy tqdm toml", file=sys.stderr)
    print("You also need the Spanish model: python -m spacy download es_core_news_lg", file=sys.stderr)
    sys.exit(1)

# --- Normalization Logic (Copied from llm2books/helper.py for standalone use) ---
def normalize_spanish_lemma(lemma_str: str) -> str:
    import unicodedata
    s = lemma_str.lower().strip()
    s = s.replace('á', 'a').replace('é', 'e').replace('í', 'i').replace('ó', 'o').replace('ú', 'u')
    s = re.sub(r'^[^\w]+|[^\w]+$', '', s)
    if not s: return ""
    s = unicodedata.normalize('NFC', s)
    if re.search(r'[^a-z-]', s): return ""
    return s

def repair_json_file(file_path: Path, spacy_model, pbar):
    """Loads a single JSON file, repairs it, and saves it back."""
    try:
        with open(file_path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (IOError, json.JSONDecodeError) as e:
        pbar.write(f"  - Skipping {file_path.name}: Could not read or parse file. Error: {e}")
        return False

    repair_needed = False
    repaired_segment_count = 0

    content_blocks = data.get("content_blocks", [])
    for block in content_blocks:
        if block.get("block_type") == "sentence":
            for seg in block.get("adv_spanish_segments", []):
                # --- MODIFICATION: Added check for key existence ---
                if "inverse_diglot_map" not in seg:
                    continue # Skip segment if the key isn't even there

                inv_diglot_map = seg.get("inverse_diglot_map")
                
                if isinstance(inv_diglot_map, dict):
                    repair_needed = True
                    repaired_segment_count += 1
                    
                    new_map_entries = []
                    for spanish_word, english_sub in inv_diglot_map.items():
                        doc = spacy_model(spanish_word)
                        main_token = next((t for t in doc if not t.is_punct), None)
                        
                        if main_token:
                            lemma = normalize_spanish_lemma(main_token.lemma_)
                        else:
                            lemma = normalize_spanish_lemma(spanish_word)

                        new_map_entries.append({
                            "spanish_word": spanish_word,
                            "spanish_lemma": lemma,
                            "english_substitute": english_sub,
                        })
                    
                    seg["inverse_diglot_map"] = new_map_entries

    if repair_needed:
        try:
            with open(file_path, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2, ensure_ascii=False)
            # Use pbar.write to avoid interfering with the progress bar
            pbar.write(f"  - Repaired {file_path.name} ({repaired_segment_count} segments converted).")
            return True
        except IOError as e:
            pbar.write(f"  - FAILED to write repaired file {file_path.name}. Error: {e}")
            return False
    else:
        # This check is now more robust.
        is_already_repaired = False
        if content_blocks:
             first_sent = next((b for b in content_blocks if b.get("block_type") == "sentence"), None)
             if first_sent:
                 first_seg_list = first_sent.get("adv_spanish_segments", [])
                 if first_seg_list:
                     first_seg = first_seg_list[0]
                     if "inverse_diglot_map" in first_seg:
                        inv_map = first_seg.get("inverse_diglot_map")
                        if isinstance(inv_map, list):
                            is_already_repaired = True

        if is_already_repaired:
            pbar.write(f"  - Skipping {file_path.name}: Already in new format.")
        else:
            pbar.write(f"  - Skipping {file_path.name}: No repairable dictionary-format data found.")
        return False

def main():
    parser = argparse.ArgumentParser(
        description="Repair WeaveLang Stage 10 JSON files to the new V8 schema.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--config", default="config.toml", help="Path to the main project config file."
    )
    parser.add_argument(
        "--target_stage_num",
        type=int,
        default=10,
        help="The stage number of your final JSON files to repair.",
    )
    args = parser.parse_args()

    try:
        with open(args.config, "r", encoding="utf-8") as f:
            config = toml.load(f)
        content_project_dir_str = config.get("content_project_dir")
        if not content_project_dir_str:
            print(f"ERROR: 'content_project_dir' not found in '{args.config}'.", file=sys.stderr)
            sys.exit(1)
    except Exception as e:
        print(f"ERROR: Failed to load or parse config file '{args.config}': {e}", file=sys.stderr)
        sys.exit(1)

    print("Loading SpaCy model (es_core_news_lg)... This may take a moment.")
    try:
        spacy_es = spacy.load("es_core_news_lg")
    except IOError:
        print("\n---", file=sys.stderr)
        print("ERROR: Spanish SpaCy model 'es_core_news_lg' not found.", file=sys.stderr)
        print("Please run this command to download it:", file=sys.stderr)
        print("python -m spacy download es_core_news_lg", file=sys.stderr)
        sys.exit(1)
    print("SpaCy model loaded successfully.")

    content_project_path = Path(content_project_dir_str)
    pipeline_base_dir = content_project_path / "pipeline"
    target_dir = pipeline_base_dir / f"stage{args.target_stage_num}"
    
    # --- MODIFICATION: Added diagnostic prints ---
    print("\n--- Configuration ---")
    print(f"Scanning for JSON files in target directory: '{target_dir.resolve()}'")

    if not target_dir.is_dir():
        print(f"\nERROR: Target directory not found. Please check your path.", file=sys.stderr)
        sys.exit(1)

    # --- MODIFICATION: The glob pattern is now more flexible. ---
    # It will match 'Book.json' AND 'Book.stage10.json'
    json_files = sorted(list(target_dir.glob(f"*.json")))

    # --- MODIFICATION: Added diagnostic prints ---
    print(f"Found {len(json_files)} '.json' file(s) to check.")

    if not json_files:
        print(f"\nWARNING: No .json files found in the target directory. Nothing to do.")
        sys.exit(0)
    
    print(f"\nStarting repair process...")
    print("---")
    
    repaired_file_count = 0
    
    with tqdm(json_files, desc="Repairing files", unit="file") as pbar:
        for file_path in pbar:
            if repair_json_file(file_path, spacy_es, pbar):
                repaired_file_count += 1

    print("\n--- Repair Process Complete ---")
    print(f"Processed {len(json_files)} file(s).")
    print(f"Successfully repaired and saved {repaired_file_count} file(s).")
    print("Data migration is finished. You can now run the Rust generation engine.")

if __name__ == "__main__":
    main()