# explore/05_visualize_simplified_tree.py

"""
Parses an English text file and represents the full constituency tree of
each sentence using a simplified, parenthesis-only format, similar to a
Lisp S-expression.

This script produces a pure structural representation by:
1.  Re-parsing the source text to get the original Stanza Tree objects.
2.  Defining a recursive function that traverses a tree:
    - If a node is a leaf (a word), it's wrapped in parentheses: `(word)`.
    - If a node is internal (a phrase), its children's representations
      are joined by spaces and wrapped in a single set of parentheses.
3.  Writing the resulting simplified string to a text file.
"""

import os
import re
import toml
import stanza
from pathlib import Path

def format_tree_simplified(tree):
    """
    Recursively converts a Stanza Tree object into the simplified,
    parenthesis-only string format.
    """
    # Base case: If the node is a leaf (a word), wrap it in parentheses.
    # The word itself is stored in the 'label' of a leaf node.
    if tree.is_leaf():
        return f"({tree.label})"
    
    # Recursive step: If it's an internal node (a phrase)...
    else:
        # 1. Recursively format all of its children.
        child_strings = [format_tree_simplified(child) for child in tree.children]
        # 2. Join the formatted children with spaces.
        joined_children = " ".join(child_strings)
        # 3. Wrap the entire result in one set of parentheses for this phrase.
        return f"({joined_children})"

def main():
    """Main function to generate the simplified tree visualization."""
    print("Starting simplified tree structure script...")

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
    output_file = output_dir / "simplified_tree_structure.txt"

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
    print("Parsing text and converting trees to simplified format...")
    doc = nlp("\n\n".join(extracted_sentences))

    with open(output_file, 'w', encoding='utf-8') as f_out:
        f_out.write("--- Simplified Hierarchical Sentence Structures ---\n\n")

        for i, sentence in enumerate(doc.sentences):
            tree = sentence.constituency
            f_out.write(f"--- Sentence: \"{sentence.text}\" ---\n")

            if tree and tree.children:
                # The top-level tree is ROOT. We want to format its main child, which is the S (Sentence) node.
                sentence_node = tree.children[0]
                simplified_string = format_tree_simplified(sentence_node)
                
                f_out.write(simplified_string)
                f_out.write("\n\n")
            else:
                # For fragments, we can just show the words.
                f_out.write(f"({sentence.text})\n\n")

    print("Processing complete.")


if __name__ == "__main__":
    main()