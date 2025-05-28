import pytest
from llm_output_validator import validate_llm_block # Import the function to test
import test_llm_block_fixtures as fx # Import the test data fixtures

# --- Helper Function for Assertions (Optional but can make tests cleaner) ---
def assert_validation_contains_error(errors: list[str], expected_error_substring: str, test_case_name: str):
    """Asserts that at least one error message contains the expected substring."""
    assert errors, f"{test_case_name}: Validation should have failed but returned no errors."
    assert any(expected_error_substring.lower() in err.lower() for err in errors), \
        f"{test_case_name}: Expected error substring '{expected_error_substring}' not found in errors: {errors}"

def assert_validation_passes(errors: list[str], test_case_name: str):
    """Asserts that the validation passes (no errors)."""
    assert not errors, f"{test_case_name}: Validation failed unexpectedly. Errors: {errors}"

# --- Test Functions for Good Blocks ---

def test_good_block_minimal_correct():
    errors = validate_llm_block(fx.GOOD_BLOCK_MINIMAL_CORRECT)
    assert_validation_passes(errors, "GOOD_BLOCK_MINIMAL_CORRECT")

def test_good_block_multi_segment_with_locked():
    errors = validate_llm_block(fx.GOOD_BLOCK_MULTI_SEGMENT_WITH_LOCKED)
    assert_validation_passes(errors, "GOOD_BLOCK_MULTI_SEGMENT_WITH_LOCKED")

def test_good_block_empty_sections_valid():
    errors = validate_llm_block(fx.GOOD_BLOCK_EMPTY_SECTIONS_VALID)
    assert_validation_passes(errors, "GOOD_BLOCK_EMPTY_SECTIONS_VALID")

# --- Test Functions for Bad Blocks - Fundamental Structure ---

def test_bad_block_empty():
    errors = validate_llm_block(fx.BAD_BLOCK_EMPTY)
    assert_validation_contains_error(errors, "Block is empty", "BAD_BLOCK_EMPTY")

def test_bad_block_whitespace_only():
    errors = validate_llm_block(fx.BAD_BLOCK_WHITESPACE_ONLY)
    assert_validation_contains_error(errors, "Block is empty", "BAD_BLOCK_WHITESPACE_ONLY")

def test_bad_block_premature_end_sentence():
    errors = validate_llm_block(fx.BAD_BLOCK_PREMATURE_END_SENTENCE)
    assert_validation_contains_error(errors, "premature END_SENTENCE", "BAD_BLOCK_PREMATURE_END_SENTENCE")

def test_bad_block_missing_required_advs():
    errors = validate_llm_block(fx.BAD_BLOCK_MISSING_REQUIRED_ADVS)
    assert_validation_contains_error(errors, "Missing required section marker: AdvS::", "BAD_BLOCK_MISSING_REQUIRED_ADVS")

def test_bad_block_duplicate_section_sims():
    errors = validate_llm_block(fx.BAD_BLOCK_DUPLICATE_SECTION_SIMS)
    assert_validation_contains_error(errors, "Duplicate required section marker: SimS::", "BAD_BLOCK_DUPLICATE_SECTION_SIMS")

def test_bad_block_wrong_order_sims_before_advs():
    errors = validate_llm_block(fx.BAD_BLOCK_WRONG_ORDER_SIMS_BEFORE_ADVS)
    assert_validation_contains_error(errors, "SimS:: (at line 2) appears out of expected order relative to AdvS::", "BAD_BLOCK_WRONG_ORDER_SIMS_BEFORE_ADVS")
    # Or more specifically, if AdvS:: is present later, it might also complain about AdvS order
    # The exact error might depend on how the order check is implemented for multiple out-of-order items.

# --- Test Functions for Bad Blocks - SimS_Segments ---

