import pytest
from llm2books.stages.process_target_tiers import ProcessTargetTiers
from llm2books.validator import ValidationError

@pytest.fixture
def mock_stage1_output():
    """Provides a mock data block as it would come from AssembleTiers."""
    return {
        "block_type": "sentence",
        "s_id": "S1",
        "tiers": [
            {"tier_id": "base", "full_text": "Base text.", "segments": [{"text": "Base text."}]},
            {"tier_id": "advanced_target", "full_text": "Texto avanzado.", "segments": [{"text": "Texto avanzado."}]},
            {"tier_id": "moderate_target", "full_text": "Texto moderado.", "segments": [{"text": "Texto moderado."}]},
            {"tier_id": "basic_target", "full_text": "Texto básico.", "segments": [{"text": "Texto básico."}]},
            {"tier_id": "simple_target", "full_text": "Texto simple.", "segments": [{"text": "Texto simple."}]},
        ]
    }

def test_process_target_tiers_populates_all_four_tiers(mock_stage1_output, spacy_es_model):
    """
    Tests that the new stage correctly processes all four target-language tiers,
    adding tokenized_text and lemmas to each one.
    """
    # ARRANGE
    mock_resources = {
        "language_config": {"target_code": "es"},
        "spacy_models": {"es": spacy_es_model},
        "content_project_dir": "dummy/path",
    }
    stage = ProcessTargetTiers("test_book", None, mock_resources)
    mock_data = {"content_blocks": [mock_stage1_output]}

    # ACT
    processed_data = stage._process_data(mock_data)
    processed_block = processed_data["content_blocks"][0]

    # ASSERT
    target_tier_ids = ["advanced_target", "moderate_target", "basic_target", "simple_target"]
    for tier_id in target_tier_ids:
        tier = next((t for t in processed_block["tiers"] if t["tier_id"] == tier_id), None)
        assert tier is not None, f"Tier '{tier_id}' should exist."
        
        # Check for lemmatization
        assert "lemmas" in tier
        assert len(tier["lemmas"]) > 0, f"Lemmas should be populated for tier '{tier_id}'."
        assert "texto" in tier["lemmas"]

        # Check for tokenization in segments
        assert "segments" in tier
        assert len(tier["segments"]) > 0, f"Segments should exist for tier '{tier_id}'."
        segment = tier["segments"][0]
        assert "tokenized_text" in segment
        assert len(segment["tokenized_text"]) > 0, f"tokenized_text should be populated for tier '{tier_id}'."
        
        # Check that word tokens have lemmas
        word_token = next((tok for tok in segment["tokenized_text"] if tok["t"] == "w"), None)
        assert word_token is not None, f"Word token should exist for tier '{tier_id}'."
        assert "l" in word_token and len(word_token["l"]) > 0, f"Word tokens should have lemmas in tier '{tier_id}'."

    # Check that the base tier was NOT processed
    base_tier = next((t for t in processed_block["tiers"] if t["tier_id"] == "base"), None)
    assert "lemmas" not in base_tier, "Base tier should not have lemmas."
    assert "tokenized_text" not in base_tier["segments"][0], "Base tier should not be tokenized by this stage."