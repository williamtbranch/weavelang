# backfill_pipeline.py
"""
A one-off utility script for the WeaveLang project.

This script finds completed JSON files from a specified source stage (e.g., stage8)
and copies them to all preceding stage directories (e.g., stage1 to stage7).
This is necessary to "backfill" the pipeline's history so that the orchestrator's
resumability logic can correctly identify where to start processing when a new
stage is added to the end of the pipeline.
"""
import argparse
import shutil
import re
from pathlib import Path

# --- Configuration ---
try:
    import tomllib
except ImportError:
    try:
        import toml as tomllib
    except ImportError:
        print("CRITICAL: 'toml' library not found. Please run `pip install toml` for Python < 3.11.")
        exit(1)


def main():
    """Main function to run the backfill process."""
    parser = argparse.ArgumentParser(
        description="Backfill WeaveLang pipeline stages for resumability.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--config", default="config.toml", help="Path to the main project config file."
    )
    parser.add_argument(
        "--source-stage",
        type=int,
        default=8,
        help="The stage number of your existing, complete JSON files.",
    )
    parser.add_argument(
        "--fill-until-stage",
        type=int,
        default=8,
        help="The stage number *before* which directories will be filled (e.g., 8 fills stages 1-7).",
    )
    args = parser.parse_args()

    # --- Load Project Config ---
    try:
        with open(args.config, "rb") as f:
            config = tomllib.load(f)
        content_project_dir_str = config.get("content_project_dir")
        if not content_project_dir_str:
            print(f"ERROR: 'content_project_dir' not found in '{args.config}'.")
            exit(1)
    except FileNotFoundError:
        print(f"ERROR: Config file not found at '{args.config}'.")
        exit(1)
    except Exception as e:
        print(f"ERROR: Failed to load or parse config file '{args.config}': {e}")
        exit(1)

    # --- Set Up Paths ---
    content_project_path = Path(content_project_dir_str)
    pipeline_base_dir = content_project_path / "pipeline"
    source_dir = pipeline_base_dir / f"stage{args.source_stage}"

    if not source_dir.is_dir():
        print(f"ERROR: Source directory not found: '{source_dir}'")
        print(f"Please make sure your completed JSON files are in that directory.")
        exit(1)

    # Regex to extract the book stem from a filename like 'BookName.stage8.json'
    book_stem_regex = re.compile(r"^(.*?)\.stage\d+\.json$")
    
    # --- Main Logic ---
    print(f"Starting backfill process...")
    print(f"Reading from source: '{source_dir.relative_to(content_project_path)}'")
    
    source_files = list(source_dir.glob("*.json"))
    if not source_files:
        print("WARNING: No .json files found in the source directory. Nothing to do.")
        exit(0)

    book_count = 0
    for source_file_path in source_files:
        match = book_stem_regex.match(source_file_path.name)
        if not match:
            print(f"  - Skipping non-standard file: {source_file_path.name}")
            continue
        
        book_stem = match.group(1)
        print(f"\nProcessing book: '{book_stem}'")
        book_count += 1

        for stage_num in range(1, args.fill_until_stage):
            dest_dir = pipeline_base_dir / f"stage{stage_num}"
            dest_dir.mkdir(parents=True, exist_ok=True)
            
            dest_file_path = dest_dir / f"{book_stem}.stage{stage_num}.json"
            
            print(f"  -> Creating copy for Stage {stage_num}...")
            shutil.copy2(source_file_path, dest_file_path)

    print(f"\n--- Backfill Complete ---")
    print(f"Processed {book_count} book(s).")
    print("You can now run the main pipeline orchestrator (a2l.ps1).")


if __name__ == "__main__":
    main()