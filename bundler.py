import os
import argparse

# --- Configuration ---
DEFAULT_OUTPUT_FILE = "mono.out"
DEFAULT_EXTENSIONS = ['.zig', '.py', '.md', '.txt', '.rs', '.toml'] # Added .rs and .toml
DEFAULT_IGNORE_PATHS_OR_NAMES = [
    '.git', '__pycache__', 'zig-cache', 'zig-out', 'stage', 'data', 'output_audio',
    DEFAULT_OUTPUT_FILE, 'mono.in', '.DS_Store',
    'target', # Added Rust target directory
]

# START and END markers
START_MARKER_TPL = "//*** START FILE: {} ***//"
END_MARKER_TPL = "//*** END FILE: {} ***//"

def should_ignore(current_path_abs, base_for_rel_path_calc, ignore_list):
    base_name = os.path.basename(current_path_abs)
    if base_name in ignore_list:
        return True 

    try:
        relative_current_path = os.path.relpath(current_path_abs, base_for_rel_path_calc).replace(os.sep, '/')
    except ValueError:
        relative_current_path = None

    if relative_current_path:
        for pattern in ignore_list:
            normalized_pattern = pattern.replace(os.sep, '/')
            if relative_current_path == normalized_pattern:
                return True
            # Check if current_path_abs is a directory and the pattern is a prefix
            # e.g. ignore "target" should ignore "target/debug"
            if os.path.isdir(current_path_abs) and relative_current_path.startswith(normalized_pattern + '/'):
                 return True
            # Check if pattern is for a directory and current_path_abs is inside it
            # e.g. ignore "src/old_stuff" should ignore "src/old_stuff/file.rs"
            if normalized_pattern.endswith('/') and relative_current_path.startswith(normalized_pattern): # pattern is dir
                return True
            if not normalized_pattern.endswith('/') and pattern + '/' == relative_current_path + '/': # basename dir match
                 return True


    return False


