# explore/11_final_definitive_segmentation.py

"""
The final, definitive, and correct script for intelligent segmentation.
This version combines the robust recursive structure with intelligent,
POS-tag-based word counting to produce the most natural segments.

The algorithm is a single, powerful recursive function that ensures:
1.  Grammatical boundaries are always respected.
2.  The "local merge" happens at every node in the tree, preventing unnatural splits.
3.  Only "real" words (not punctuation) contribute to the MIN_SEGMENT_SIZE count.
4.  Possessives and other edge cases are handled correctly.
"""

import os
import re
import toml
import stanza
from pathlib import Path

# --- Configuration Constant ---
MIN_SEGMENT_SIZE = 4

# --- Core Segmentation Logic (Definitive Version) ---

def segment_tree(tree):
    """
    The definitive recursive function. It returns a list of `(text, word_count)`
    tuples, representing the final segments for the given subtree.
    """
    # Base case: A leaf has no children. Its parent is the POS tag.
    # We cannot decide its word count here, the parent must do it.
    if tree.is_leaf():
        # This function should only be called on non-leaf nodes by its logic,
        # but as a fallback, we return the leaf's text with a provisional word count.
        return [(tree.label, 1)]

    # --- Recursive Step & Local Merge ---
    segments_from_children = []
    
    # First, gather the recursively-segmented lists from all children.
    for child in tree.children:
        if child.is_leaf():
            # If the child is a leaf, its parent is the current `tree`.
            # We determine if it's a word based on the parent's (POS) tag.
            is_word = tree.label.isalpha()
            segments_from_children.append( (child.label, 1 if is_word else 0) )
        else:
            # If the child is a phrase, recurse to get its segmented list of tuples.
            segments_from_children.extend(segment_tree(child))

    # --- Pre-merge Possessives on the tuple list ---
    stitched_segments = []
    for text, count in segments_from_children:
        if text in ("'s", "’s") and stitched_segments:
            prev_text, prev_count = stitched_segments.pop()
            stitched_segments.append((prev_text + text, prev_count))
        else:
            stitched_segments.append((text, count))

    # --- Local "Dew Drop" Merge on the (text, word_count) tuples ---
    final_segments = [] # This will be a list of (text, count) tuples
    buffer = [] # Buffer will also store tuples
    
    for text, count in stitched_segments:
        buffer.append((text, count))
        current_word_count = sum(c for t, c in buffer)
        
        if current_word_count >= MIN_SEGMENT_SIZE:
            # The buffer is large enough. Solidify it into a single tuple.
            merged_text = " ".join(t for t, c in buffer)
            total_count = sum(c for t, c in buffer)
            final_segments.append((merged_text, total_count))
            buffer = []

    # After the loop, merge any leftovers backwards.
    if buffer:
        leftover_text = " ".join(t for t, c in buffer)
        leftover_count = sum(c for t, c in buffer)
        if final_segments:
            # Append to the last segment
            prev_text, prev_count = final_segments.pop()
            final_segments.append((prev_text + " " + leftover_text, prev_count + leftover_count))
        else:
            # This is the only segment
            final_segments.append((leftover_text, leftover_count))
            
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

    input_file = content_dir / "common_pool"/ "source_texts" / "test.en.txt"

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
                # The top-level call returns a list of (text, count) tuples
                final_segments_tuples = segment_tree(sentence_node)
                
                # We only need the text part for the final output
                formatted_output = " ".join(f"({text})" for text, count in final_segments_tuples)
                f_out.write(formatted_output)
                f_out.write("\n\n")
            else:
                f_out.write(f"({sentence.text})\n\n")

    print("Processing complete. The definitive output is in production_segmented.txt")


if __name__ == "__main__":
    main()