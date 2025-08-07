# explore/01_parse_english_file.py

"""
Parses an English text file to extract its constituency trees using Stanza.

This script performs the following actions:
1.  Reads the main `config.toml` file to find the content project directory.
2.  Constructs the path to the input file (`Staged/test.txt`).
3.  Constructs the path for the output file (`explore/parsed_sentences.txt`).
4.  Initializes the Stanza English pipeline with a constituency parser.
5.  Reads the input file and uses a regular expression to extract only the
    sentences marked with the {S<number>: ...} format.
6.  Processes the extracted sentences with Stanza.
7.  Writes the original sentence and its pretty-printed constituency tree
    to the output file for analysis.
"""

import os
import re
import toml
import stanza
import io
import contextlib
from pathlib import Path

def main():
    """Main function to orchestrate the parsing process."""
    print("Starting sentence parsing script...")

    # --- 1. Load Configuration and Set Up Paths ---
    try:
        # The script is in 'explore', so the config is one level up
        script_dir = Path(__file__).parent
        config_path = script_dir.parent / "config.toml"
        config = toml.load(config_path)
        content_dir = Path(config['content_project_dir'])
    except (FileNotFoundError, KeyError) as e:
        print(f"Error: Could not load configuration. Ensure config.toml exists and has 'content_project_dir'. Details: {e}")
        return

    input_file = content_dir / "Staged" / "test.txt"
    output_dir = content_dir / "explore"
    output_file = output_dir / "parsed_sentences.txt"

    # Ensure the output directory exists
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"Input file: {input_file}")
    print(f"Output file: {output_file}")

    if not input_file.exists():
        print(f"Error: Input file not found at {input_file}")
        return

    # --- 2. Extract Sentences from Input File ---
    print("Extracting sentences from input file...")
    sentence_pattern = re.compile(r"\{S\d+:\s*(.*?)\}")
    extracted_sentences = []
    try:
        with open(input_file, 'r', encoding='utf-8') as f:
            for line in f:
                match = sentence_pattern.search(line)
                if match:
                    extracted_sentences.append(match.group(1))
    except Exception as e:
        print(f"Error reading input file: {e}")
        return

    if not extracted_sentences:
        print("Warning: No sentences found in the {Sn: ...} format. Exiting.")
        return
    
    print(f"Found {len(extracted_sentences)} sentences to parse.")

    # --- 3. Initialize Stanza Pipeline ---
    print("Initializing Stanza English pipeline (this may download models on first run)...")
    try:
        # ================== FIX IS HERE ==================
        # We need the 'pos' processor as a prerequisite for 'constituency'
        nlp = stanza.Pipeline('en', processors='tokenize,pos,constituency')
        # ===============================================
    except Exception as e:
        print(f"Error initializing Stanza. Ensure models are downloaded and accessible. Details: {e}")
        return
    print("Stanza pipeline loaded.")

    # --- 4. Process Sentences and Write Output ---
    print("Parsing sentences and writing to output file...")
    # Process all sentences in a batch for efficiency
    doc = nlp("\n\n".join(extracted_sentences))

    try:
        with open(output_file, 'w', encoding='utf-8') as f_out:
            for i, sentence in enumerate(doc.sentences):
                f_out.write(f"--- PARSE FOR SENTENCE {i+1} ---\n")
                f_out.write(f"Original: {sentence.text}\n\n")
                
                with io.StringIO() as buf, contextlib.redirect_stdout(buf):
                    sentence.constituency.pretty_print()
                    parse_string = buf.getvalue()
                
                f_out.write(parse_string)
                f_out.write("\n\n")
    except Exception as e:
        print(f"Error writing to output file: {e}")
        return

    print(f"Processing complete. Parsed trees written to {output_file}")

if __name__ == "__main__":
    main()