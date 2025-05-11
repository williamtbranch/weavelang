import os
import re
import argparse

# --- Configuration ---
DEFAULT_INPUT_FILE = "mono.in"
UTF8_BOM = '\ufeff'

# Regex patterns for the new markers
# Match "//*** START FILE: <path> ***//"
START_MARKER_REGEX = re.compile(r"//\*{3}\s*START FILE:\s*(.*?)\s*\*{3}//")
# Match "//*** END FILE: <path> ***//"
END_MARKER_REGEX = re.compile(r"//\*{3}\s*END FILE:\s*(.*?)\s*\*{3}//")

def unbundle_files(input_file):
    if not os.path.exists(input_file):
        print(f"Error: Input file '{input_file}' not found.")
        return

    try:
        with open(input_file, 'r', encoding='utf-8') as infile: # Read mono.in as utf-8
            lines = infile.readlines()
    except Exception as e:
        print(f"Error reading input file {input_file}: {e}")
        return

    current_file_path = None
    current_file_lines = []
    files_unbundled_count = 0

    for line_number, line_content in enumerate(lines): # Use enumerate for better error reporting if needed
        start_match = START_MARKER_REGEX.match(line_content)
        end_match = END_MARKER_REGEX.match(line_content)

        if start_match:
            if current_file_path:
                print(f"Warning: Found START marker for '{start_match.group(1).strip()}' before END marker for '{current_file_path}'. Previous file content might be incomplete.")
            current_file_path = start_match.group(1).strip()
            current_file_lines = []
        elif end_match:
            end_file_path = end_match.group(1).strip()
            if current_file_path is None:
                print(f"Warning: Found END marker for '{end_file_path}' without corresponding START marker. Skipping.")
            else:
                if end_file_path != current_file_path:
                    print(f"Warning: END marker path '{end_file_path}' does not match current START marker path '{current_file_path}'. Saving content under '{current_file_path}'.")
                
                # --- Write file (common logic for matched or mismatched END marker) ---
                try:
                    # Remove UTF-8 BOM from the first line of content if present
                    if current_file_lines and current_file_lines[0].startswith(UTF8_BOM):
                        current_file_lines[0] = current_file_lines[0][len(UTF8_BOM):]
                        # If the first line ONLY contained the BOM and is now empty,
                        # and it was the only line, current_file_lines[0] will be "".
                        # If there were other lines, current_file_lines[0] might be just "\n" or actual content.
                        # This is generally fine. An empty current_file_lines[0] will write an empty line if followed by \n.

                    # Ensure the directory exists
                    dir_name = os.path.dirname(current_file_path)
                    if dir_name: # Create directory only if path includes one
                        os.makedirs(dir_name, exist_ok=True)
                    
                    # Write the file using 'utf-8' encoding (which does NOT add a BOM by default)
                    with open(current_file_path, 'w', encoding='utf-8', newline='') as outfile:
                        # newline='' is important to prevent Python from converting \n to \r\n on Windows
                        # for lines that already have \n from the input.
                        # We want to preserve the line endings as they were in the bundle.
                        outfile.writelines(current_file_lines)
                    print(f"Unbundled: {current_file_path}")
                    files_unbundled_count += 1
                except Exception as e:
                    print(f"Error writing file {current_file_path}: {e}")
                
                current_file_path = None # Reset state
                current_file_lines = []
                # --- End write file ---

        elif current_file_path is not None:
            current_file_lines.append(line_content)

    if current_file_path is not None:
         print(f"Warning: Input file ended while processing '{current_file_path}'. File might be incomplete (missing END marker).")
         # Optionally save the incomplete file here. For now, we'll just warn.
         # If you want to save, reuse the writing logic from the END marker block.

    if files_unbundled_count > 0:
        print(f"\nUnbundling complete. Processed {files_unbundled_count} files from: {input_file}")
    else:
        print("No files were unbundled. Check input format and delimiters.")
        print(f"Expected format: {START_MARKER_REGEX.pattern} ...content... {END_MARKER_REGEX.pattern}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Unbundle files from a single monolithic file using //*** START/END FILE: path ***// delimiters.")
    parser.add_argument('input_file', nargs='?', default=DEFAULT_INPUT_FILE,
                        help=f"Input file to unbundle (default: {DEFAULT_INPUT_FILE})")

    args = parser.parse_args()
    unbundle_files(args.input_file)
