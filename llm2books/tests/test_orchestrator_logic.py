# llm2books/tests/test_orchestrator_logic.py

import pytest
from pathlib import Path

# We are writing tests for functions that DON'T EXIST YET.
# This is the core of TDD. We will import them once we create them.
from llm2books.orchestrate_pipeline import build_language_config
from llm2books.llm_prompts import load_prompt_template

# A mock manifest dictionary that mirrors the structure of languages.toml
# This will be our single source of truth for these tests.
MOCK_MANIFEST = {
    "en": {
        "name": "English",
        "spacy_model": "en_core_web_lg",
        "normalization_function": "default_latin",
    },
    "es": {
        "name": "Spanish",
        "spacy_model": "es_core_news_lg",
        "normalization_function": "spanish_latin_unaccented",
    },
    "ja": {
        "name": "Japanese",
        "spacy_model": "ja_core_news_lg",
        "normalization_function": "japanese_dictionary_form",
    },
    "pair": {
        "en-es": {"prompt_dir": "prompts/en-es"},
        "es-en": {"prompt_dir": "prompts/es-en"},
        # Note: ja-en is intentionally omitted to test the fallback mechanism
    },
}

# --- Test Suite for Language Config Builder ---

class TestBuildLanguageConfig:
    def test_build_config_raises_error_for_missing_language(self):
        """
        Tests that a ValueError is raised if a requested language is not in the manifest.
        """
        with pytest.raises(ValueError) as excinfo:
            build_language_config(MOCK_MANIFEST, "en", "fr") # 'fr' is not in our mock
        
        assert "'fr'" in str(excinfo.value)
        assert "target language" in str(excinfo.value)

    def test_build_config_handles_missing_pair_gracefully(self):
        """
        Tests that a missing [pair] entry results in a None prompt_dir, not an error.
        """
        # 'ja' and 'en' exist, but 'ja-en' is not in the [pair] section of the mock
        lang_config = build_language_config(MOCK_MANIFEST, "ja", "en")
        
        assert lang_config["base_code"] == "ja"
        assert lang_config["target_code"] == "en"
        assert lang_config["pair_prompt_dir"] is None

    def test_build_config_reverse_path_es_en(self):
        """
        Tests that a reverse language pair is assembled correctly.
        """
        # ACT
        lang_config = build_language_config(MOCK_MANIFEST, "es", "en")
        
        # ASSERT: Check that 'base' and 'target' are correctly swapped
        assert lang_config["base_code"] == "es"
        assert lang_config["target_code"] == "en"
        assert lang_config["base_name"] == "Spanish"
        assert lang_config["target_name"] == "English"
        assert lang_config["base_spacy_model"] == "es_core_news_lg"
        assert lang_config["target_spacy_model"] == "en_core_web_lg"
        assert lang_config["pair_prompt_dir"] == "prompts/es-en"

    def test_build_config_happy_path_en_es(self):
        """
        Tests that a standard language pair is assembled correctly.
        """
        # ARRANGE: We have our mock manifest.
        
        # ACT: Call the function we are testing (which doesn't exist yet).
        lang_config = build_language_config(MOCK_MANIFEST, "en", "es")
        
        # ASSERT: Check that every piece of the config is correct.
        assert lang_config["base_code"] == "en"
        assert lang_config["target_code"] == "es"
        assert lang_config["base_name"] == "English"
        assert lang_config["target_name"] == "Spanish"
        assert lang_config["base_spacy_model"] == "en_core_web_lg"
        assert lang_config["target_spacy_model"] == "es_core_news_lg"
        assert lang_config["pair_prompt_dir"] == "prompts/en-es"
    # Test cases will go here
    pass

# --- Test Suite for Prompt Loader ---

class TestLoadPromptTemplate:

    def test_loads_specific_prompt_when_it_exists(self, fs):
        """
        Tests that the loader correctly finds and uses a language-pair specific
        override prompt.
        
        The 'fs' argument is a pytest fixture that provides a fake file system.
        """
        # ARRANGE: Create the fake files and directories needed for this test.
        base_asset_path = Path("/weavelang/assets")
        
        fs.create_file(
            base_asset_path / "prompts/_defaults/stage1_translate.txt",
            contents="This is the default prompt."
        )
        
        fs.create_file(
            base_asset_path / "prompts/en-ja/stage1_translate.txt",
            contents="This is the specific en-ja override prompt."
        )
        fs.create_dir(base_asset_path / "prompts/en-es")
        
        # ACT: Call the function we are testing (which doesn't exist yet).
        # We need to design the function signature. Let's make it simple.
        # It takes the filename, the root asset path, and the specific pair directory.
        prompt_content = load_prompt_template(
            prompt_name="stage1_translate.txt",
            base_asset_path=base_asset_path,
            pair_prompt_dir="prompts/en-ja" # This pair has a specific file
        )
        
        # ASSERT
        assert prompt_content == "This is the specific en-ja override prompt."

    def test_falls_back_to_default_when_specific_prompt_missing(self, fs):
        """
        Tests that the loader correctly falls back to the default prompt when
        a language-pair specific override does not exist.
        """
        # ARRANGE: Set up the fake file system.
        base_asset_path = Path("/weavelang/assets")
        fs.create_file(
            base_asset_path / "prompts/_defaults/stage1_translate.txt",
            contents="This is the default prompt."
        )
        # Note: We do NOT create a file in the en-es directory.
        fs.create_dir(base_asset_path / "prompts/en-es")
        
        # ACT: Call the function for the en-es pair, which has no override.
        prompt_content = load_prompt_template(
            prompt_name="stage1_translate.txt",
            base_asset_path=base_asset_path,
            pair_prompt_dir="prompts/en-es"
        )
        
        # ASSERT
        assert prompt_content == "This is the default prompt."