# explore/06_intelligent_segmentation.py

"""
Performs an intelligent, hierarchical segmentation of sentences by applying a
"local merge" strategy at every node of the parse tree. This version correctly
respects grammatical boundaries.

The algorithm is as follows:
1. A recursive function processes each node.
2. It first gathers the already-segmented lists from all its children.
3. It then performs a "dew drop" merge on that collected list of segments.
   Small, consecutive segments are joined together until the MIN_SEGMENT_SIZE
   is met or exceeded.
4. The newly merged list of segments is then passed up to the parent node.
   This ensures that grammatical units like "to keep watch" are evaluated
   and merged locally before being considered by higher-level phrases.
"""

import os
import re
import toml
import stanza
from pathlib import Path

# --- Configuration Constant ---
MIN_SEGMENT_SIZE = 3

# --- Core Segmentation Logic (Definitive Version) ---

def segment_tree(tree):
    """
    Recursively segments a tree using a local merge strategy at each node.
    Returns a list of final segment strings.
    """
    # Base case: A leaf is an atomic unit of size 1.
    if tree.is_leaf():
        return [tree.label]

    # --- Recursive Step: Gather segments from all children first ---
    segments_from_children = []
    if tree.children:
        for child in tree.children:
            segments_from_children.extend(segment_tree(child))

    # --- Local "Dew Drop" Merge ---
    # Now, perform the merge logic on the segments collected from the children.
    if not segments_from_children:
        return []

    final_segments = []
    buffer = []
    
    for segment in segments_from_children:
        buffer.append(segment)
        # Check if the combined buffer now meets the size requirement.
        # We join the buffer to count words accurately.
        current_buffer_word_count = len(" ".join(buffer).split())
        
        if current_buffer_word_count >= MIN_SEGMENT_SIZE:
            # The buffer is now large enough. Solidify it into a segment.
            final_segments.append(" ".join(buffer))
            buffer = [] # Clear the buffer for the next segments.

    # After the loop, if there's anything left in the buffer, it means it
    # never reached the minimum size. It becomes its own final segment.
    if buffer:
        final_segments.append(" ".join(buffer))
        
    return final_segments

def main():
    """Main function to generate the final segmented output."""
    print("Starting intelligent sentence segmentation script (final version)...")

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
    output_file = output_dir / "intelligently_segmented.txt"

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
        f_out.write(f"--- Intelligently Segmented Sentences (Min Size: {MIN_SEGMENT_SIZE}) ---\n\n")

        for sentence in doc.sentences:
            f_out.write(f"--- Sentence: \"{sentence.text}\" ---\n")
            tree = sentence.constituency

            if tree and tree.children:
                sentence_node = tree.children[0]
                # The top-level call to the recursive function
                final_segments = segment_tree(sentence_node)
                
                formatted_output = " ".join(f"({segment})" for segment in final_segments)
                f_out.write(formatted_output)
                f_out.write("\n\n")
            else:
                f_out.write(f"({sentence.text})\n\n")

    print("Processing complete.")

if __name__ == "__main__":
    main()