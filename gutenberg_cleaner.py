# Filename: gutenberg_cleaner.py
import os
import re
import argparse
import sys
import toml

# --- Configuration for no-argument mode ---
DEFAULT_CONFIG_FILE_PATH = "config.toml"
DEFAULT_INPUT_SUBDIR = "Raw"
DEFAULT_OUTPUT_SUBDIR = "Prepped"
DEFAULT_FILE_EXTENSION = ".txt"

# Regex to find common Gutenberg SINGLE-LINE illustration/metadata markers
SINGLE_LINE_BRACKET_REGEX = re.compile(
    r"\[(?:Illustration|Copyright|Etext|Project Gutenberg|PG|etext)[^\]\n]*?\]" # Added \n to ensure it's single line
) # Note: The general [^\]\n]+\] was removed as it's often too greedy. Add back if needed for specific cases.

# Regex for chapter-like headings
CHAPTER_HEADING_REGEX = re.compile(
    r"^(?:CHAPTER\s+[IVXLCDM\d]+(?:\]|\.)?|PREFACE\.?|CONTENTS\.?|LIST OF ILLUSTRATIONS\.?|EPILOGUE\.?|PROLOGUE\.?|[A-Z][A-Z\s]{3,}[A-Z]\.?)$",
    re.IGNORECASE # Ensure last char is also a letter for all-caps titles, or it's a known word like PREFACE
)

# Start and End markers for the actual content
START_BOOK_MARKER_PREFIX = "*** START OF THE PROJECT GUTENBERG EBOOK"
END_BOOK_MARKER_PREFIX = "*** END OF THE PROJECT GUTENBERG EBOOK"

def is_actually_blank(line):
    """Checks if a line is truly blank or contains only whitespace."""
    return not line.strip()

def load_app_config(config_file_path=DEFAULT_CONFIG_FILE_PATH):
    if not os.path.exists(config_file_path):
        print(f"Error: Configuration file '{config_file_path}' not found.")
        return None
    try:
        with open(config_file_path, 'r', encoding='utf-8') as f:
            config = toml.load(f)
        return config
    except Exception as e:
        print(f"Error parsing TOML file '{config_file_path}': {e}")
        return None

def remove_multiline_illustration_blocks(lines):
    output_lines = []
    in_illustration_block = False
    block_start_chars = ["[illustration", "[Illustration:"] # Make it a list
    
    for line in lines:
        stripped_line = line.strip()
        is_block_start = any(stripped_line.lower().startswith(start_char) for start_char in block_start_chars)

        if in_illustration_block:
            if stripped_line.endswith("]"):
                in_illustration_block = False
            # Continue to skip lines within the block
        elif is_block_start:
            if not stripped_line.endswith("]"): # If it doesn't end on the same line
                in_illustration_block = True
            # Else (it's a single line [Illustration...]), it's skipped this line
        else:
            output_lines.append(line) # Not in a block and not starting one
    return output_lines

def clean_gutenberg_text(text_content):
    lines = text_content.splitlines()
    in_book_content = False
    
    raw_book_lines = []
    for line in lines:
        if line.strip().startswith(END_BOOK_MARKER_PREFIX):
            in_book_content = False
            break 
        if in_book_content:
            raw_book_lines.append(line)
        if line.strip().startswith(START_BOOK_MARKER_PREFIX):
            in_book_content = True
    
    if not raw_book_lines:
        print("Warning: Could not find book content markers. Attempting to process whole file (may include headers/footers).")
        book_lines_no_multiline_illustrations = remove_multiline_illustration_blocks(lines)
    else:
        book_lines_no_multiline_illustrations = remove_multiline_illustration_blocks(raw_book_lines)
    
    return process_lines_for_paragraphs(book_lines_no_multiline_illustrations)


def process_lines_for_paragraphs(lines_to_process):
    output_paragraphs = []
    current_paragraph_lines = []

    for i, line_raw in enumerate(lines_to_process):
        # 1. Clean line of any remaining single-line bracket markers
        line_cleaned = SINGLE_LINE_BRACKET_REGEX.sub("", line_raw).strip()

        # 2. Skip lines that became empty *only* due to marker removal
        if not line_cleaned and line_raw.strip(): # Was not blank before, but is now
            continue
        
        # 3. Handle chapter headings
        if CHAPTER_HEADING_REGEX.match(line_cleaned):
            if current_paragraph_lines: # Finish previous paragraph
                output_paragraphs.append(" ".join(current_paragraph_lines))
                current_paragraph_lines = []
            output_paragraphs.append("") # Ensure blank line before heading
            output_paragraphs.append(line_cleaned) # Add chapter heading
            output_paragraphs.append("") # Ensure blank line after heading
            continue

        # 4. Paragraph rejoining logic
        if is_actually_blank(line_raw): # Use raw line to check for original blank lines
            # This line was intentionally blank (or only whitespace) in the source
            if current_paragraph_lines:
                output_paragraphs.append(" ".join(current_paragraph_lines))
                output_paragraphs.append("") # Add a blank line for paragraph separation
                current_paragraph_lines = []
            # If it's already a blank line and previous wasn't, ensure one blank line
            elif output_paragraphs and output_paragraphs[-1] != "":
                output_paragraphs.append("")
        elif line_cleaned: # Line has content after cleaning
            current_paragraph_lines.append(line_cleaned)
        # If line_cleaned is empty but line_raw was not (e.g. only contained spaces after regex),
        # it might be a soft break. The current logic will treat it as ignorable
        # if it doesn't trigger is_actually_blank(line_raw).
        # This might need adjustment if some files use space-only lines as para breaks.

    # Add any remaining paragraph
    if current_paragraph_lines:
        output_paragraphs.append(" ".join(current_paragraph_lines))

    # Filter out multiple consecutive blank lines that might have been introduced
    final_text_list = []
    if output_paragraphs:
        final_text_list.append(output_paragraphs[0]) # Add first element
        for j in range(1, len(output_paragraphs)):
            # Add current element if it's not blank, OR if it is blank but previous wasn't
            if output_paragraphs[j] != "" or (output_paragraphs[j] == "" and output_paragraphs[j-1] != ""):
                final_text_list.append(output_paragraphs[j])
    
    # Remove trailing blank line if any
    if final_text_list and final_text_list[-1] == "":
        final_text_list.pop()

    return "\n".join(final_text_list)


