# Filename: raw2stage.py
import re
from pathlib import Path
import sys

# --- Configuration for TOML parsing ---
try:
    import tomllib
except ImportError:
    try:
        import toml as tomllib
    except ImportError:
        print("TOML library not found. Please install 'toml' or use Python 3.11+.")
        sys.exit(1)

# --- Stanza Setup (replaces NLTK) ---
try:
    import stanza
except ImportError:
    print("Stanza library not found. Please run `pip install stanza`.")
    sys.exit(1)

# --- Regexes for Chapter/Section Detection ---
BRACKETED_ARABIC_CHAPTER_REGEX = re.compile(r"^\s*\[\s*(\d+)\s*\]\s*$")
BRACKETED_ROMAN_CHAPTER_REGEX = re.compile(r"^\s*\[\s*([IVXLCDM]+)\s*\]\s*$", re.IGNORECASE)
EM_DASH_SECTION_REGEX = re.compile(r"^\s*—\s*([IVXLCDM]+)\s*—\s*$", re.IGNORECASE)
CHAPTER_MAIN_REGEX = re.compile(
    r"^\s*(CHAPTER)\s+([IVXLCDM\d]+(?:st|nd|rd|th)?|[A-ZÀ-ÖØ-ÞĀ-ĒĪ-ŌŪ-Ž'-]+)\s*[:.]?\s*(.*)$",
    re.IGNORECASE,
)
KNOWN_WORD_NUMBERS = {
    "ZERO", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX", "SEVEN", "EIGHT", "NINE", "TEN",
    "ELEVEN", "TWELVE", "THIRTEEN", "FOURTEEN", "FIFTEEN", "SIXTEEN", "SEVENTEEN", "EIGHTEEN", "NINETEEN",
    "TWENTY", "THIRTY", "FORTY", "FIFTY", "SIXTY", "SEVENTY", "EIGHTY", "NINETY", "HUNDRED", "THOUSAND",
    "FIRST", "SECOND", "THIRD", "FOURTH", "FIFTH", "SIXTH", "SEVENTH", "EIGHTH", "NINTH", "TENTH",
}
SHORT_LINE_NUMERAL_CHAPTER_REGEX = re.compile(r"^\s*([IVXLCDM\d]+)\s*[:.]?\s*$", re.IGNORECASE)
SPECIAL_SECTION_REGEX = re.compile(
    r"^\s*(PREFACE|INTRODUCTION|EPILOGUE|PROLOGUE|CONTENTS|APPENDIX|GLOSSARY|FORWARD|FOREWORD)(?:[:.\s]|$)",
    re.IGNORECASE,
)

# --- Regexes for cleaning italics/underscores ---
def clean_italics_and_underscores(text: str) -> str:
    """
    Intelligently cleans text containing underscores used for italics.
    """
    text = re.sub(r'\b_([^_]+)_([.,!?;:])', r'\1\2', text)
    text = re.sub(r'\b_([^_]+)_\b', r'\1', text)
    text = re.sub(r'_', ' ', text)
    return text

def load_config(config_path_str="config.toml"):
    config_path = Path(config_path_str)
    if not config_path.is_file():
        print(f"Error: Configuration file '{config_path}' not found.")
        return None
    try:
        with open(config_path, "rb") as f:
            config = tomllib.load(f)
        return config
    except Exception as e:
        print(f"Error parsing TOML file '{config_path}': {e}")
        return None

