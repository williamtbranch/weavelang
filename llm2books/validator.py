# llm2books/validator.py

from typing import Dict, Any

# --- Custom Exception Class ---
class ValidationError(Exception):
    """Custom exception for data integrity failures in the JSON."""
    pass

# --- Validation Functions ---

def validate_full_text_reconstruction(tier: Dict[str, Any]):
    """
    Validates the "Lossless Reconstruction" rule for a single tier.
    
    The text formed by concatenating the 'v' (value) of all tokens from all
    segments within a tier MUST exactly equal the 'full_text' for that tier.

    Args:
        tier: A dictionary representing a single tier from the JSON.

    Raises:
        ValidationError: If the reconstructed text does not match the full_text.
    """
    tier_id = tier.get("tier_id", "Unknown")
    full_text = tier.get("full_text", "")
    
    reconstructed_parts = []
    for segment in tier.get("segments", []):
        for token in segment.get("tokenized_text", []):
            reconstructed_parts.append(token.get("v", ""))
            
    reconstructed_text = "".join(reconstructed_parts)
    
    if reconstructed_text != full_text:
        raise ValidationError(
            f"Lossless reconstruction failed for tier '{tier_id}'.\n"
            f"  Expected: '{full_text}'\n"
            f"  Got:      '{reconstructed_text}'"
        )

def validate_bwbw_invariant(token_list: list, tier_id: str, seg_id: str):
    """
    Validates the BWBWB Invariant for a tokenized_text array.

    The array must start and end with a background token ('b') and must
    alternate between 'b' and word ('w') tokens.

    Args:
        token_list: The list of token dictionaries to validate.
        tier_id: The ID of the tier for clear error messages.
        seg_id: The ID of the segment for clear error messages.

    Raises:
        ValidationError: If the BWBWB invariant is violated.
    """
    if not token_list:
        # An empty or null list is technically a violation as it doesn't
        # start and end with a 'b' token. A valid empty segment
        # should have at least one token: [{"t": "b", "v": ""}]
        raise ValidationError(
            f"Token list for {tier_id}.{seg_id} is empty."
        )

    # Check start and end tokens
    if token_list[0].get("t") != "b":
        raise ValidationError(
            f"BWBWB Invariant Violation in {tier_id}.{seg_id}: "
            f"Token list must start with a background ('b') token."
        )
    if token_list[-1].get("t") != "b":
        raise ValidationError(
            f"BWBWB Invariant Violation in {tier_id}.{seg_id}: "
            f"Token list must end with a background ('b') token."
        )

    # Check for consecutive tokens of the same type
    for i in range(len(token_list) - 1):
        current_token_type = token_list[i].get("t")
        next_token_type = token_list[i+1].get("t")
        if current_token_type == next_token_type:
            raise ValidationError(
                f"BWBWB Invariant Violation in {tier_id}.{seg_id}: "
                f"Found consecutive tokens of the same type ('{current_token_type}')."
            )

def validate_base_tier_diglot_indices(tier: Dict[str, Any]):
    """
    Validates two rules for a 'base' tier:
    1. All word tokens must have a 'di' key.
    2. 'di' values must be unique and sequential across the entire tier (all segments).
    
    Args:
        tier: A dictionary representing a single 'base' tier from the JSON.

    Raises:
        ValidationError: If any of the 'di' rules are violated.
    """
    tier_id = tier.get("tier_id", "Unknown")
    if tier_id != "base":
        # This validation only applies to the base tier.
        return

    all_di_values = []
    
    # First, collect all 'di' values from all segments
    for seg_idx, segment in enumerate(tier.get("segments", [])):
        for token_idx, token in enumerate(segment.get("tokenized_text", [])):
            if token.get("t") == "w":
                if "di" not in token:
                    raise ValidationError(
                        f"Validation failed for {tier_id}.segment[{seg_idx}].token[{token_idx}]: "
                        f"Word token '{token.get('v')}' is missing 'di' key."
                    )
                all_di_values.append(token["di"])

    # Now, validate the collected list of 'di' values
    
    # Rule 2a: Check for duplicates
    if len(all_di_values) != len(set(all_di_values)):
        # Find the first duplicate to make the error message helpful
        seen = set()
        for di in all_di_values:
            if di in seen:
                raise ValidationError(
                    f"Validation failed for {tier_id}: Duplicate 'di' value found: {di}."
                )
            seen.add(di)
            
    # Rule 2b: Check for sequentiality
    sorted_dis = sorted(all_di_values)
    for i, di in enumerate(sorted_dis):
        if i != di:
            raise ValidationError(
                f"Validation failed for {tier_id}: 'di' sequence was not sequential. "
                f"Expected {i}, but got {di}."
            )

def validate_exhaustive_diglot_mapping(sentence_block: Dict[str, Any]):
    """
    Validates that for every 'base' segment, the number of word tokens
    matches the number of entries in the simple_target_to_base_diglot map.

    Args:
        sentence_block: The dict for a single sentence block from the JSON.
    
    Raises:
        ValidationError: If the counts do not match for any segment.
    """
    s_id = sentence_block.get("s_id", "UnknownSentence")
    base_tier = next((t for t in sentence_block.get("tiers", []) if t.get("tier_id") == "base"), None)
    
    if not base_tier:
        return # Cannot validate if there is no base tier

    diglot_map = sentence_block.get("mappings", {}).get("simple_target_to_base_diglot", {})

    for segment in base_tier.get("segments", []):
        seg_id = segment.get("seg_id")
        if not seg_id:
            continue # Skip segments without an ID

        # Count the number of word tokens in this segment
        word_token_count = sum(1 for token in segment.get("tokenized_text", []) if token.get("t") == "w")
        
        # If a mapping exists for this segment, check its length
        if seg_id in diglot_map:
            mapping_entry_count = len(diglot_map[seg_id])
            
            if word_token_count != mapping_entry_count:
                raise ValidationError(
                    f"Exhaustive Diglot Mapping failed for {s_id}.{seg_id}: "
                    f"Mismatch between word count and mapping entry count. "
                    f"Expected {word_token_count} mapping entries, but found {mapping_entry_count}."
                )
def validate_exhaustive_inverse_diglot_mapping(sentence_block: Dict[str, Any]):
    """
    Validates that for every 'simpler_advanced_target' segment, the number
    of word tokens matches the number of entries in the inverse diglot map.

    Args:
        sentence_block: The dict for a single sentence block from the JSON.
    
    Raises:
        ValidationError: If the counts do not match for any segment.
    """
    s_id = sentence_block.get("s_id", "UnknownSentence")
    simpler_adv_tier = next((
        t for t in sentence_block.get("tiers", []) 
        if t.get("tier_id") == "simpler_advanced_target"
    ), None)
    
    if not simpler_adv_tier:
        return # Cannot validate if the tier does not exist

    inv_diglot_map = sentence_block.get("mappings", {}).get("simpler_adv_target_to_base_inv_diglot", {})

    for segment in simpler_adv_tier.get("segments", []):
        seg_id = segment.get("seg_id")
        if not seg_id:
            continue

        word_token_count = sum(1 for token in segment.get("tokenized_text", []) if token.get("t") == "w")
        
        if seg_id in inv_diglot_map:
            mapping_entry_count = len(inv_diglot_map[seg_id])
            
            if word_token_count != mapping_entry_count:
                raise ValidationError(
                    f"Exhaustive Inverse Diglot Mapping failed for {s_id}.{seg_id}: "
                    f"Mismatch between word count and mapping entry count. "
                    f"Expected {word_token_count} mapping entries, but found {mapping_entry_count}."
                )