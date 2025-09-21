# Save as: generate_segment_prompts.py
import json
from pathlib import Path
import argparse
# Import the processor and the now-standalone helper function
from llm2books.stanza_segmenter import EnglishStanzaProcessor, count_real_words

def generate_final_segment_analysis(std_json_path: Path, output_path: Path, segmenter, limit: int):
    """
    Reads an ENGLISH .std.json file, runs the final production segmentation logic,
    and writes the resulting segments to a text file for validation.
    """
    if not std_json_path.exists():
        print(f"Error: Input file not found at '{std_json_path}'")
        return

    print(f"Reading from: {std_json_path}")
    
    with open(std_json_path, 'r', encoding='utf-8') as f:
        data = json.load(f)

    with open(output_path, 'w', encoding='utf-8') as f_out:
        f_out.write(f"# Final Production Segmentation Analysis\n")
        f_out.write(f"# Source: {std_json_path.name}\n")
        if limit > 0:
            f_out.write(f"# (Limited to first {limit} sentences)\n")
        f_out.write("=" * 80 + "\n\n")

        content_blocks = data.get("content", [])
        sentence_count = 0
        
        for block in content_blocks:
            if block.get("block_type") == "sentence":
                if limit > 0 and sentence_count >= limit:
                    break
                
                s_id = block.get("s_id")
                full_text = block.get("full_text", "").strip()
                
                if not s_id or not full_text:
                    continue

                sentence_count += 1
                
                # --- CALL THE FINAL PRODUCTION FUNCTION ---
                final_segments = segmenter.segment_sentence(full_text)
                
                f_out.write(f"--- S_ID: {s_id} (Sentence #{sentence_count}) ---\n")
                f_out.write(f"Original: {full_text}\n\n")
                
                # --- FORMAT THE FINAL OUTPUT ---
                f_out.write("Final Segments:\n")
                if final_segments:
                    for j, phrase in enumerate(final_segments):
                        word_count = count_real_words(phrase)
                        f_out.write(f"  - Seg {j+1} ({word_count}w): {phrase}\n")
                else:
                    f_out.write("  (No segments generated)\n")

                f_out.write("-" * 40 + "\n\n")

    print(f"Successfully generated final segment analysis for {sentence_count} sentence(s) at: '{output_path}'")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Analyzes sentences from an ENGLISH .std.json file using the production segmentation logic."
    )
    parser.add_argument(
        "std_json_file",
        type=Path,
        help="Path to the input ENGLISH .std.json file (e.g., 'common_pool/derived_texts/Book.en.std.json')."
    )
    parser.add_argument(
        "-l", "--limit",
        type=int,
        default=50,
        help="Limit the output to the first N sentences. Set to 0 for no limit."
    )
    args = parser.parse_args()
    
    print("Initializing EnglishStanzaProcessor for analysis...")
    english_segmenter = EnglishStanzaProcessor()
    
    output_file_path = args.std_json_file.parent / f"{args.std_json_file.stem}_final_segment_analysis.txt"
    
    generate_final_segment_analysis(args.std_json_file, output_file_path, english_segmenter, args.limit)