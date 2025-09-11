import pytest
import json
from pathlib import Path
from llm2books.stages import AssembleTiers

# A helper function to create minimal mock pool files
def create_mock_pool_file(path: Path, tier_name: str, s_id: str):
    content = {
        "meta": {"tier_type": tier_name},
        "content": [{
            "block_type": "sentence",
            "s_id": s_id,
            "full_text": f"This is the text for {tier_name}."
        }]
    }
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(content, f)

def test_assemble_tiers_creates_five_tiers_in_correct_order(tmp_path):
    """
    Tests that the AssembleTiers stage correctly loads all five
    source files and assembles them into a sentence block with a 5-element
    tiers array in the specified order.
    """
    # ARRANGE
    # Create a set of mock pool files in a temporary directory
    mock_s_id = "S1"
    pool_paths = {
        "base_std": tmp_path / "book.en.std.json",
        "target_std": tmp_path / "book.es.std.json",
        "target_mod": tmp_path / "book.es.mod.json",
        "target_bas": tmp_path / "book.es.bas.json",
        "target_sim": tmp_path / "book.es.sim.json",
    }
    create_mock_pool_file(pool_paths["base_std"], "std", mock_s_id)
    create_mock_pool_file(pool_paths["target_std"], "std", mock_s_id)
    create_mock_pool_file(pool_paths["target_mod"], "mod", mock_s_id)
    create_mock_pool_file(pool_paths["target_bas"], "bas", mock_s_id)
    create_mock_pool_file(pool_paths["target_sim"], "sim", mock_s_id)

    # Instantiate the stage with mock resources
    mock_resources = {
        "language_config": {"base_code": "en", "target_code": "es"},
        "content_project_dir": "dummy/path",
        "book_resources": pool_paths,
    }
    stage = AssembleTiers("book", None, mock_resources)

    # ACT
    # Directly call the processing logic of the stage
    processed_data = stage._process_data(pool_paths)

    # ASSERT
    assert processed_data is not None, "Processing should succeed"
    assert "content_blocks" in processed_data
    assert len(processed_data["content_blocks"]) == 1, "Should process one sentence block"
    
    sentence_block = processed_data["content_blocks"][0]
    assert "tiers" in sentence_block
    assert len(sentence_block["tiers"]) == 5, "There should be exactly five tiers"

    # Assert the correct order and naming of the tiers
    actual_tier_ids = [t["tier_id"] for t in sentence_block["tiers"]]
    expected_tier_ids = [
        "base",
        "advanced_target",
        "moderate_target",
        "basic_target",
        "simple_target",
    ]
    assert actual_tier_ids == expected_tier_ids, "The tiers are not in the correct order"