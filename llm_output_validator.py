import re
from typing import List, Tuple, Optional, Set # Added Set for type hinting

# --- Regex Constants ---
RE_S_SEGMENT_LINE = re.compile(r"^(S\d+)\((.*)\)$")
RE_PHRASE_ALIGN_LINE = re.compile(r"^(S\d+)\s*~\s*(.*?)\s*~\s*(.*)$")
RE_S_LEMMA_LINE = re.compile(r"^(S\d+)\s*::\s*(.*)$") # For SimSL and DIGLOT_MAP segment headers
RE_DIGLOT_ENTRY = re.compile(r"^(.*?)->(.*?)\((.*?)\)\s*\(([A-Za-z])\)$")
RE_S_ID_FORMAT = re.compile(r"^S\d+$") # To check S-ID format in LOCKED_PHRASE

# --- Section Marker Constants ---
# These are the sections expected for a standard LLM-processed sentence block
REQUIRED_SECTION_MARKERS = [
    "AdvS::", "SimS::", "SimE::",
    "SimS_Segments::", "PHRASE_ALIGN::", "SimSL::",
    "AdvSL::", "DIGLOT_MAP::"
]
OPTIONAL_SECTION_MARKERS = ["LOCKED_PHRASE::"]
ALL_POSSIBLE_MARKERS = REQUIRED_SECTION_MARKERS + OPTIONAL_SECTION_MARKERS


def get_section_content(
    original_block_lines: List[str],
    start_marker: str,
    all_known_markers: List[str]
) -> Tuple[Optional[List[str]], Optional[int]]:
    """
    Helper to extract lines belonging to a section from the original block lines.
    Returns a tuple: (list of content lines (stripped), start line index of the marker in original_block_lines)
    or (None, None) if marker not found or error.
    Content lines are individual lines of content, already stripped.
    If marker has content on the same line, that's the first content line.
    """
    start_line_idx = -1
    for i, line in enumerate(original_block_lines):
        if line.strip().startswith(start_marker):
            start_line_idx = i
            break
    if start_line_idx == -1:
        return None, None

    # Find where the next section starts to define the end of the current section
    end_line_idx_for_content = len(original_block_lines) # Default to end of block
    for i in range(start_line_idx + 1, len(original_block_lines)):
        line_stripped_for_next_marker_check = original_block_lines[i].strip()
        for marker_to_check_next in all_known_markers:
            if line_stripped_for_next_marker_check.startswith(marker_to_check_next):
                end_line_idx_for_content = i
                break
        if end_line_idx_for_content != len(original_block_lines): # Found next marker
            break
    
    section_content_lines = []
    # Check for content on the same line as the marker
    marker_line_itself_stripped = original_block_lines[start_line_idx].strip()
    if len(marker_line_itself_stripped) > len(start_marker): # Content exists after marker on same line
        content_on_marker_line = marker_line_itself_stripped[len(start_marker):].strip()
        if content_on_marker_line: # Add if not just whitespace
            section_content_lines.append(content_on_marker_line)
    
    # Add subsequent lines that belong to this section
    for i in range(start_line_idx + 1, end_line_idx_for_content):
        line_content = original_block_lines[i].strip()
        if line_content: # Add non-empty stripped lines
            section_content_lines.append(line_content)
            
    return section_content_lines, start_line_idx


