import os
import argparse

# --- Configuration ---
DEFAULT_OUTPUT_FILE = "mono.out"
DEFAULT_EXTENSIONS = ['.zig', '.py', '.md', '.txt']
DEFAULT_IGNORE_PATHS_OR_NAMES = [
    '.git', '__pycache__', 'zig-cache', 'zig-out', 'stage', 'data', 'output_audio',
    DEFAULT_OUTPUT_FILE, 'mono.in', '.DS_Store'
]

# START and END markers
START_MARKER_TPL = "//*** START FILE: {} ***//"
END_MARKER_TPL = "//*** END FILE: {} ***//"

def should_ignore(current_path_abs, base_for_rel_path_calc, ignore_list):
    """
    Determines if a given path should be ignored.
    - Ignores if the basename of current_path_abs matches any entry in ignore_list.
    - Ignores if the path of current_path_abs relative to base_for_rel_path_calc matches any entry in ignore_list.
    """
    base_name = os.path.basename(current_path_abs)
    if base_name in ignore_list:
        return True # Matches simple names like '.git', 'output.txt'

    try:
        # Calculate path relative to the defined base (e.g., CWD)
        relative_current_path = os.path.relpath(current_path_abs, base_for_rel_path_calc).replace(os.sep, '/')
    except ValueError:
        # current_path_abs is not under base_for_rel_path_calc (e.g. different drive)
        # In this case, only basename matching (done above) could apply.
        # Relative path patterns in ignore_list won't match here.
        relative_current_path = None

    if relative_current_path:
        for pattern in ignore_list:
            normalized_pattern = pattern.replace(os.sep, '/')
            # Match if the full relative path is identical to a pattern
            # e.g. pattern "src/some_file.txt" matches file ".../project/src/some_file.txt"
            # e.g. pattern "src/build" matches directory ".../project/src/build"
            if relative_current_path == normalized_pattern:
                return True
    return False


def bundle_files(paths_to_bundle, output_file, extensions, ignore_list):
    with open(output_file, 'w', encoding='utf-8') as outfile:
        processed_paths = set()
        # This is the consistent base for calculating relative paths for display AND for ignore checks.
        base_for_relative_paths = os.getcwd()

        for path_item_arg in paths_to_bundle:
            path_item_abs = os.path.abspath(path_item_arg)

            # Check if the top-level argument itself should be ignored
            # Uses base_for_relative_paths for context, so ignore patterns are relative to CWD.
            if should_ignore(path_item_abs, base_for_relative_paths, ignore_list):
                 print(f"Ignoring top-level argument: {path_item_arg}")
                 continue

            if not os.path.exists(path_item_abs):
                print(f"Warning: Path not found, skipping: {path_item_arg}")
                continue

            if os.path.isfile(path_item_abs):
                if path_item_abs in processed_paths: continue
                processed_paths.add(path_item_abs)
                # Display path is relative to CWD.
                display_path = os.path.relpath(path_item_abs, base_for_relative_paths).replace(os.sep, '/')

                # Ignore check uses base_for_relative_paths for context.
                if not should_ignore(path_item_abs, base_for_relative_paths, ignore_list) and \
                   any(path_item_abs.endswith(ext) for ext in extensions):
                    try:
                        with open(path_item_abs, 'r', encoding='utf-8') as infile:
                            content = infile.read()
                            outfile.write(START_MARKER_TPL.format(display_path) + "\n")
                            outfile.write(content)
                            if content and not content.endswith('\n'):
                                outfile.write("\n")
                            outfile.write(END_MARKER_TPL.format(display_path) + "\n\n")
                        print(f"Bundled: {path_item_arg} (as {display_path})")
                    except Exception as e:
                        print(f"Error reading file {path_item_abs}: {e}")

            elif os.path.isdir(path_item_abs):
                for root, dirs, files in os.walk(path_item_abs, topdown=True):
                    # Pruning directories:
                    # Check should be relative to base_for_relative_paths (e.g. CWD)
                    # so patterns like "src/build" work correctly regardless of how deep the walk is.
                    dirs[:] = [d for d in dirs if not should_ignore(os.path.abspath(os.path.join(root, d)), base_for_relative_paths, ignore_list)]
                    
                    for file_name in files:
                        file_abs_path = os.path.abspath(os.path.join(root, file_name))
                        if file_abs_path in processed_paths: continue
                        processed_paths.add(file_abs_path)
                        # Display path is relative to CWD.
                        display_path = os.path.relpath(file_abs_path, base_for_relative_paths).replace(os.sep, '/')

                        # Ignore check for files within walked directories:
                        # Also relative to base_for_relative_paths.
                        if not should_ignore(file_abs_path, base_for_relative_paths, ignore_list) and \
                           any(file_name.endswith(ext) for ext in extensions):
                            try:
                                with open(file_abs_path, 'r', encoding='utf-8') as infile:
                                    content = infile.read()
                                    outfile.write(START_MARKER_TPL.format(display_path) + "\n")
                                    outfile.write(content)
                                    if content and not content.endswith('\n'):
                                        outfile.write("\n")
                                    outfile.write(END_MARKER_TPL.format(display_path) + "\n\n")
                                print(f"Bundled: {os.path.join(root, file_name)} (as {display_path})") # Using join for original path
                            except Exception as e:
                                print(f"Error reading file {file_abs_path}: {e}")
    print(f"\nBundle created: {output_file}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Bundle multiple text files into one using //*** START/END FILE: path ***// delimiters.")
    parser.add_argument('paths', nargs='+', help="List of files or directories to bundle (e.g., ./src, file.zig).")
    parser.add_argument('-o', '--output', default=DEFAULT_OUTPUT_FILE,
                        help=f"Output file name (default: {DEFAULT_OUTPUT_FILE})")
    parser.add_argument('-e', '--extensions', nargs='+', default=DEFAULT_EXTENSIONS,
                        help=f"File extensions to include (default: {' '.join(DEFAULT_EXTENSIONS)})")
    parser.add_argument('-i', '--ignore', nargs='+', default=DEFAULT_IGNORE_PATHS_OR_NAMES,
                        help="File/directory names or relative paths to ignore.")

    args = parser.parse_args()
    # Ensure output file and mono.in are in the ignore list (using their basenames for simplicity)
    # This way, they are ignored if they appear anywhere, which is usually desired for these specific files.
    current_ignore_list = list(set(args.ignore)) # Use set to handle potential duplicates from default and args

    output_basename = os.path.basename(args.output) # Use the actual output file name from args
    if output_basename not in current_ignore_list:
        current_ignore_list.append(output_basename)

    # Add default output file's basename if it's different and not already covered
    default_output_basename = os.path.basename(DEFAULT_OUTPUT_FILE)
    if default_output_basename != output_basename and default_output_basename not in current_ignore_list:
         current_ignore_list.append(default_output_basename)
    
    if 'mono.in' not in current_ignore_list and os.path.basename('mono.in') not in current_ignore_list : # os.path.basename is redundant here
        current_ignore_list.append('mono.in')

    bundle_files(args.paths, args.output, args.extensions, current_ignore_list)