def process_file(input_file_path, output_dir, file_name_override=None):
    print(f"Processing file: {input_file_path}")
    try:
        with open(input_file_path, 'r', encoding='utf-8', errors='ignore') as f:
            raw_content = f.read()
    except Exception as e:
        print(f"Error reading file {input_file_path}: {e}")
        return

    cleaned_content = clean_gutenberg_text(raw_content)

    if not cleaned_content.strip():
        print(f"Warning: No substantial content processed for {input_file_path}. Output file may be empty or nearly empty.")

    base_name = os.path.basename(input_file_path)
    if file_name_override:
        output_file_stem = file_name_override
    else:
        output_file_stem = os.path.splitext(base_name)[0]
        
    output_file_name = output_file_stem + "_cleaned.txt"
    output_file_path = os.path.join(output_dir, output_file_name)

    try:
        os.makedirs(output_dir, exist_ok=True)
        with open(output_file_path, 'w', encoding='utf-8') as f:
            f.write(cleaned_content)
        print(f"Cleaned file saved to: {output_file_path}")
    except Exception as e:
        print(f"Error writing file {output_file_path}: {e}")

def process_directory(input_dir, output_dir, extension_to_process):
    if not os.path.isdir(input_dir):
        print(f"Error: Input directory '{input_dir}' not found or is not a directory.")
        return
    
    print(f"Processing all '{extension_to_process}' files in directory: {input_dir}")
    found_files = False
    for filename in os.listdir(input_dir):
        if filename.lower().endswith(extension_to_process.lower()):
            found_files = True
            file_path = os.path.join(input_dir, filename)
            process_file(file_path, output_dir)
    if not found_files:
        print(f"No files with extension '{extension_to_process}' found in '{input_dir}'.")

def main():
    parser = argparse.ArgumentParser(description="Clean Project Gutenberg text files for TTS.")
    parser.add_argument("input_path", nargs='?', default=None, 
                        help="Path to a single .txt file or a directory. If not provided, uses config.toml.")
    parser.add_argument('-o', '--output_dir', default=None,
                        help="Directory to save cleaned files. Overrides config-based 'Prepped' if provided.")
    parser.add_argument('-e', '--extension', default=DEFAULT_FILE_EXTENSION,
                        help=f"File extension to process in directory mode (default: {DEFAULT_FILE_EXTENSION})")
    parser.add_argument('--config', default=DEFAULT_CONFIG_FILE_PATH,
                        help=f"Path to the config.toml file (default: {DEFAULT_CONFIG_FILE_PATH} in CWD).")

    args = parser.parse_args()

    if args.input_path:
        input_path = args.input_path
        output_dir_to_use = args.output_dir if args.output_dir else "cleaned_texts_arg_mode"
        extension_to_process = args.extension

        if not os.path.exists(input_path):
            print(f"Error: Input path '{input_path}' does not exist.")
            return

        if os.path.isfile(input_path):
            process_file(input_path, output_dir_to_use)
        elif os.path.isdir(input_path):
            process_directory(input_path, output_dir_to_use, extension_to_process)
        else:
            print(f"Error: Input path '{input_path}' is not a valid file or directory.")
    else:
        print("No input_path provided. Using config.toml for default operation.")
        
        app_config = load_app_config(args.config)
        if not app_config:
            return

        content_project_root = app_config.get("content_project_dir")
        if not content_project_root:
            print(f"Error: 'content_project_dir' not found in '{args.config}'.")
            return
        
        content_project_root = os.path.expanduser(os.path.expandvars(content_project_root))

        if not os.path.isdir(content_project_root):
            print(f"Error: 'content_project_dir' ('{content_project_root}') from config is not a valid directory.")
            return

        raw_dir_path = os.path.join(content_project_root, DEFAULT_INPUT_SUBDIR)
        prepped_dir_path = args.output_dir if args.output_dir else os.path.join(content_project_root, DEFAULT_OUTPUT_SUBDIR)
        
        process_directory(raw_dir_path, prepped_dir_path, args.extension)

if __name__ == "__main__":
    main()