def validate_llm_block(block_text: str) -> List[str]:
    """
    Validates the structural integrity of a single LLM-generated block.
    Returns a list of error strings if issues are found, otherwise an empty list.
    Assumes block_text is the core output from LLM, before script appends final END_SENTENCE.
    """
    errors: List[str] = []
    original_lines = block_text.splitlines() # Keep original lines for accurate indexing by get_section_content
    stripped_lines_for_initial_checks = [line.strip() for line in original_lines if line.strip()]

    if not stripped_lines_for_initial_checks:
        errors.append("Block is empty or contains only whitespace.")
        return errors

    # I. Fundamental Block Structure Issues
    if any("END_SENTENCE" in line for line in stripped_lines_for_initial_checks):
        errors.append("Block contains premature END_SENTENCE marker(s).")

    marker_indices: dict[str, int] = {} # Stores line index of found markers
    
    for marker in REQUIRED_SECTION_MARKERS:
        found_marker_at_index = -1
        for i, line_text in enumerate(original_lines):
            if line_text.strip().startswith(marker):
                if marker in marker_indices:
                    errors.append(f"Duplicate required section marker: {marker} (first at line {marker_indices[marker]+1}, new at {i+1}).")
                else: # Store first occurrence only for order checks
                    marker_indices[marker] = i
                found_marker_at_index = i # Mark as found for this specific marker
                # break # Don't break, to catch duplicates properly for *this* marker if any on later lines.
                # Correction: only one instance of each marker is expected. If found, store and assume it's the one.
                # The duplicate check handles if it appears again.
        if marker not in marker_indices : # If after checking all lines, marker was not added
            errors.append(f"Missing required section marker: {marker}")
            
    for marker in OPTIONAL_SECTION_MARKERS:
        for i, line_text in enumerate(original_lines):
            if line_text.strip().startswith(marker):
                if marker in marker_indices: # Check against all markers already found
                    errors.append(f"Duplicate optional section marker: {marker} (first at line {marker_indices[marker]+1}, new at {i+1}).")
                else:
                    marker_indices[marker] = i
                # break # Optional markers can appear once.

    if errors: return errors # Stop if fundamental markers are missing/duplicated

    # Check order of REQUIRED markers
    last_idx = -1
    for i, marker_in_order in enumerate(REQUIRED_SECTION_MARKERS):
        current_marker_actual_idx = marker_indices[marker_in_order] # Assumes all required markers are now in marker_indices
        if current_marker_actual_idx < last_idx:
            errors.append(f"Section {marker_in_order} (at line {current_marker_actual_idx+1}) appears out of expected order relative to {REQUIRED_SECTION_MARKERS[i-1]}.")
        last_idx = current_marker_actual_idx
    
    if errors: return errors

    # --- Section-specific validations ---
    s_segment_ids_ordered: List[str] = []
    s_segment_ids_set: Set[str] = set()

    # II. SimS_Segments::
    content_lines, _ = get_section_content(original_lines, "SimS_Segments::", ALL_POSSIBLE_MARKERS)
    if content_lines is None: errors.append("SimS_Segments:: marker found but content extraction failed (internal helper error).")
    elif not content_lines: errors.append("SimS_Segments:: section is present but has no segment definition lines.")
    else:
        for i, line in enumerate(content_lines):
            match = RE_S_SEGMENT_LINE.match(line)
            if not match:
                errors.append(f"SimS_Segments:: Line {i+1} ('{line[:40]}...') invalid format (expected S<n>(content)).")
                continue
            s_id, s_content = match.groups()
            if not s_content: errors.append(f"SimS_Segments:: Segment {s_id} has no content in parentheses.")
            if s_id in s_segment_ids_set: errors.append(f"SimS_Segments:: Duplicate segment ID: {s_id}")
            s_segment_ids_ordered.append(s_id)
            s_segment_ids_set.add(s_id)
        expected_s_ids = [f"S{j+1}" for j in range(len(s_segment_ids_ordered))]
        if s_segment_ids_ordered != expected_s_ids:
            errors.append(f"SimS_Segments:: IDs not sequential. Found: {s_segment_ids_ordered}, Expected: {expected_s_ids}")

    # III. PHRASE_ALIGN::
    content_lines, _ = get_section_content(original_lines, "PHRASE_ALIGN::", ALL_POSSIBLE_MARKERS)
    if content_lines is None: errors.append("PHRASE_ALIGN:: marker found but content extraction failed.")
    elif not content_lines and s_segment_ids_set: errors.append("PHRASE_ALIGN:: section empty but SimS_Segments exist.")
    elif content_lines:
        if len(content_lines) != len(s_segment_ids_set):
            errors.append(f"PHRASE_ALIGN:: Line count ({len(content_lines)}) differs from SimS_Segments count ({len(s_segment_ids_set)}).")
        for i, line in enumerate(content_lines):
            match = RE_PHRASE_ALIGN_LINE.match(line)
            if not match:
                errors.append(f"PHRASE_ALIGN:: Line {i+1} ('{line[:40]}...') invalid format (expected S<n> ~ span ~ span).")
                continue
            s_id, span1, span2 = match.groups()
            if s_id not in s_segment_ids_set: errors.append(f"PHRASE_ALIGN:: Line {i+1} uses unknown segment ID: {s_id}")
            if not span1.strip(): errors.append(f"PHRASE_ALIGN:: Line {i+1} (ID {s_id}) has empty first span.")
            if not span2.strip(): errors.append(f"PHRASE_ALIGN:: Line {i+1} (ID {s_id}) has empty second span.")

    # IV. SimSL::
    content_lines, _ = get_section_content(original_lines, "SimSL::", ALL_POSSIBLE_MARKERS)
    if content_lines is None: errors.append("SimSL:: marker found but content extraction failed.")
    elif not content_lines and s_segment_ids_set: errors.append("SimSL:: section empty but SimS_Segments exist.")
    elif content_lines:
        if len(content_lines) != len(s_segment_ids_set):
            errors.append(f"SimSL:: Line count ({len(content_lines)}) differs from SimS_Segments count ({len(s_segment_ids_set)}).")
        found_s_ids_in_simsl = set()
        for i, line in enumerate(content_lines):
            match = RE_S_LEMMA_LINE.match(line)
            if not match:
                errors.append(f"SimSL:: Line {i+1} ('{line[:40]}...') invalid format (expected S<n> :: lemmas).")
                continue
            s_id, _ = match.groups() # Lemmas can be empty
            if s_id not in s_segment_ids_set: errors.append(f"SimSL:: Line {i+1} uses unknown segment ID: {s_id}")
            if s_id in found_s_ids_in_simsl: errors.append(f"SimSL:: Duplicate S-ID line for segment: {s_id}")
            found_s_ids_in_simsl.add(s_id)
        for s_id_expected in s_segment_ids_set: # Ensure all segments have a SimSL line
            if s_id_expected not in found_s_ids_in_simsl:
                errors.append(f"SimSL:: Missing S-ID line for segment: {s_id_expected}")


    # V. AdvSL::
    content_lines, _ = get_section_content(original_lines, "AdvSL::", ALL_POSSIBLE_MARKERS)
    if content_lines is None: errors.append("AdvSL:: marker found but content extraction failed.")
    # AdvSL can be empty, e.g. "AdvSL::" or "AdvSL:: \n"
    elif len(content_lines) > 1: errors.append("AdvSL:: section has multiple content lines; expected one logical line of lemmas.")
    # No specific content validation for lemmas themselves here, just structure.