def test_bad_block_sims_segments_empty_content():
    errors = validate_llm_block(fx.BAD_BLOCK_SIMS_SEGMENTS_EMPTY_CONTENT)
    assert_validation_contains_error(errors, "SimS_Segments:: section is present but has no segment definition lines", "BAD_BLOCK_SIMS_SEGMENTS_EMPTY_CONTENT")

def test_bad_block_sims_segments_malformed_line():
    errors = validate_llm_block(fx.BAD_BLOCK_SIMS_SEGMENTS_MALFORMED_LINE)
    assert_validation_contains_error(errors, "SimS_Segments:: Line 2", "BAD_BLOCK_SIMS_SEGMENTS_MALFORMED_LINE")
    assert_validation_contains_error(errors, "invalid format", "BAD_BLOCK_SIMS_SEGMENTS_MALFORMED_LINE")


def test_bad_block_sims_segments_empty_paren_content():
    errors = validate_llm_block(fx.BAD_BLOCK_SIMS_SEGMENTS_EMPTY_PAREN_CONTENT)
    assert_validation_contains_error(errors, "SimS_Segments:: Segment S1 has no content", "BAD_BLOCK_SIMS_SEGMENTS_EMPTY_PAREN_CONTENT")

def test_bad_block_sims_segments_duplicate_id():
    errors = validate_llm_block(fx.BAD_BLOCK_SIMS_SEGMENTS_DUPLICATE_ID)
    assert_validation_contains_error(errors, "SimS_Segments:: Duplicate segment ID: S1", "BAD_BLOCK_SIMS_SEGMENTS_DUPLICATE_ID")

def test_bad_block_sims_segments_non_sequential_id():
    errors = validate_llm_block(fx.BAD_BLOCK_SIMS_SEGMENTS_NON_SEQUENTIAL_ID)
    assert_validation_contains_error(errors, "SimS_Segments:: IDs not sequential", "BAD_BLOCK_SIMS_SEGMENTS_NON_SEQUENTIAL_ID")

# --- Test Functions for Bad Blocks - PHRASE_ALIGN ---

def test_bad_block_phrase_align_empty_content():
    errors = validate_llm_block(fx.BAD_BLOCK_PHRASE_ALIGN_EMPTY_CONTENT)
    assert_validation_contains_error(errors, "PHRASE_ALIGN:: section empty but SimS_Segments exist", "BAD_BLOCK_PHRASE_ALIGN_EMPTY_CONTENT")

def test_bad_block_phrase_align_malformed_line():
    errors = validate_llm_block(fx.BAD_BLOCK_PHRASE_ALIGN_MALFORMED_LINE)
    assert_validation_contains_error(errors, "PHRASE_ALIGN:: Line 1", "BAD_BLOCK_PHRASE_ALIGN_MALFORMED_LINE")
    assert_validation_contains_error(errors, "invalid format", "BAD_BLOCK_PHRASE_ALIGN_MALFORMED_LINE")

def test_bad_block_phrase_align_unknown_id():
    errors = validate_llm_block(fx.BAD_BLOCK_PHRASE_ALIGN_UNKNOWN_ID)
    assert_validation_contains_error(errors, "PHRASE_ALIGN:: Line 1 uses unknown segment ID: S2", "BAD_BLOCK_PHRASE_ALIGN_UNKNOWN_ID")

def test_bad_block_phrase_align_missing_span():
    errors = validate_llm_block(fx.BAD_BLOCK_PHRASE_ALIGN_MISSING_SPAN)
    assert_validation_contains_error(errors, "PHRASE_ALIGN:: Line 1 (ID S1) has empty first span", "BAD_BLOCK_PHRASE_ALIGN_MISSING_SPAN")

def test_bad_block_phrase_align_line_count_mismatch():
    errors = validate_llm_block(fx.BAD_BLOCK_PHRASE_ALIGN_LINE_COUNT_MISMATCH)
    assert_validation_contains_error(errors, "PHRASE_ALIGN:: Line count (1) differs from SimS_Segments count (2)", "BAD_BLOCK_PHRASE_ALIGN_LINE_COUNT_MISMATCH")

