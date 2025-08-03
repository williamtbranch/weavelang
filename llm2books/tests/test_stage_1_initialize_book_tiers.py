# llm2books/tests/test_stage_1_initialize_book_tiers.py

import pytest
import json
from pathlib import Path
from unittest.mock import patch, MagicMock
from llm2books.stages.initialize_book_tiers import InitializeBookTiers

# A minimal mock language_config for an en-es run
MOCK_LANG_CONFIG = {
    "base_code": "en", "target_code": "es",
    "base_name": "English", "target_name": "Spanish"
}

@pytest.fixture
def mock_common_resources(fs):
    """A pytest fixture to create a standard set of mocked resources."""
    # Use the fake filesystem 'fs' provided by pyfakefs
    content_project_dir = Path("/fakedir")
    staged_path = content_project_dir / "Staged"
    fs.create_dir(staged_path)
    
    # Create a fake source file
    source_content = "%%lang:en%%\n{S1: This is a test.}"
    fs.create_file(staged_path / "test_book.txt", contents=source_content)

    return {
        'language_config': MOCK_LANG_CONFIG,
        'content_project_dir': str(content_project_dir),
        'source_lang': 'en',
        'source_path': staged_path / "test_book.txt",
        # Mock other resources as needed
        'models_config': {},
        'stages_config': {},
    }

def test_run_source_is_base(mock_common_resources):
    """
    Tests the run() method when the source file language matches the base language.
    It should:
    1. Copy the base text directly.
    2. Make one LLM call to generate the target text.
    3. Produce a correct stage1.json file.
    """
    # ARRANGE
    book_stem = "test_book"
    mock_args = MagicMock() # Mock the command line arguments
    mock_args.output_llm_subdir = "pipeline" 
    mock_args.input_staged_subdir = "Staged" # Good practice to add this too

    # This is our fake LLM response for the en -> es translation
    fake_llm_response = { "S1": "Esto es una prueba." }

    # Use unittest.mock.patch to replace the real LLM call with our fake one
    with patch(
        'llm2books.stages.initialize_book_tiers.InitializeBookTiers._generate_text_via_llm',
        return_value=fake_llm_response
    ) as mock_llm_call:
        
        # ACT
        stage = InitializeBookTiers(book_stem, mock_args, mock_common_resources)
        success = stage.run()

        # ASSERT
        assert success is True

        # Assert that the LLM was called exactly once, and with the right arguments
        mock_llm_call.assert_called_once()
        # Check the second argument of the call, which is the 'to_lang_code'
        assert mock_llm_call.call_args[0][1] == "es"

        # Assert that the output file was created and is valid JSON
        output_path = Path("/fakedir/pipeline/stage1/test_book.stage1.json")
        assert output_path.exists()
        
        with open(output_path, 'r') as f:
            data = json.load(f)
        
        # Assert the content of the generated JSON
        assert data["book_meta"]["base_language"] == "en"
        assert data["book_meta"]["target_language"] == "es"
        
        sentence_block = data["content_blocks"][0]
        assert sentence_block["s_id"] == "S1"
        
        base_tier = sentence_block["tiers"][0]
        target_tier = sentence_block["tiers"][1]
        
        assert base_tier["tier_id"] == "base"
        assert base_tier["full_text"] == "This is a test."
        
        assert target_tier["tier_id"] == "advanced_target"
        assert target_tier["full_text"] == "Esto es una prueba."

