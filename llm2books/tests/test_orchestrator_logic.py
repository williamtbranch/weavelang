# llm2books/tests/test_orchestrator_logic.py

import pytest
from pathlib import Path

# We are writing tests for functions that DON'T EXIST YET.
# This is the core of TDD. We will import them once we create them.
from llm2books.orchestrate_pipeline import build_language_config

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
