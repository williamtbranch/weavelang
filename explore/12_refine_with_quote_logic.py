# explore/12_refine_with_quote_logic.py

"""
Applies a final layer of pragmatic refinement to the segmented output,
specifically handling quoted speech to create more natural breaks in dialogue.

This script acts as a post-processor:
1.  It reads the high-quality, grammatically-aware segments from the
    previous script's output file.
2.  It iterates through the segments and applies a special rule for quotes:
    - If a segment contains an opening quote, it is split.
    - If the part before the quote is now too small, it is merged backwards
      into the previous segment.
3.  This produces the final, production-quality output ready for the
    WeaveLang application.
"""

import re
from pathlib import Path
import toml

# --- Configuration Constant ---
MIN_SEGMENT_SIZE = 3
QUOTE_CHARS = "‘“" # Includes single and double opening quotes

def count_real_words(text):
    """Counts only actual words in a string, ignoring standalone punctuation."""
    return len(re.findall(r'\w+', text))

def refine_with_quote_logic(segments):
    """
    Takes a list of segments and applies quote-splitting logic.
    """
    refined_segments = []
    
    for segment in segments:
        match = re.search(f"([{QUOTE_CHARS}])", segment)
        
        if match and match.start() > 0:
            quote_char = match.group(1)
            pre_quote_part, quote_part = segment.split(quote_char, 1)
            pre_quote_part = pre_quote_part.strip()
            quote_part = quote_char + quote_part
            
            if count_real_words(pre_quote_part) < MIN_SEGMENT_SIZE and refined_segments:
                refined_segments[-1] += " " + pre_quote_part
                refined_segments.append(quote_part)
            else:
                if pre_quote_part:
                    refined_segments.append(pre_quote_part)
                refined_segments.append(quote_part)
        else:
            refined_segments.append(segment)
            
    return refined_segments

def main():
    """Main function to apply the final refinement."""
    print("Applying final refinement for quoted speech...")

    try:
        script_dir = Path(__file__).parent
        config_path = script_dir.parent / "config.toml"
        config = toml.load(config_path)
        content_dir = Path(config['content_project_dir'])
    except Exception as e:
        print(f"Error loading configuration: {e}")
        return

    # ================== FIX IS HERE ==================
    # Correctly define all paths using the base content_dir Path object.
    explore_dir = content_dir / "explore"
    # The input file is the output from the previous script.
    input_file = explore_dir / "production_segmented.txt"
    # The output file will also go in the explore directory.
    output_file = explore_dir / "final_refined_weavable_text.txt"
    # ===============================================

    if not input_file.exists():
        print(f"Error: Input file not found at {input_file}. Please run the production segmenter first.")
        return

    print(f"Reading from: {input_file}")
    print(f"Writing to:   {output_file}")

    with open(input_file, 'r', encoding='utf-8') as f_in, \
         open(output_file, 'w', encoding='utf-8') as f_out:
        
        f_out.write("--- Final Refined Weavable Text ---\n\n")

        current_sentence_segments = []
        for line in f_in:
            line = line.strip()
            if line.startswith("--- Sentence:"):
                if current_sentence_segments:
                    refined = refine_with_quote_logic(current_sentence_segments)
                    formatted_output = " ".join(f"({segment})" for segment in refined)
                    f_out.write(formatted_output + "\n\n")
                
                f_out.write(line + "\n")
                current_sentence_segments = []
            elif line.startswith("("):
                current_sentence_segments = re.findall(r"\((.*?)\)", line)
        
        if current_sentence_segments:
            refined = refine_with_quote_logic(current_sentence_segments)
            formatted_output = " ".join(f"({segment})" for segment in refined)
            f_out.write(formatted_output + "\n\n")

    print("Refinement complete. The final output is ready for WeaveLang.")

if __name__ == "__main__":
    main()