# --- Test Functions for Bad Blocks - SimSL ---

def test_bad_block_simsl_unknown_id():
    errors = validate_llm_block(fx.BAD_BLOCK_SIMSL_UNKNOWN_ID)
    assert_validation_contains_error(errors, "SimSL:: Line 1 uses unknown segment ID: S2", "BAD_BLOCK_SIMSL_UNKNOWN_ID")
    assert_validation_contains_error(errors, "SimSL:: Missing S-ID line for segment: S1", "BAD_BLOCK_SIMSL_UNKNOWN_ID") # Because S1 defined in segments but not SimSL

# --- Test Functions for Bad Blocks - AdvSL ---

def test_bad_block_advsl_multiple_lines():
    errors = validate_llm_block(fx.BAD_BLOCK_ADVSL_MULTIPLE_LINES)
    assert_validation_contains_error(errors, "AdvSL:: section has multiple content lines", "BAD_BLOCK_ADVSL_MULTIPLE_LINES")

# --- Test Functions for Bad Blocks - DIGLOT_MAP ---

def test_bad_block_diglot_missing_s_line_for_segment():
    errors = validate_llm_block(fx.BAD_BLOCK_DIGLOT_MISSING_S_LINE_FOR_SEGMENT)
    assert_validation_contains_error(errors, "DIGLOT_MAP:: Missing S-ID line for segment defined in SimS_Segments: S2", "BAD_BLOCK_DIGLOT_MISSING_S_LINE_FOR_SEGMENT")

def test_bad_block_diglot_malformed_entry_syntax():
    errors = validate_llm_block(fx.BAD_BLOCK_DIGLOT_MALFORMED_ENTRY_SYNTAX)
    assert_validation_contains_error(errors, "DIGLOT_MAP:: S-ID S1, entry 'Eng1->Spa1_Form1_Y...' malformed", "BAD_BLOCK_DIGLOT_MALFORMED_ENTRY_SYNTAX")

def test_bad_block_diglot_empty_engword():
    errors = validate_llm_block(fx.BAD_BLOCK_DIGLOT_EMPTY_ENGWORD)
    assert_validation_contains_error(errors, "entry '->Spa1(Form1)(Y)...' has empty EngWord", "BAD_BLOCK_DIGLOT_EMPTY_ENGWORD")

def test_bad_block_diglot_invalid_viability_flag():
    errors = validate_llm_block(fx.BAD_BLOCK_DIGLOT_INVALID_VIABILITY_FLAG)
    # Match the more specific part of the error message
    assert_validation_contains_error(errors, "has invalid ViabilityFlag character: 'X'. Expected Y or N.", "BAD_BLOCK_DIGLOT_INVALID_VIABILITY_FLAG")

def test_bad_block_diglot_extra_pipe():
    errors = validate_llm_block(fx.BAD_BLOCK_DIGLOT_EXTRA_PIPE)
    # Match the core part of the validator's more detailed message
    assert_validation_contains_error(errors, "found empty entry part (likely due to '||' or trailing/leading '|')", "BAD_BLOCK_DIGLOT_EXTRA_PIPE")
# --- Test Functions for Bad Blocks - LOCKED_PHRASE ---

def test_bad_block_locked_phrase_unknown_id():
    errors = validate_llm_block(fx.BAD_BLOCK_LOCKED_PHRASE_UNKNOWN_ID)
    assert_validation_contains_error(errors, "LOCKED_PHRASE:: Uses unknown segment ID: S5", "BAD_BLOCK_LOCKED_PHRASE_UNKNOWN_ID")

# --- To run this test script:
# 1. Make sure pytest is installed: pip install pytest
# 2. Save this file as test_validator_script.py
# 3. Save llm_output_validator.py and test_llm_block_fixtures.py in the same directory.
# 4. Open your terminal in that directory and run: pytest