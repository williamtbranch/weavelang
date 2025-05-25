import re
import nltk  # For sentence tokenization
from pathlib import Path
import sys   # For sys.exit

# --- Configuration for TOML parsing ---
# Try to import tomllib (Python 3.11+), fallback to toml (installable package)
try:
    import tomllib
    TOML_LOAD_MODE = "rb" # tomllib expects binary mode
    def load_toml_file(f):
        return tomllib.load(f)
except ImportError:
    try:
        import toml # Requires: pip install toml
        TOML_LOAD_MODE = "r" # toml expects text mode
        def load_toml_file(f):
            return toml.load(f)
        print("Using 'toml' library for config. Python 3.11+ with 'tomllib' is preferred.")
    except ImportError:
        print("TOML library not found. Please install 'toml' (pip install toml) or use Python 3.11+.")
        sys.exit(1)
# --- End Configuration for TOML parsing ---

# Download 'punkt' tokenizer for NLTK if not already present
try:
    nltk.data.find('tokenizers/punkt')
    # print("NLTK 'punkt' tokenizer found.") # Optional: for successful find
except LookupError: # Catch the primary error from nltk.data.find
    print("NLTK 'punkt' tokenizer not found. Attempting to download...")
    try:
        nltk.download('punkt', quiet=True) # Set quiet=False for more download details if needed
        print("'punkt' downloaded successfully.")
        # Verify after download
        try:
            nltk.data.find('tokenizers/punkt')
            print("Verified 'punkt' tokenizer is now available.")
        except LookupError:
            print("ERROR: 'punkt' download seemed to complete, but it's still not found. Check NLTK setup and paths.")
            sys.exit(1)
    except Exception as e: # Catch any exception during the download process
        print(f"Error downloading 'punkt': {e}")
        print("Please ensure you can connect to the internet or install 'punkt' manually by running:")
        print(">>> import nltk")
        print(">>> nltk.download('punkt')")
        print("In a Python interpreter.")
        sys.exit(1)
except Exception as e: # Catch other unexpected NLTK issues
    print(f"An unexpected error occurred related to NLTK data: {e}")
    print("Please ensure NLTK is installed correctly and its data paths are accessible.")
    sys.exit(1)

def load_config(config_path_str="config.toml"):
    """Loads configuration from a TOML file."""
    config_path = Path(config_path_str)
    if not config_path.is_file():
        print(f"Error: Configuration file '{config_path}' not found.")
        return None
    try:
        with open(config_path, TOML_LOAD_MODE) as f:
            config = load_toml_file(f)
        return config
    except Exception as e: # Catches tomllib.TOMLDecodeError or toml.TomlDecodeError
        print(f"Error parsing TOML file '{config_path}': {e}")
        return None

def process_text_to_sentences(text_content):
    """
    Processes raw text content to extract and format sentences.
    - Joins artificially broken lines within paragraphs.
    - Splits text into sentences based on paragraph breaks of 2+ newlines.
    - Removes specific bracketed metadata at block and inline levels.
    - Formats sentences as {Sn: content}.
    """
    # 1. Split into "logical" paragraphs.
    #    This regex splits on two or more newlines, considering optional whitespace between them.
    #    (?:...) is a non-capturing group. \n\s* means newline then optional whitespace.
    #    {2,} means that group must occur 2 or more times.
    raw_paragraphs = re.split(r'(?:\n\s*){2,}', text_content.strip())

    all_formatted_sentences = []
    sentence_counter = 1

    for raw_para in raw_paragraphs:
        # Skip if raw_para itself is empty or just whitespace after split
        if not raw_para.strip():
            continue

        # 2. Reconstruct paragraph:
        #    - Replace single newlines (and surrounding whitespace) within a paragraph with a single space.
        reconstructed_para = re.sub(r'\s*\n\s*', ' ', raw_para).strip()
        #    - Normalize multiple consecutive spaces to single spaces.
        reconstructed_para = re.sub(r'\s+', ' ', reconstructed_para)

        if not reconstructed_para: # Skip if paragraph became empty after all cleaning
            continue

        # 3. Block-level metadata removal:
        #    If the entire reconstructed paragraph starts with '[' and ends with ']',
        #    it's considered a block of metadata to be skipped.
        #    The pattern r'^\[.*\]$' matches a string that starts with '[',
        #    has any characters in between ('.*'), and ends with ']'.
        #    The 's' flag (dotall) is not strictly needed for r'^\[.*\]$' here because 
        #    newlines within the paragraph have already been replaced by spaces in 'reconstructed_para'.
        #    The re.match implicitly anchors at the beginning of the string. The '$' ensures it ends there.
        if re.match(r'^\[.*\]\s*$', reconstructed_para, re.S):
            # print(f"DEBUG: Skipping block metadata via regex: {reconstructed_para[:100]}...")
            continue
        

        # 4. Sentence Segmentation using NLTK
        try:
            sentences_in_para = nltk.sent_tokenize(reconstructed_para)
        except Exception as e:
            print(f"Warning: NLTK error tokenizing paragraph: '{reconstructed_para[:100]}...'. Error: {e}. Skipping paragraph.")
            continue

        for sent in sentences_in_para:
            # 5. Inline metadata removal from each sentence:
            #    Removes patterns like "[anything inside brackets]".
            #    First, remove the bracketed content itself. `\[[^\]]*\]` matches a `[`
            #    followed by any characters that are NOT `]`, then the closing `]`.
            #    This handles simple, non-nested brackets well.
            sentence_no_brackets = re.sub(r'\[[^\]]*\]', '', sent)
            
            #    Then, normalize whitespace that might have been left or created by the removal.
            cleaned_sentence = re.sub(r'\s+', ' ', sentence_no_brackets).strip()

            # If sentence becomes empty after removing brackets and stripping, skip it.
            if not cleaned_sentence:
                # print(f"DEBUG: Skipping empty sentence after bracket removal. Original: '{sent[:70]}...'") # For debugging
                continue
            
            all_formatted_sentences.append(f"{{S{sentence_counter}: {cleaned_sentence}}}")
            sentence_counter += 1
    
    return "\n".join(all_formatted_sentences)