# VI. DIGLOT_MAP::
    content_lines, _ = get_section_content(original_lines, "DIGLOT_MAP::", ALL_POSSIBLE_MARKERS)
    if content_lines is None: 
        errors.append("DIGLOT_MAP:: marker found but content extraction failed.")
    elif not content_lines and s_segment_ids_set: # Check if s_segment_ids_set is non-empty
        errors.append("DIGLOT_MAP:: section empty but SimS_Segments exist (expected S-ID lines for DIGLOT_MAP).")
    elif content_lines: # Only proceed if there are lines to parse for DIGLOT_MAP
        found_s_ids_in_diglot = set()
        for i, line in enumerate(content_lines):
            match_outer = RE_S_LEMMA_LINE.match(line) # Expects S<n> :: entries
            if not match_outer:
                errors.append(f"DIGLOT_MAP:: Line {i+1} ('{line[:40]}...') invalid S-ID header format (expected S<n> :: entries).")
                continue # Skip to next line if header is malformed

            s_id, entries_str = match_outer.groups()

            if s_id not in s_segment_ids_set: # Check against S-IDs from SimS_Segments
                errors.append(f"DIGLOT_MAP:: Line {i+1} (S-ID header '{s_id}') uses unknown segment ID not defined in SimS_Segments.")
            
            if s_id in found_s_ids_in_diglot:
                errors.append(f"DIGLOT_MAP:: Duplicate S-ID line for segment: {s_id}")
            found_s_ids_in_diglot.add(s_id)

            if entries_str.strip(): # Only process if there are entries after "S<n> :: "
                for entry_idx, entry_part_raw in enumerate(entries_str.split('|')):
                    entry_part_stripped = entry_part_raw.strip()
                    if not entry_part_stripped:
                        # Check if this empty part was due to an actual "||" or leading/trailing "|"
                        # A single entry line that is empty after "S<n> :: " is fine (e.g. S1 :: )
                        # but "S1 :: entry1 || entry2" would create an empty part.
                        if entries_str.count('|') > 0 and entry_idx > 0 and entry_idx < entries_str.count('|'):
                             errors.append(f"DIGLOT_MAP:: S-ID {s_id}, found empty entry part (likely due to '||' or trailing/leading '|'). Full entry string: '{entries_str}'")
                        continue # Skip further processing for this genuinely empty part

                    match_entry = RE_DIGLOT_ENTRY.match(entry_part_stripped)
                    if not match_entry:
                        errors.append(f"DIGLOT_MAP:: S-ID {s_id}, entry '{entry_part_stripped[:30]}...' malformed (failed basic regex).")
                    else:
                        eng, spa_lemma, form, viability_char = match_entry.groups()
                        if not eng.strip(): # Check if eng is not just whitespace
                            errors.append(f"DIGLOT_MAP:: S-ID {s_id}, entry '{entry_part_stripped[:30]}...' has empty EngWord.")
                        if not spa_lemma.strip():
                            errors.append(f"DIGLOT_MAP:: S-ID {s_id}, entry '{entry_part_stripped[:30]}...' has empty SpaLemma.")
                        if not form.strip():
                            errors.append(f"DIGLOT_MAP:: S-ID {s_id}, entry '{entry_part_stripped[:30]}...' has empty ExactSpaForm.")
                        if viability_char not in "YN":
                            errors.append(f"DIGLOT_MAP:: S-ID {s_id}, entry '{entry_part_stripped[:30]}...' has invalid ViabilityFlag character: '{viability_char}'. Expected Y or N.")
        
        # After checking all lines in DIGLOT_MAP, ensure all S-IDs from SimS_Segments have a corresponding line
        for s_id_expected in s_segment_ids_set:
            if s_id_expected not in found_s_ids_in_diglot:
                errors.append(f"DIGLOT_MAP:: Missing S-ID line for segment defined in SimS_Segments: {s_id_expected}")


    # VII. LOCKED_PHRASE:: (Optional)
    if "LOCKED_PHRASE::" in marker_indices: # Only validate if marker was found
        content_lines, _ = get_section_content(original_lines, "LOCKED_PHRASE::", ALL_POSSIBLE_MARKERS)
        if content_lines is None: errors.append("LOCKED_PHRASE:: marker found but content extraction failed.")
        elif len(content_lines) > 1: errors.append("LOCKED_PHRASE:: section has multiple content lines; expected one.")
        elif content_lines and content_lines[0].strip(): # If content exists and is not just whitespace
            locked_ids_str = content_lines[0]
            for s_id_locked in locked_ids_str.split():
                if not RE_S_ID_FORMAT.match(s_id_locked): errors.append(f"LOCKED_PHRASE:: Contains non-S<n> formatted ID: '{s_id_locked}'")
                elif s_id_locked not in s_segment_ids_set: errors.append(f"LOCKED_PHRASE:: Uses unknown segment ID: {s_id_locked}")
    
    return errors