# explore/03_visualize_toplevel_phrases.py

"""
Reads the structured constituent data from `constituents.jsonl` and
creates a clean, human-readable summary of only the top-level,
non-redundant phrases for each sentence.

This script makes the parser's output more intuitive by:
1.  Reading the full list of constituents for a sentence.
2.  Filtering out "redundant" phrases. A phrase is considered redundant if its
    text is fully contained within another, larger phrase in the list.
3.  Also filtering out the phrase that represents the entire sentence.
4.  Formatting the result into a clean list for easy analysis.
"""

import json
from pathlib import Path
import toml

def filter_redundant_phrases(phrases, original_sentence):
    """
    Filters a list of constituent phrases to keep only the top-level ones.
    """
    # Create a set of all phrase texts for efficient lookup
    all_texts = {p['text'] for p in phrases}
    
    top_level_phrases = []
    
    for phrase in phrases:
        # Ignore the phrase that is the entire sentence itself
        if phrase['text'] == original_sentence:
            continue
            
        is_redundant = False
        # Check if this phrase's text is a proper substring of any other phrase's text
        for other_text in all_texts:
            if phrase['text'] != other_text and phrase['text'] in other_text:
                is_redundant = True
                break  # Found a larger phrase that contains this one, so it's redundant
        
        if not is_redundant:
            top_level_phrases.append(phrase)
            
    return top_level_phrases

def main():
    """Main function to generate the cleaned-up phrase list."""
    print("Starting top-level phrase visualization script...")

    # --- 1. Load Configuration and Set Up Paths ---
    try:
        script_dir = Path(__file__).parent
        config_path = script_dir.parent / "config.toml"
        config = toml.load(config_path)
        content_dir = Path(config['content_project_dir'])
    except Exception as e:
        print(f"Error loading configuration: {e}")
        return

    input_file = content_dir / "explore" / "constituents.jsonl"
    output_file = content_dir / "explore" / "toplevel_phrases.txt"

    if not input_file.exists():
        print(f"Error: Input file not found at {input_file}. Please run 02_extract... script first.")
        return

    print(f"Reading from: {input_file}")
    print(f"Writing to:   {output_file}")

    # --- 2. Read and Process Data ---
    with open(input_file, 'r', encoding='utf-8') as f_in, \
         open(output_file, 'w', encoding='utf-8') as f_out:
        
        f_out.write("--- Top-Level Constituent Phrases ---\n\n")

        for i, line in enumerate(f_in):
            data = json.loads(line)
            original_sentence = data['original_sentence']
            phrases = data['phrases']

            # Filter out the nested, redundant phrases
            top_level_phrases = filter_redundant_phrases(phrases, original_sentence)

            f_out.write(f"--- Sentence: \"{original_sentence}\" ---\n")
            
            if top_level_phrases:
                for phrase in top_level_phrases:
                    f_out.write(f"  - [{phrase['label']}] \"{phrase['text']}\"\n")
            else:
                f_out.write("  - (This is a fragment or has no complex phrases)\n")
            
            f_out.write("\n")

    print(f"Processing complete. See results in {output_file}")


if __name__ == "__main__":
    main()