def main():
    config = load_config() # Assumes config.toml is in the same dir as the script or CWD
    if not config:
        sys.exit(1) # Error message already printed by load_config

    content_project_dir_str = config.get("content_project_dir")
    if not content_project_dir_str:
        print("Error: 'content_project_dir' not found in config.toml.")
        sys.exit(1)

    content_project_path = Path(content_project_dir_str)
    raw_dir = content_project_path / "Raw"
    staged_dir = content_project_path / "Staged"

    if not raw_dir.is_dir():
        print(f"Error: Raw directory not found at '{raw_dir}'")
        sys.exit(1)

    # Ensure Staged directory exists
    staged_dir.mkdir(parents=True, exist_ok=True)
    print(f"Processing files from: {raw_dir}")
    print(f"Saving processed files to: {staged_dir}")

    processed_count = 0
    skipped_count = 0
    error_count = 0

    # Process all .txt files in the Raw directory
    for raw_file_path in raw_dir.glob('*.txt'):
        print(f"\n--- Analyzing: {raw_file_path.name} ---")
        
        staged_file_path = staged_dir / raw_file_path.name

        if staged_file_path.exists():
            print(f"Skipping '{raw_file_path.name}': Output file already exists in Staged directory.")
            skipped_count += 1
            continue

        print(f"Processing '{raw_file_path.name}'...")
        try:
            with open(raw_file_path, 'r', encoding='utf-8') as f:
                raw_content = f.read()
        except Exception as e:
            print(f"Error reading file '{raw_file_path.name}': {e}")
            error_count += 1
            continue

        if not raw_content.strip():
            print(f"Skipping '{raw_file_path.name}': File is empty or contains only whitespace.")
            # Optionally, create an empty staged file to mark as "processed"
            # with open(staged_file_path, 'w', encoding='utf-8') as f:
            #     f.write("")
            skipped_count +=1 # Counting as skipped because no real processing needed
            continue
            
        processed_sentences_text = process_text_to_sentences(raw_content)

        if not processed_sentences_text:
            print(f"No processable sentences extracted from '{raw_file_path.name}'. Output file will be empty.")
        
        try:
            with open(staged_file_path, 'w', encoding='utf-8') as f:
                f.write(processed_sentences_text)
            print(f"Successfully processed and saved to '{staged_file_path.name}'")
            processed_count += 1
        except Exception as e:
            print(f"Error writing file '{staged_file_path.name}': {e}")
            error_count +=1

    print(f"\n--- Processing Complete ---")
    print(f"Successfully processed {processed_count} new file(s).")
    print(f"Skipped {skipped_count} file(s) (already existed or empty).")
    if error_count > 0:
        print(f"Encountered errors with {error_count} file(s).")

if __name__ == "__main__":
    main()