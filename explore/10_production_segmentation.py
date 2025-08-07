# explore/10_final_production_segmentation.py

"""
The definitive, production-ready script for intelligent segmentation.
This version uses a clean, two-function design for maximum reliability:
1.  A recursive function gets a flat list of all words, tagged by whether
    they are "real words" or punctuation based on their POS tag.
2.  A separate, non-recursive function takes this list and applies the
    merging logic to produce the final segments.
"""

import os
import re
import toml
import stanza
from pathlib import Path

# --- Configuration Constant ---
MIN_SEGMENT_SIZE = 3

# --- Core Logic: Two-Function Design ---

def get_word_tagged_leaves(tree):
    """
    Recursively traverses a tree to produce a flat list of tuples.
    Each tuple is (word_text, is_real_word_bool).
    """
    # Base case: A leaf node has no children.
    # We can't know if it's a word here, as the POS tag is its parent.
    # The logic is handled in the recursive step.
    if tree.is_leaf():
        return [(tree.label, True)] # Placeholder, will be corrected by parent

    tagged_leaves = []
    for child in tree.children:
        if child.is_leaf():
            # If the child is a leaf, its parent is the current 'tree'.
            # The parent's label is the POS tag.
            is_word = tree.label.isalpha()
            tagged_leaves.append((child.label, is_word))
        else:
            # If the child is a phrase, recurse.
            tagged_leaves.extend(get_word_tagged_leaves(child))
    return tagged_leaves

def merge_segments(tagged_words):
    """

    Takes a list of (word, is_real_word) tuples and merges them into
    segments based on MIN_SEGMENT_SIZE. This function is not recursive.
    """
    # First, handle possessive stitching on the raw tuple list.
    stitched_tuples = []
    for text, is_word in tagged_words:
        if text in ("'s", "’s") and stitched_tuples:
            # Append the possessive text to the last tuple's text
            last_text, last_is_word = stitched_tuples[-1]
            stitched_tuples[-1] = (last_text + text, last_is_word)
        else:
            stitched_tuples.append((text, is_word))

    # Now, perform the merge logic on the stitched list.
    final_segments = []
    buffer = [] # Buffer will store the text parts
    word_count_in_buffer = 0

    for text, is_word in stitched_tuples:
        buffer.append(text)
        if is_word:
            word_count_in_buffer += 1

        if word_count_in_buffer >= MIN_SEGMENT_SIZE:
            final_segments.append(" ".join(buffer))
            buffer = []
            word_count_in_buffer = 0
            
    # After the loop, merge any leftovers backwards.
    if buffer:
        leftover_text = " ".join(buffer)
        if final_segments:
            final_segments[-1] += " " + leftover_text
        else:
            final_segments.append(leftover_text)
            
    return final_segments

def main():
    """Main function to generate the final segmented output."""
    print("Starting definitive production sentence segmentation script...")

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
    output_file = output_dir / "production_segmented.txt"

    output_dir.mkdir(parents=True, exist_ok=True)
    print(f"Input file: {input_file}")
    print(f"Output file: {output_file}")
    print(f"Using minimum segment size: {MIN_SEGMENT_SIZE}")

    print("Initializing Stanza pipeline...")
    nlp = stanza.Pipeline('en', processors='tokenize,pos,constituency', use_gpu=True)
    print("Stanza pipeline loaded.")
    
    print("Parsing text and performing intelligent segmentation...")
    sentence_pattern = re.compile(r"\{S\d+:\s*(.*?)\}")
    with open(input_file, 'r', encoding='utf-8') as f:
        text = f.read()
    extracted_sentences = sentence_pattern.findall(text)
    doc = nlp("\n\n".join(extracted_sentences))

    with open(output_file, 'w', encoding='utf-8') as f_out:
        f_out.write(f"--- Production Segmented Sentences (Min Size: {MIN_SEGMENT_SIZE}) ---\n\n")

        for sentence in doc.sentences:
            f_out.write(f"--- Sentence: \"{sentence.text}\" ---\n")
            tree = sentence.constituency

            if tree and tree.children:
                sentence_node = tree.children[0]
                
                # Step 1: Get the flat list of tagged words.
                tagged_words = get_word_tagged_leaves(sentence_node)
                
                # Step 2: Apply the merging logic to the flat list.
                final_segments = merge_segments(tagged_words)
                
                formatted_output = " ".join(f"({segment})" for segment in final_segments)
                f_out.write(formatted_output)
                f_out.write("\n\n")
            else:
                f_out.write(f"({sentence.text})\n\n")

    print("Processing complete. The definitive output is in production_segmented.txt")


if __name__ == "__main__":
    main()