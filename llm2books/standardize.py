# llm2books/standardize.py
from typing import List, Dict, Any, Tuple
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