def bundle_files(paths_to_bundle, output_file, extensions, ignore_list):
    with open(output_file, 'w', encoding='utf-8') as outfile:
        processed_paths = set()
        base_for_relative_paths = os.getcwd() # This is CWD where bundler.py is run

        for path_item_arg in paths_to_bundle:
            # path_item_arg can be like "src", "./src", "Cargo.toml"
            path_item_abs = os.path.abspath(path_item_arg) # Resolve to absolute path

            if should_ignore(path_item_abs, base_for_relative_paths, ignore_list):
                 print(f"Ignoring top-level argument: {path_item_arg} (resolved to {path_item_abs})")
                 continue

            if not os.path.exists(path_item_abs):
                print(f"Warning: Path not found, skipping: {path_item_arg} (resolved to {path_item_abs})")
                continue

            if os.path.isfile(path_item_abs):
                if path_item_abs in processed_paths: continue
                processed_paths.add(path_item_abs)
                
                display_path = os.path.relpath(path_item_abs, base_for_relative_paths).replace(os.sep, '/')

                # Check extensions for files
                file_ext = os.path.splitext(path_item_abs)[1]
                if file_ext.lower() in [e.lower() for e in extensions]: # Case-insensitive extension check
                    try:
                        with open(path_item_abs, 'r', encoding='utf-8', errors='ignore') as infile: # errors='ignore' for problematic files
                            content = infile.read()
                            outfile.write(START_MARKER_TPL.format(display_path) + "\n")
                            outfile.write(content)
                            if content and not content.endswith('\n'):
                                outfile.write("\n")
                            outfile.write(END_MARKER_TPL.format(display_path) + "\n\n")
                        print(f"Bundled: {path_item_arg} (as {display_path})")
                    except Exception as e:
                        print(f"Error reading file {path_item_abs}: {e}")
                # else:
                #     print(f"Skipping file due to extension mismatch: {path_item_arg} (ext: {file_ext})")


            elif os.path.isdir(path_item_abs):
                # For os.walk, the 'root' paths will be absolute if path_item_abs is absolute.
                for root, dirs, files in os.walk(path_item_abs, topdown=True):
                    # Pruning directories using should_ignore
                    # Check should be relative to base_for_relative_paths (e.g. CWD)
                    dirs[:] = [d for d in dirs if not should_ignore(os.path.abspath(os.path.join(root, d)), base_for_relative_paths, ignore_list)]
                    
                    for file_name in files:
                        file_abs_path = os.path.abspath(os.path.join(root, file_name))
                        if file_abs_path in processed_paths: continue
                        
                        # We need to check should_ignore for each file as well,
                        # as ignore_list can contain full relative file paths.
                        if should_ignore(file_abs_path, base_for_relative_paths, ignore_list):
                            # print(f"Ignoring file within directory: {file_abs_path}")
                            continue
                        
                        processed_paths.add(file_abs_path)
                        display_path = os.path.relpath(file_abs_path, base_for_relative_paths).replace(os.sep, '/')
                        
                        file_ext = os.path.splitext(file_name)[1]
                        if file_ext.lower() in [e.lower() for e in extensions]: # Case-insensitive extension check
                            try:
                                with open(file_abs_path, 'r', encoding='utf-8', errors='ignore') as infile:
                                    content = infile.read()
                                    outfile.write(START_MARKER_TPL.format(display_path) + "\n")
                                    outfile.write(content)
                                    if content and not content.endswith('\n'):
                                        outfile.write("\n")
                                    outfile.write(END_MARKER_TPL.format(display_path) + "\n\n")
                                print(f"Bundled: {os.path.join(root, file_name)} (as {display_path})")
                            except Exception as e:
                                print(f"Error reading file {file_abs_path}: {e}")
                        # else:
                        #    print(f"Skipping file in dir due to extension: {file_abs_path} (ext: {file_ext})")
                            
    print(f"\nBundle created: {output_file}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Bundle multiple text files into one using //*** START/END FILE: path ***// delimiters.")
    parser.add_argument('paths', nargs='+', help="List of files or directories to bundle (e.g., ./src, Cargo.toml).")
    parser.add_argument('-o', '--output', default=DEFAULT_OUTPUT_FILE,
                        help=f"Output file name (default: {DEFAULT_OUTPUT_FILE})")
    parser.add_argument('-e', '--extensions', nargs='+', default=DEFAULT_EXTENSIONS,
                        help=f"File extensions to include (default: {' '.join(DEFAULT_EXTENSIONS)})")
    parser.add_argument('-i', '--ignore', nargs='+', default=None, # Default to None to better merge with defaults
                        help="File/directory names or relative paths to ignore.")

    args = parser.parse_args()

    # Combine default ignores with user-provided ignores
    current_ignore_list = list(DEFAULT_IGNORE_PATHS_OR_NAMES) # Start with defaults
    if args.ignore: # If user provided ignores, add them
        current_ignore_list.extend(args.ignore)
    current_ignore_list = list(set(current_ignore_list)) # Remove duplicates

    # Ensure output file and mono.in are in the ignore list (using their basenames)
    output_basename = os.path.basename(args.output)
    if output_basename not in current_ignore_list:
        current_ignore_list.append(output_basename)
    
    # Add default output file's basename if it's different and not already covered
    default_output_basename_const = os.path.basename(DEFAULT_OUTPUT_FILE) # from constant
    if default_output_basename_const != output_basename and default_output_basename_const not in current_ignore_list:
         current_ignore_list.append(default_output_basename_const)
    
    if 'mono.in' not in current_ignore_list:
        current_ignore_list.append('mono.in')
    
    # Ensure 'target' (common for Rust) is in the ignore list if not already added by user
    if 'target' not in current_ignore_list:
        current_ignore_list.append('target')


    print(f"Bundling with extensions: {args.extensions}")
    print(f"Bundling with ignore list: {current_ignore_list}")

    bundle_files(args.paths, args.output, args.extensions, current_ignore_list)
