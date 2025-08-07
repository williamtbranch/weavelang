# explore/04_visualize_hierarchical_structure.py

"""
Reads the original Stanza parse trees and visualizes their full
hierarchical structure in a clean, nested format.

This script provides a deep view of the sentence structure by:
1.  Reloading the Stanza pipeline to re-parse the sentences. This is necessary
    because the .jsonl file only contains the extracted phrases, not the
    original tree objects.
2.  Defining a recursive function to convert a Stanza `Tree` object into a
    nested Python dictionary. Leaf nodes (words) are represented as strings.
3.  Writing the resulting nested structure to a text file using `json.dumps`
    with indentation, which provides a clear, hierarchical view.
"""

import os
import re
import toml
import stanza
import json
from pathlib import Path

def convert_tree_to_dict(tree):
    """
    Recursively converts a Stanza Tree object into a nested dictionary.
    """
    # Base case: If the node is a leaf, return its text (the word).
    if tree.is_leaf():
        return tree.label
    
    # Recursive step: If it's an internal node, create a dictionary.
    # The key is the phrase label (e.g., 'NP'), and the value is a list
    # containing the recursively converted children.
    else:
        children = [convert_tree_to_dict(child) for child in tree.children]
        return {tree.label: children}

def main():
    """Main function to generate the hierarchical visualization."""
    print("Starting hierarchical structure visualization script...")

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
    output_file = output_dir / "hierarchical_structure.txt"

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
    # We must re-parse to get the original tree objects back.
    print("Initializing Stanza pipeline...")
    nlp = stanza.Pipeline('en', processors='tokenize,pos,constituency', use_gpu=True)
    print("Stanza pipeline loaded.")

    # --- 4. Process and Convert Trees ---
    print("Parsing text and converting trees to hierarchical format...")
    doc = nlp("\n\n".join(extracted_sentences))

    with open(output_file, 'w', encoding='utf-8') as f_out:
        f_out.write("--- Hierarchical Sentence Structures ---\n\n")

        for i, sentence in enumerate(doc.sentences):
            tree = sentence.constituency
            f_out.write(f"--- Sentence: \"{sentence.text}\" ---\n")

            if tree and tree.children:
                # Convert the Stanza Tree to our nested dictionary format
                hierarchical_dict = convert_tree_to_dict(tree)
                
                # Use json.dumps with indent for pretty-printing the hierarchy
                pretty_json_string = json.dumps(hierarchical_dict, indent=2)
                f_out.write(pretty_json_string)
                f_out.write("\n\n")
            else:
                f_out.write("(This is a fragment and has no hierarchical structure.)\n\n")

    print("Processing complete.")


if __name__ == "__main__":
    main()