def filter_paragraph_lines_for_bracket_content(lines_in_raw_para: list[str]) -> tuple[str, str]:
    overall_cleaned_line_strings = []
    overall_junk_line_strings = []
    bracket_depth = 0
    for line_text in lines_in_raw_para:
        current_line_clean_chars = []
        current_line_junk_chars = []
        line_became_entirely_junk_due_to_filtering = True
        char_idx_first_non_whitespace = -1
        for ci, char_test in enumerate(line_text):
            if not char_test.isspace():
                char_idx_first_non_whitespace = ci
                break
        if char_idx_first_non_whitespace == -1:
            if bracket_depth == 0:
                current_line_clean_chars.extend(list(line_text))
                line_became_entirely_junk_due_to_filtering = False
            else:
                current_line_junk_chars.extend(list(line_text))
        else:
            original_leading_whitespace = line_text[:char_idx_first_non_whitespace]
            for char_idx, char_val in enumerate(line_text[char_idx_first_non_whitespace:], start=char_idx_first_non_whitespace):
                is_first_nw_char_on_line = char_idx == char_idx_first_non_whitespace
                if char_val == '[':
                    if bracket_depth == 0 and is_first_nw_char_on_line:
                        bracket_depth += 1
                        current_line_junk_chars.extend(list(original_leading_whitespace))
                        current_line_junk_chars.append(char_val)
                    elif bracket_depth > 0:
                        bracket_depth += 1
                        current_line_junk_chars.append(char_val)
                    else:
                        if line_became_entirely_junk_due_to_filtering:
                            current_line_clean_chars.extend(list(original_leading_whitespace))
                        current_line_clean_chars.append(char_val)
                        line_became_entirely_junk_due_to_filtering = False
                elif char_val == ']':
                    if bracket_depth > 0:
                        current_line_junk_chars.append(char_val)
                        bracket_depth -= 1
                    else:
                        if line_became_entirely_junk_due_to_filtering:
                            current_line_clean_chars.extend(list(original_leading_whitespace))
                        current_line_clean_chars.append(char_val)
                        line_became_entirely_junk_due_to_filtering = False
                else:
                    if bracket_depth == 0:
                        if line_became_entirely_junk_due_to_filtering:
                            current_line_clean_chars.extend(list(original_leading_whitespace))
                        current_line_clean_chars.append(char_val)
                        line_became_entirely_junk_due_to_filtering = False
                    else:
                        current_line_junk_chars.append(char_val)
        if not line_became_entirely_junk_due_to_filtering:
            overall_cleaned_line_strings.append("".join(current_line_clean_chars))
        overall_junk_line_strings.append("".join(current_line_junk_chars))
    
    # This now correctly returns the paragraph as a single string
    final_cleaned_text = " ".join(s.strip() for s in overall_cleaned_line_strings if s.strip())
    final_cleaned_text = re.sub(r'\s+', ' ', final_cleaned_text).strip()

    final_junk_text = "\n".join(overall_junk_line_strings)
    return final_cleaned_text, final_junk_text

