# Save as: generate_segment_prompts.py
import json
from pathlib import Path
import argparse
import tomllib
from llm2books.llm_logger import LLMLogger

# --- SIMPLIFIED IMPORTS ---
# We only need the processor classes and the word counter now.
from llm2books.stanza_segmenter import (
    EnglishStanzaProcessor, 
    SpanishStanzaProcessor, 
    count_real_words
)

# --- THE CORE ANALYSIS FUNCTION (NOW SIMPLIFIED) ---
def generate_final_segment_analysis(std_json_path: Path, output_path: Path, segmenter, limit: int):
    """
    Reads a .std.json file, runs the LLM segmentation, and writes the
    resulting segments to a text file for validation.
    """
    if not std_json_path.exists():
        print(f"Error: Input file not found at '{std_json_path}'")
        return

    print(f"Reading from: {std_json_path}")
    
    with open(std_json_path, 'r', encoding='utf-8') as f:
        data = json.load(f)

    with open(output_path, 'w', encoding='utf-8') as f_out:
        f_out.write(f"# LLM-Based Segmentation Analysis\n")
        f_out.write(f"# Source: {std_json_path.name}\n")
        f_out.write(f"# Segmenter: {type(segmenter).__name__}\n")
        if limit > 0: f_out.write(f"# (Limited to first {limit} sentences)\n")
        f_out.write("=" * 80 + "\n\n")

        content_blocks = data.get("content", [])
        sentence_count = 0
        
        for block in content_blocks:
            if block.get("block_type") == "sentence":
                if limit > 0 and sentence_count >= limit: break
                
                s_id = block.get("s_id")
                full_text = block.get("full_text", "").strip()
                
                if not s_id or not full_text: continue
                sentence_count += 1
                
                f_out.write(f"--- S_ID: {s_id} (Sentence #{sentence_count}) ---\n")
                f_out.write(f"Original: {full_text}\n\n")

                # --- CALL THE LLM SEGMENTER ---
                final_segments = segmenter.segment_sentence(full_text)
                
                f_out.write("Final Segments:\n")
                if final_segments:
                    for j, phrase in enumerate(final_segments):
                        word_count = count_real_words(phrase)
                        f_out.write(f"  - Seg {j+1} ({word_count}w): {phrase}\n")
                else:
                    f_out.write("  (No segments generated)\n")

                f_out.write("-" * 40 + "\n\n")

    print(f"Successfully generated analysis for {sentence_count} sentence(s) at: '{output_path}'")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Generates a segmentation analysis from a .std.json file using the LLM segmenter."
    )
    parser.add_argument(
        "std_json_file",
        type=Path,
        help="Path to the input .std.json file (e.g., 'common_pool/derived_texts/Book.lang.std.json')."
    )
    parser.add_argument(
        "-l", "--limit",
        type=int,
        default=50,
        help="Limit the output to the first N sentences. Set to 0 for no limit."
    )
    args = parser.parse_args()
    try:
        with open("config.toml", "rb") as f:
            config = tomllib.load(f)
    except Exception as e:
        print(f"Could not load config.toml: {e}")
        exit(1)
    # Log to a dedicated analysis folder to keep logs separate from pipeline runs
    analysis_log_dir = Path("analysis_logs")
    llm_logger = LLMLogger(analysis_log_dir)
    
    # This logic remains the same and correctly selects the processor.
    if ".es.std.json" in args.std_json_file.name:
        print("Initializing SpanishStanzaProcessor (LLM-based) for analysis...")
        segmenter_to_use = SpanishStanzaProcessor(config, llm_logger)
    else:
        print("Initializing EnglishStanzaProcessor (LLM-based) for analysis...")
        segmenter_to_use = EnglishStanzaProcessor(config, llm_logger)
    
    output_file_path = args.std_json_file.parent / f"{args.std_json_file.stem}_final_segment_analysis.txt"
    
    generate_final_segment_analysis(args.std_json_file, output_file_path, segmenter_to_use, args.limit)