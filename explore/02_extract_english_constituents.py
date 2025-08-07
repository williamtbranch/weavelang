# explore/02_extract_english_constituents.py

"""
Parses an English text file, filters for complete sentences, and extracts
their constituent phrases into a structured JSON Lines file.

This script has been corrected to use the proper Stanza Tree API.
"""
import os
import re
import toml
import stanza
import json
from pathlib import Path

# We can define the phrase types we're interested in extracting
TARGET_PHRASES = {'NP', 'VP', 'PP', 'SBAR'}

# ================== NEW HELPER FUNCTION (CORRECTED LOGIC) ==================
def get_text_from_tree(tree):
    """
    Correctly extracts the full text from a Stanza constituency subtree
    by recursively joining the text of all its leaf nodes.
    """
    # Base case: If the node is a leaf, its text is its label.
    if tree.is_leaf():
        return tree.label
    # Recursive step: If it's not a leaf, get text from all children and join.
    else:
        return " ".join(get_text_from_tree(child) for child in tree.children)
# ==========================================================================

def extract_phrases(tree):
    """
    Recursively traverses a Stanza constituency tree to extract all
    phrases of interest using the corrected text extraction method.
    """
    phrases = []
    # Check if the current tree node's label is one we want to extract
    if tree.label in TARGET_PHRASES:
        phrases.append({
            "label": tree.label,
            "text": get_text_from_tree(tree) # Use the new helper function
        })
    
    # Recursively call this function on all children of the current node
    if tree.children:
        for child in tree.children:
            phrases.extend(extract_phrases(child))
        
    return phrases

def main():
    """Main function to orchestrate the parsing and extraction process."""
    print("Starting constituent extraction script...")

    # --- 1. Load Configuration and Set Up Paths ---
    try:
        script_dir = Path(__file__).parent
        config_path = script_dir.parent / "config.toml"
        config = toml.load(config_path)
        content_dir = Path(config['content_project_dir'])
    except Exception as e:
        print(f"Error loading configuration: {e}")
        return

    input_file = content_dir / "Staged" / "test.txt"
    output_dir = content_dir / "explore"
    output_file = output_dir / "constituents.jsonl"

    output_dir.mkdir(parents=True, exist_ok=True)
    print(f"Input file: {input_file}")
    print(f"Output file: {output_file}")

    # --- 2. Extract Sentences ---
    print("Extracting sentences...")
    sentence_pattern = re.compile(r"\{S\d+:\s*(.*?)\}")
    try:
        with open(input_file, 'r', encoding='utf-8') as f:
            text = f.read()
    except FileNotFoundError:
        print(f"Error: Input file not found at {input_file}")
        return
        
    extracted_sentences = sentence_pattern.findall(text)
    print(f"Found {len(extracted_sentences)} potential sentences.")

    # --- 3. Initialize Stanza Pipeline ---
    print("Initializing Stanza pipeline...")
    nlp = stanza.Pipeline('en', processors='tokenize,pos,constituency', use_gpu=True)
    print("Stanza pipeline loaded.")

    # --- 4. Process, Filter, and Extract ---
    print("Parsing text and extracting constituents from valid sentences...")
    doc = nlp("\n\n".join(extracted_sentences))

    all_sentence_data = []
    for sentence in doc.sentences:
        tree = sentence.constituency

        # Fragments are parsed as a ROOT tree with no children.
        if tree and tree.children:
            phrases = extract_phrases(tree)
            
            if phrases:
                all_sentence_data.append({
                    "original_sentence": sentence.text,
                    "phrases": phrases
                })
    
    print(f"Successfully processed {len(all_sentence_data)} complete sentences.")

    # --- 5. Write to JSON Lines file ---
    if not all_sentence_data:
        print("No valid sentences were processed to write to the output file.")
    else:
        print(f"Writing structured data to {output_file}...")
        with open(output_file, 'w', encoding='utf-8') as f_out:
            for entry in all_sentence_data:
                f_out.write(json.dumps(entry) + '\n')

    print("Processing complete.")


if __name__ == "__main__":
    main()