def process_text_to_staged_format(
    text_content: str, initial_sentence_counter: int, stanza_nlp
) -> tuple[list[str], str, int]:
    raw_paragraphs = re.split(r'(?:\n\s*){2,}', text_content.strip())
    staged_output_lines = []
    collected_junk_text_parts = []
    current_sentence_counter = initial_sentence_counter

    for raw_para_idx, raw_para_text_unstripped in enumerate(raw_paragraphs):
        raw_para_text = raw_para_text_unstripped.strip()
        if not raw_para_text:
            continue
        
        is_chapter_heading = False
        chapter_display_text = ""
        paragraph_fully_handled = False
        
        # --- Chapter Heading Logic (Unchanged) ---
        match_bracket_arabic = BRACKETED_ARABIC_CHAPTER_REGEX.match(raw_para_text)
        if match_bracket_arabic:
            is_chapter_heading = True
            chapter_display_text = f"Chapter {match_bracket_arabic.group(1).strip()}"
            collected_junk_text_parts.append(f"--- Junk from identified bracketed chapter marker (Para {raw_para_idx + 1}) ---\n{raw_para_text_unstripped.strip()}")
            paragraph_fully_handled = True
        else:
            match_bracket_roman = BRACKETED_ROMAN_CHAPTER_REGEX.match(raw_para_text)
            if match_bracket_roman:
                is_chapter_heading = True
                chapter_display_text = f"Chapter {match_bracket_roman.group(1).strip().upper()}"
                collected_junk_text_parts.append(f"--- Junk from identified bracketed chapter marker (Para {raw_para_idx + 1}) ---\n{raw_para_text_unstripped.strip()}")
                paragraph_fully_handled = True
        
        cleaned_para_text_for_processing = ""
        if not paragraph_fully_handled:
            original_lines = raw_para_text_unstripped.splitlines()
            cleaned_para_text_for_processing, junk_from_filter = filter_paragraph_lines_for_bracket_content(original_lines)
            if junk_from_filter.strip():
                collected_junk_text_parts.append(f"--- Junk from bracket filtering (Para {raw_para_idx + 1}) ---\n{junk_from_filter}")
            
            cleaned_para_text_for_processing = clean_italics_and_underscores(cleaned_para_text_for_processing)
            
            if not cleaned_para_text_for_processing:
                paragraph_fully_handled = True
        
        if not paragraph_fully_handled:
            match_em_dash_section = EM_DASH_SECTION_REGEX.match(cleaned_para_text_for_processing)
            if match_em_dash_section:
                numeral = match_em_dash_section.group(1).strip().upper()
                collected_junk_text_parts.append(f"--- Junk from identified em-dash section marker: Part {numeral} (Para {raw_para_idx + 1}) ---\n{cleaned_para_text_for_processing}")
                paragraph_fully_handled = True

        if not paragraph_fully_handled:
            match_main = CHAPTER_MAIN_REGEX.match(cleaned_para_text_for_processing)
            if match_main:
                is_chapter_heading = True
                potential_num_or_title_word = match_main.group(2).strip().upper()
                rest_of_title = match_main.group(3).strip()
                is_actual_numeral = bool(re.fullmatch(r"[IVXLCDM\d]+(?:ST|ND|RD|TH)?", potential_num_or_title_word, re.IGNORECASE))
                is_known_word_number = potential_num_or_title_word in KNOWN_WORD_NUMBERS
                
                if is_actual_numeral or is_known_word_number:
                    chapter_display_text = f"Chapter {potential_num_or_title_word}"
                    if rest_of_title: chapter_display_text += f": {rest_of_title.strip('. ')}"
                else:
                    full_title_part = f"{potential_num_or_title_word} {rest_of_title}".strip()
                    chapter_display_text = f"Chapter {full_title_part}"
            
            if not is_chapter_heading:
                match_special = SPECIAL_SECTION_REGEX.match(cleaned_para_text_for_processing)
                if match_special:
                    is_chapter_heading = True
                    chapter_display_text = match_special.group(1).strip().title()

            if not is_chapter_heading and len(cleaned_para_text_for_processing) < 30:
                match_short_numeral = SHORT_LINE_NUMERAL_CHAPTER_REGEX.match(cleaned_para_text_for_processing)
                if match_short_numeral:
                    is_chapter_heading = True
                    chapter_display_text = f"Chapter {match_short_numeral.group(1).strip().upper()}"
            
            paragraph_fully_handled = is_chapter_heading

        if is_chapter_heading:
            chapter_display_text = re.sub(r'\s+', ' ', chapter_display_text).strip()
            if chapter_display_text:
                staged_output_lines.append(f"%%CHAPTER_MARKER%% {chapter_display_text}")
                staged_output_lines.append(f"{{S{current_sentence_counter}: {chapter_display_text}}}")
                current_sentence_counter += 1
        
        elif not paragraph_fully_handled and cleaned_para_text_for_processing:
            # --- THIS IS THE ROBUST LOGIC BLOCK ---
            # 1. Re-join lines with soft breaks into a single string.
            lines = cleaned_para_text_for_processing.strip().splitlines()
            rejoined_paragraph = " ".join(line.strip() for line in lines)
            rejoined_paragraph = re.sub(r'\s+', ' ', rejoined_paragraph).strip()
            
            if not rejoined_paragraph:
                continue

            # 2. Now, sentence-split the re-joined paragraph with Stanza.
            try:
                doc = stanza_nlp(rejoined_paragraph)
                sentences_in_para = [sent.text for sent in doc.sentences]
            except Exception as e:
                print(f"Warning: Stanza error processing paragraph: '{rejoined_paragraph[:100]}...'. Error: {e}. Skipping.")
                continue
            # --- END OF ROBUST LOGIC BLOCK ---

            for sent_text in sentences_in_para:
                test_sent_alphanum = "".join(filter(str.isalnum, sent_text))
                if not test_sent_alphanum or (len(test_sent_alphanum) < 2 and len(sent_text) < 6):
                    continue
                cleaned_sentence = re.sub(r'\s+', ' ', sent_text).strip()
                if not cleaned_sentence:
                    continue
                staged_output_lines.append(f"{{S{current_sentence_counter}: {cleaned_sentence}}}")
                current_sentence_counter += 1
    
    final_all_junk_text = "\n\n".join(collected_junk_text_parts)
    return staged_output_lines, final_all_junk_text, current_sentence_counter

