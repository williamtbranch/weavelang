# explore/08_production_segmentation.py

"""
The production-ready script for intelligent, hierarchical segmentation. It ensures
that all content from a grammatical node remains self-contained by merging
any leftover small segments backwards into the last available segment from
that same node.

This prevents "orphan" segments from being created and incorrectly merged
with sibling nodes higher up the tree.
"""

import os
import re
import toml
import stanza
from pathlib import Path

# --- Configuration Constant ---
MIN_SEGMENT_SIZE = 3

# --- Core Segmentation Logic (Definitive Production Version) ---

def segment_tree(tree):
    """
    Recursively segments a tree, ensuring node content is self-contained.
    Returns a list of final segment strings.
    """
    if tree.is_leaf():
        return [tree.label]

    # 1. Gather segments from all children recursively.
    segments_from_children = []
    if tree.children:
        for child in tree.children:
            segments_from_children.extend(segment_tree(child))
    
    # Pre-merge possessives to handle that specific edge case first.
    stitched_segments = []
    for segment in segments_from_children:
        if segment in ("'s", "’s") and stitched_segments:
            stitched_segments[-1] += segment
        else:
            stitched_segments.append(segment)

    # 2. Perform the local merge on the (potentially stitched) segments.
    if not stitched_segments:
        return []

    final_segments = []
    buffer = []
    
    for segment in stitched_segments:
        buffer.append(segment)
        current_buffer_word_count = len(" ".join(buffer).split())
        
        if current_buffer_word_count >= MIN_SEGMENT_SIZE:
            final_segments.append(" ".join(buffer))
            buffer = []

    # --- THIS IS THE CRITICAL CHANGE ---
    # After the loop, handle any leftovers in the buffer.
    if buffer:
        leftover_text = " ".join(buffer)
        if final_segments:
            # If segments already exist, merge backwards into the last one.
            final_segments[-1] += " " + leftover_text
        else:
            # If no segments were created, this is the only one.
            final_segments.append(leftover_text)
    # --- END OF CHANGE ---
        
    return final_segments

def main():
    """Main function to generate the final segmented output."""
    print("Starting production sentence segmentation script...")

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
                final_segments = segment_tree(sentence_node)
                
                formatted_output = " ".join(f"({segment})" for segment in final_segments)
                f_out.write(formatted_output)
                f_out.write("\n\n")
            else:
                f_out.write(f"({sentence.text})\n\n")

    print("Processing complete. The production-ready output is in production_segmented.txt")


if __name__ == "__main__":
    main()