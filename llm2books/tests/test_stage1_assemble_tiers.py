# llm2books/tests/test_stage1_assemble_tiers.py
import pytest
import json
from pathlib import Path
from llm2books.stages import AssembleTiers

# --- THIS IS THE FIX ---
# Update the helper to create more realistic mock data, including empty lists for lemmas/segments.
def create_mock_pool_file(path: Path, tier_name: str, s_id: str):
    content = {
        "meta": {"tier_type": tier_name},
        "content": [{
            "block_type": "sentence",
            "s_id": s_id,
            "full_text": f"This is the text for {tier_name}.",
            "lemmas": [f"lemma_{tier_name}"], # Add mock lemma
            "segments": [{"seg_id": "S1", "text": f"This is the text for {tier_name}."}] # Add mock segment
        }]
    }
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(content, f)

def test_assemble_tiers_creates_initial_tiers_correctly(tmp_path):
    """
    V11 Test: Tests that AssembleTiers correctly loads the two foundational
    pool files and creates the initial three tiers (base, advanced_target, moderate_target).
    """
    # ARRANGE
    mock_s_id = "S1"
    # The test now only needs two mock files.
    pool_paths = {
        "base_std": tmp_path / "book.en.std.json",
        "target_std": tmp_path / "book.es.std.json",
    }
    create_mock_pool_file(pool_paths["base_std"], "std_en", mock_s_id)
    create_mock_pool_file(pool_paths["target_std"], "std_es", mock_s_id)

    mock_resources = {
        "language_config": {"base_code": "en", "target_code": "es"},
        "content_project_dir": "dummy/path",
        "book_resources": pool_paths,
    }
    stage = AssembleTiers("book", None, mock_resources)

    # ACT
    processed_data = stage._process_data(pool_paths)

    # ASSERT
    assert processed_data is not None
    assert "content_blocks" in processed_data
    assert len(processed_data["content_blocks"]) == 1
    
    sentence_block = processed_data["content_blocks"][0]
    assert "tiers" in sentence_block
    
    # --- THIS IS THE FIX ---
    # The stage should now create exactly THREE tiers.
    assert len(sentence_block["tiers"]) == 3, "AssembleTiers should create three initial tiers"

    actual_tier_ids = [t["tier_id"] for t in sentence_block["tiers"]]
    expected_tier_ids = [
        "base",
        "advanced_target",
        "moderate_target", # This is a placeholder copy of advanced_target
    ]
    assert actual_tier_ids == expected_tier_ids, "The initial tiers are not correct or in the right order"

    # Verify the placeholder moderate tier was created correctly
    adv_tier = next(t for t in sentence_block["tiers"] if t["tier_id"] == "advanced_target")
    mod_tier = next(t for t in sentence_block["tiers"] if t["tier_id"] == "moderate_target")
    assert mod_tier["full_text"] == adv_tier["full_text"]
    assert mod_tier["lemmas"] == adv_tier["lemmas"]