def main():
    config = load_config()
    if not config:
        sys.exit(1)
    content_project_dir_str = config.get("content_project_dir")
    if not content_project_dir_str:
        print("Error: 'content_project_dir' not found in config.toml.")
        sys.exit(1)

    content_project_path = Path(content_project_dir_str)
    input_dir_name = "RawText"
    raw_files_dir = content_project_path / input_dir_name
    staged_output_dir = content_project_path / "StagedWaiting"

    if not raw_files_dir.is_dir():
        print(f"Error: Input directory for raw files not found at '{raw_files_dir}'")
        sys.exit(1)

    print("Initializing Stanza English pipeline for sentence splitting (this may take a moment)...")
    try:
        stanza_nlp = stanza.Pipeline('en', processors='tokenize', use_gpu=False)
    except Exception as e:
        print(f"Could not initialize Stanza. Ensure models are downloaded. Error: {e}")
        sys.exit(1)
    print("Stanza pipeline loaded.")

    staged_output_dir.mkdir(parents=True, exist_ok=True)
    print(f"Processing files from: {raw_files_dir}")
    print(f"Saving processed files to: {staged_output_dir}")

    processed_count, skipped_count, error_count = 0, 0, 0
    for raw_file_path in raw_files_dir.glob('*.txt'):
        print(f"\n--- Analyzing: {raw_file_path.name} ---")
        base_name = raw_file_path.stem
        staged_text_file_path = staged_output_dir / f"{base_name}.txt"
        junk_file_path = staged_output_dir / f"{base_name}.junk.txt"

        if staged_text_file_path.exists() and junk_file_path.exists():
            print(f"Skipping '{raw_file_path.name}': Output files already exist.")
            skipped_count += 1
            continue
        
        print(f"Processing '{raw_file_path.name}'...")
        try:
            with open(raw_file_path, 'r', encoding='utf-8', errors='ignore') as f:
                raw_content = f.read()
        except Exception as e:
            print(f"Error reading file '{raw_file_path.name}': {e}")
            error_count += 1
            continue

        current_sentence_idx = 1
        all_staged_lines_for_book, all_junk_for_book_parts = [], []

        if not raw_content.strip():
            print(f"Skipping '{raw_file_path.name}': File is empty.")
            skipped_count += 1
            continue
        
        staged_lines, junk_text, _ = process_text_to_staged_format(raw_content, current_sentence_idx, stanza_nlp)
        all_staged_lines_for_book.extend(staged_lines)
        if junk_text.strip():
            all_junk_for_book_parts.append(junk_text)

        final_staged_text_output = "\n".join(all_staged_lines_for_book)
        if not final_staged_text_output:
            print(f"No processable output for '{raw_file_path.name}'.")
        
        try:
            with open(staged_text_file_path, 'w', encoding='utf-8') as f_s:
                f_s.write(final_staged_text_output)
            print(f"Saved staged text to '{staged_text_file_path.name}'")
        except Exception as e_s:
            print(f"Error writing staged file '{staged_text_file_path.name}': {e_s}")
            error_count += 1
            continue
        
        final_junk_output = "\n\n".join(all_junk_for_book_parts)
        try:
            with open(junk_file_path, 'w', encoding='utf-8') as f_j:
                f_j.write(final_junk_output)
            print(f"Saved junk to '{junk_file_path.name}'")
        except Exception as e_j:
            print(f"Error writing junk file '{junk_file_path.name}': {e_j}")
            error_count += 1
        
        processed_count += 1

    print(f"\n--- Processing Complete ---")
    print(f"Processed {processed_count} file(s).")
    print(f"Skipped {skipped_count} file(s).")
    if error_count > 0:
        print(f"Encountered errors in {error_count} file operation(s).")

if __name__ == "__main__":
    main()