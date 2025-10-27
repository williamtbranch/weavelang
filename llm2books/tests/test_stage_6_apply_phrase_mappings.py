# llm2books/tests/test_stage_6_apply_phrase_mappings.py
import pytest
from llm2books.stages import ApplyPhraseMappings

@pytest.fixture
def mock_input_data_for_fusion():
    """
    Provides a mock sentence block specifically designed to trigger the
    'di' re-indexing bug. It simulates the phrase "...in the garden..." where
    "in the" will be fused into a single group.
    """
    return {
        "content_blocks": [
            {
                "block_type": "sentence",
                "s_id": "S_TEST",
                "tiers": [
                    {
                        "tier_id": "basic_base",
                        "segments": [
                            {
                                "seg_id": "S1",
                                "tokenized_text": [
                                    {"t": "b", "v": ""},
                                    {"t": "w", "v": "a", "di": 0},
                                    {"t": "b", "v": " "},
                                    {"t": "w", "v": "tree", "di": 1},
                                    {"t": "b", "v": " "},
                                    # This is the key part that will be fused
                                    {"t": "w", "v": "in", "di": 2},
                                    {"t": "b", "v": " "},
                                    {"t": "w", "v": "the", "di": 3},
                                    # End of fusion part
                                    {"t": "b", "v": " "},
                                    {"t": "w", "v": "garden", "di": 4},
                                    {"t": "b", "v": "."},
                                ]
                            }
                        ]
                    }
                ]
            }
        ]
    }

def test_apply_phrase_mappings_reindexes_di_after_fusion(mock_input_data_for_fusion, spacy_es_model):
    """
    This test asserts that after the `ApplyPhraseMappings` stage fuses tokens
    (like "in" and "the"), it correctly re-assigns the `di` values in both the
    token stream and the diglot map to be perfectly sequential.
    """
    # ARRANGE
    mock_resources = {
        "spacy_models": {"es": spacy_es_model}, # Needed by the stage's constructor
        "language_config": {"target_code": "es"},
        "content_project_dir": "dummy/path",
    }
    stage = ApplyPhraseMappings("book", None, mock_resources)

    # This map will cause "in the" to be fused into a single word group.
    approved_map = {
        "S_TEST": [
            "a -> un",
            "tree -> árbol",
            "in the -> en el",
            "garden -> jardín",
        ]
    }

    # ACT
    processed_data = stage._process_data(mock_input_data_for_fusion, approved_map)

    # ASSERT
    processed_block = processed_data["content_blocks"][0]
    base_tier = next(t for t in processed_block["tiers"] if t["tier_id"] == "basic_base")
    word_tokens = [t for t in base_tier["segments"][0]["tokenized_text"] if t["t"] == "w"]
    
    # 1. THE CORE ASSERTION: Check the `di` sequence in the tokenized_text
    actual_di_values_in_tokens = [t['di'] for t in word_tokens]
    expected_di_values = list(range(len(word_tokens))) # e.g., [0, 1, 2, 3]

    assert actual_di_values_in_tokens == expected_di_values, \
        f"The 'di' values in the processed tokens are not sequential. Got {actual_di_values_in_tokens}"

    # 2. SECONDARY ASSERTION: Check the `di` sequence in the generated mapping
    diglot_map = processed_block["mappings"]["basic_spanish_to_basic_english_diglot"]["S1"]
    # VVV THIS IS THE CORRECTED LINE VVV
    actual_di_values_in_map = [entry[0] for entry in diglot_map] # entry[0] is the di

    assert actual_di_values_in_map == expected_di_values, \
        f"The 'di' values in the diglot map are not sequential. Got {actual_di_values_in_map}"