def test_run_source_is_target(fs):
    """
    Tests the run() method when the source file is the target language.
    It should copy target text and LLM-generate the base text.
    """
    # ARRANGE
    # Create a new fake filesystem for this specific test case
    content_project_dir = Path("/fakedir")
    staged_path = content_project_dir / "Staged"
    fs.create_dir(staged_path)
    
    # Source file is now Spanish (es)
    source_content = "%%lang:es%%\n{S1: Esto es una prueba.}"
    fs.create_file(staged_path / "test_book_es.txt", contents=source_content)

    mock_resources = {
        'language_config': MOCK_LANG_CONFIG, # Still an en -> es run
        'content_project_dir': str(content_project_dir),
        'source_lang': 'es',
        'source_path': staged_path / "test_book_es.txt",
        'models_config': {}, 'stages_config': {},
    }
    
    book_stem = "test_book_es"
    mock_args = MagicMock()
    mock_args.output_llm_subdir = "pipeline"
    
    # Fake LLM now provides the es -> en translation
    fake_llm_response = { "S1": "This is a test." }

    with patch(
        'llm2books.stages.initialize_book_tiers.InitializeBookTiers._generate_text_via_llm',
        return_value=fake_llm_response
    ) as mock_llm_call:
        
        # ACT
        stage = InitializeBookTiers(book_stem, mock_args, mock_resources)
        success = stage.run()

        # ASSERT
        assert success is True

        # LLM was called once to generate the *base* language
        mock_llm_call.assert_called_once()
        assert mock_llm_call.call_args[0][1] == "en" # Should ask for English

        # Check the output file
        output_path = Path("/fakedir/pipeline/stage1/test_book_es.stage1.json")
        assert output_path.exists()
        with open(output_path, 'r') as f: data = json.load(f)

        base_tier = data["content_blocks"][0]["tiers"][0]
        target_tier = data["content_blocks"][0]["tiers"][1]
        
        assert base_tier["full_text"] == "This is a test." # From LLM
        assert target_tier["full_text"] == "Esto es una prueba." # Copied from source

def test_run_source_is_neither(fs):
    """
    Tests the run() method when the source is a third language.
    It should make two LLM calls (one for base, one for target).
    """
    # ARRANGE
    content_project_dir = Path("/fakedir")
    staged_path = content_project_dir / "Staged"
    fs.create_dir(staged_path)
    
    # Source file is now Italian (it)
    source_content = "%%lang:it%%\n{S1: Questo e un test.}"
    fs.create_file(staged_path / "test_book_it.txt", contents=source_content)

    # We need a manifest that includes Italian for this to work
    mock_lang_config_with_it = {
        "base_code": "en", "target_code": "es",
        "base_name": "English", "target_name": "Spanish",
        # This test doesn't use the name 'Italian', so it's not strictly needed here
    }

    mock_resources = {
        'language_config': mock_lang_config_with_it, # Still an en -> es run
        'content_project_dir': str(content_project_dir),
        'source_lang': 'it',
        'source_path': staged_path / "test_book_it.txt",
        'models_config': {}, 'stages_config': {},
    }
    
    book_stem = "test_book_it"
    mock_args = MagicMock()
    mock_args.output_llm_subdir = "pipeline"
    
    # We need to mock two different return values for the two LLM calls.
    # The first call will be for base (it->en), second for target (it->es).
    mock_llm_call = MagicMock(side_effect=[
        { "S1": "This is a test." },    # First call returns English
        { "S1": "Esto es una prueba." } # Second call returns Spanish
    ])

    with patch(
        'llm2books.stages.initialize_book_tiers.InitializeBookTiers._generate_text_via_llm',
        mock_llm_call # Use our side_effect mock
    ):
        # ACT
        stage = InitializeBookTiers(book_stem, mock_args, mock_resources)
        success = stage.run()

        # ASSERT
        assert success is True
        
        # Assert that the LLM was called twice
        assert mock_llm_call.call_count == 2
        
        # Check the arguments of each call
        # Call 1: generate base ('en')
        assert mock_llm_call.call_args_list[0][0][1] == "en"
        # Call 2: generate target ('es')
        assert mock_llm_call.call_args_list[1][0][1] == "es"

        # Check the final output file
        output_path = Path("/fakedir/pipeline/stage1/test_book_it.stage1.json")
        assert output_path.exists()
        with open(output_path, 'r') as f: data = json.load(f)
        
        base_tier = data["content_blocks"][0]["tiers"][0]
        target_tier = data["content_blocks"][0]["tiers"][1]
        
        assert base_tier["full_text"] == "This is a test."
        assert target_tier["full_text"] == "Esto es una prueba."
