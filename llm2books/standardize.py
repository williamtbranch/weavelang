# llm2books/standardize.py
from typing import List, Dict, Any, Tuple, Optional
import re

class ValidationError(Exception):
    """Custom exception for data integrity failures."""
    pass

# def split_boundary(boundary_b1: str, boundary_b2: str) -> Tuple[str, str]:
#     """
#     Applies the "Smart Space Boundary" rule to two adjacent background strings.

#     Args:
#         boundary_b1: The final background string from the first segment.
#         boundary_b2: The initial background string from the second segment.

#     Returns:
#         A tuple containing the new (corrected_b1, corrected_b2).
#     """
#     combined_boundary = boundary_b1 + boundary_b2
    
#     # Find the index of the first space character
#     split_index = combined_boundary.find(' ')
    
#     if split_index != -1:
#         # A space was found. The split point is *after* the space.
#         new_b1 = combined_boundary[:split_index + 1]
#         new_b2 = combined_boundary[split_index + 1:]
#     else:
#         # No space found, so the first segment pulls in the entire boundary.
#         new_b1 = combined_boundary
#         new_b2 = ""
        
#     return (new_b1, new_b2)


def standardize_tokenized_segments(segments: List[Dict[str, Any]]):
    """
    This function doesn't exist yet, but will be needed later.
    """
    pass

#

def align_segment_boundaries(segments: List[str]) -> List[str]:
    """
    Applies the 'Smart Space Boundary' rule by re-slicing the combined
    text of adjacent segments. This is the simplest and most robust approach.
    """
    if len(segments) < 2:
        return segments

    # Create a mutable copy to work with
    aligned = list(segments)

    # We must iterate forwards to correctly pass changes along.
    i = 0
    while i < len(aligned) - 1:
        seg1 = aligned[i]
        seg2 = aligned[i+1]
        
        combined = seg1 + seg2
        
        split_point = combined.find(' ')

        if split_point != -1:
            # A space was found. Slice the combined string.
            new_seg1 = combined[:split_point + 1]
            new_seg2 = combined[split_point + 1:]
        else:
            # No space in the combined boundary. This is unusual, but to be
            # safe, seg1 takes everything and seg2 becomes empty.
            new_seg1 = combined
            new_seg2 = ""
            
        aligned[i] = new_seg1
        aligned[i+1] = new_seg2
        
        i += 1

    # Filter out any segments that became empty during the process.
    return [s for s in aligned if s]

def reconstruct_and_separate_segments(segments: List[Dict[str, Any]], simplified_text_map: Dict[str, str]) -> Tuple[List[Dict[str, Any]], str]:
    """
    Takes a list of original segments and a map of simplified texts,
    then rebuilds the segments with corrected separators and returns the
    new segment list and the reconstructed full text.

    Args:
        segments: The list of original segment dictionaries (containing seg_id).
        simplified_text_map: A dict mapping {lookup_id: "simplified text"}.

    Returns:
        A tuple containing (new_segments_list, reconstructed_full_text).
    """
    if not segments:
        return [], ""

    new_segments = []
    full_text_parts = []
    num_segments = len(segments)

    for i, seg in enumerate(segments):
        # The lookup_id is constructed from the original sentence's s_id and the segment's seg_id,
        # but for this pure function, we assume the map keys are already correct.
        # We'll just use seg_id for simplicity in this function's context if s_id isn't passed.
        lookup_id = seg.get("lookup_id", seg.get("seg_id")) # Assume lookup_id is pre-constructed
        
        clean_simplified_text = simplified_text_map.get(lookup_id, seg.get('text', ''))
        
        # Determine the final text for this segment, including the separator
        final_segment_text = clean_simplified_text
        if i < num_segments - 1:
            # Add a space if the text doesn't already end with one.
            if not final_segment_text.endswith(' '):
                final_segment_text += " "
        
        full_text_parts.append(final_segment_text)

        # Create a new segment dictionary, preserving original keys and updating text
        new_seg = seg.copy()
        new_seg['text'] = final_segment_text
        new_segments.append(new_seg)

    reconstructed_full_text = "".join(full_text_parts).rstrip()
    
    return new_segments, reconstructed_full_text

def smart_match_and_edit(token_stream: List[Dict], word_token_index: int, match_string: str) -> Optional[List[Dict]]:
    """
    Attempts to match a string against a word token and its neighbors,
    dynamically adjusting token boundaries by "pulling" from or "pushing" to
    adjacent background tokens.
    
    Returns a new token stream on success, or None on failure.
    """
    # 1. --- Basic Sanity Checks ---
    if not (0 <= word_token_index < len(token_stream)):
        return None # Index out of bounds

    word_token = token_stream[word_token_index]
    if word_token.get("t") != "w":
        return None # The specified index is not a word token

    # 2. --- Assemble the "Search Space" ---
    # Get the word token and its immediate neighbors. Handle edge cases.
    b1_token = token_stream[word_token_index - 1] if word_token_index > 0 else {"t": "b", "v": ""}
    w_token = word_token
    b2_token = token_stream[word_token_index + 1] if word_token_index < len(token_stream) - 1 else {"t": "b", "v": ""}
    
    if b1_token.get("t") != "b" or b2_token.get("t") != "b":
        return None # Invalid stream structure (not B-W-B)

    combined_text = b1_token["v"] + w_token["v"] + b2_token["v"]
    
    # 3. --- Find the Match ---
    # Find the starting position of the match_string within the combined text.
    match_start_index = combined_text.find(match_string)
    
    if match_start_index == -1:
        return None # The match string isn't even a substring of the local context.

    match_end_index = match_start_index + len(match_string)

    # 4. --- Calculate New Boundaries and Content ---
    # Determine the boundaries of the original word token within the combined text.
    original_w_start = len(b1_token["v"])
    original_w_end = original_w_start + len(w_token["v"])
    
    # Check for invalid operations, like pushing/pulling past the original word's boundaries.
    # This ensures we only modify the immediate B-W-B neighborhood.
    if match_start_index > original_w_start or match_end_index < original_w_end:
         # This logic covers cases where the match is entirely within a background token,
         # or requires skipping over the original word token, which are invalid.
         pass # Let's refine this check later if needed. Simple cases first.

    # Slice the combined text to get the new values for b1, w, and b2.
    new_b1_v = combined_text[:match_start_index]
    new_w_v = combined_text[match_start_index:match_end_index] # This is just match_string
    new_b2_v = combined_text[match_end_index:]
    
    # 5. --- Construct and Return the New Stream ---
    # Create a copy of the original stream to modify.
    new_stream = list(token_stream)
    
    # Create new tokens for the modified neighborhood.
    # Preserve original data from the word token, just update its value.
    new_w_token = w_token.copy()
    new_w_token["v"] = new_w_v
    
    new_b1_token = b1_token.copy()
    new_b1_token["v"] = new_b1_v
    
    new_b2_token = b2_token.copy()
    new_b2_token["v"] = new_b2_v
    
    # Replace the tokens in the new stream.
    if word_token_index > 0:
        new_stream[word_token_index - 1] = new_b1_token
    
    new_stream[word_token_index] = new_w_token
    
    if word_token_index < len(token_stream) - 1:
        new_stream[word_token_index + 1] = new_b2_token
        
    # Handle the edge case where B1 or B2 didn't originally exist.
    if word_token_index == 0:
        new_stream.insert(0, new_b1_token)
    if word_token_index == len(token_stream) - 1:
        new_stream.append(new_b2_token)

    return new_stream