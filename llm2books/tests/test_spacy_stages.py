# llm2books/tests/test_spacy_stages.py

import unittest
from unittest.mock import MagicMock
import spacy

# Import all the SpaCy stage classes we need to test
from llm2books.stages.lemmatize_adv_spanish import LemmatizeAdvSpanish
from llm2books.stages.finalize_simpler_spanish import FinalizeSimplerSpanish
from llm2books.stages.lemmatize_simple_spanish import LemmatizeSimpleSpanish
from llm2books.stages.lemmatize_diglot_map import LemmatizeDiglotMap

class TestSpaCyLemmatizationStages(unittest.TestCase):

    @classmethod
    def setUpClass(cls):
        """
        Load the SpaCy models once for all tests in this class.
        This is a performance optimization as loading models is slow.
        """
        print("\nLoading SpaCy models for test class...")
        try:
            cls.spacy_es = spacy.load("es_core_news_lg", disable=["ner"])
            cls.spacy_models = {"es": cls.spacy_es}
            print("SpaCy models loaded successfully.")
        except IOError:
            raise unittest.SkipTest("Spanish SpaCy model not found. Run 'python -m spacy download es_core_news_lg'")

    def create_mock_stage(self, StageClass):
        """Helper to create a stage instance with mocked config and resources."""
        mock_config = MagicMock()
        common_resources = {
            "spacy_models": self.spacy_models,
            "content_project_dir": "/fake/dir" 
        }
        return StageClass("test_book", mock_config, common_resources)

    def test_stage2_lemmatize_adv_spanish(self):
        """Tests that Stage 2 correctly lemmatizes the main advanced Spanish text."""
        # ARRANGE
        stage = self.create_mock_stage(LemmatizeAdvSpanish)
        test_data = {
            "content_blocks": [{
                "block_type": "sentence",
                "adv_spanish_full": {
                    "text": "Alicia comenzaba a sentirse muy cansada.",
                    "lemmas": [] # This is what the stage needs to populate
                },
                "llm_call_status": {}
            }]
        }
        
        # ACT
        processed_data = stage._process_data(test_data)
        
        # ASSERT
        lemmas = processed_data["content_blocks"][0]["adv_spanish_full"]["lemmas"]
        self.assertEqual(lemmas, ["comenzar", "a", "sentir él", "mucho", "cansado"])
        status = processed_data["content_blocks"][0]["llm_call_status"]["stage2"]
        self.assertEqual(status, "COMPLETED_SPACY")

    def test_stage4_finalize_simpler_spanish(self):
        """
        Tests that Stage 4 correctly lemmatizes both advanced and simpler segments
        and aggregates the simpler lemmas.
        """
        # ARRANGE
        stage = self.create_mock_stage(FinalizeSimplerSpanish)
        test_data = {
            "content_blocks": [{
                "block_type": "sentence",
                "adv_spanish_segments": [{
                    "advanced_text": "Alicia comenzaba",
                    "simpler_text": "Alicia empezaba",
                    "advanced_lemmas": [], # To be populated
                    "simpler_lemmas": []   # To be populated
                }, {
                    "advanced_text": "a sentirse cansada.",
                    "simpler_text": "a estar cansada.",
                    "advanced_lemmas": [], # To be populated
                    "simpler_lemmas": []   # To be populated
                }],
                "simpler_adv_spanish_full": { # To be populated
                    "text": "",
                    "lemmas": []
                },
                "llm_call_status": {}
            }]
        }
        
        # ACT
        processed_data = stage._process_data(test_data)
        block = processed_data["content_blocks"][0]

        # ASSERT
        # Check segment lemmas
        self.assertEqual(block["adv_spanish_segments"][0]["advanced_lemmas"], ["comenzar"])
        self.assertEqual(block["adv_spanish_segments"][0]["simpler_lemmas"], ["empezar"])
        
        self.assertEqual(block["adv_spanish_segments"][1]["simpler_lemmas"], ["a", "estar", "cansado"])
        self.assertEqual(block["adv_spanish_segments"][1]["advanced_lemmas"], ["a", "sentir él", "cansado"])
        # Check aggregated simpler text and lemmas
        self.assertEqual(block["simpler_adv_spanish_full"]["text"], "Alicia empezaba a estar cansada.")
        self.assertEqual(block["simpler_adv_spanish_full"]["lemmas"], ["empezar", "a", "estar", "cansado"])
        self.assertEqual(block["llm_call_status"]["stage4"], "COMPLETED_SPACY")

    def test_stage6_lemmatize_simple_spanish(self):
        """Tests that Stage 6 correctly lemmatizes L3 simple Spanish."""
        # ARRANGE
        stage = self.create_mock_stage(LemmatizeSimpleSpanish)
        test_data = {
            "content_blocks": [{
                "block_type": "sentence",
                "phrase_alignments_l3_to_english": [
                    {"segment_id": "S1", "simple_spanish_text": "El libro era bueno."},
                    {"segment_id": "S2", "simple_spanish_text": "Ella leía rápido."},
                ],
                "simple_spanish_l3_full": {
                    "text": "El libro era bueno. Ella leía rápido.",
                    "lemmas": [] # To be populated
                },
                "simple_spanish_l3_lemmas_per_segment": {}, # To be populated
                "llm_call_status": {}
            }]
        }
        
        # ACT
        processed_data = stage._process_data(test_data)
        block = processed_data["content_blocks"][0]
        
        # ASSERT
        # Check per-segment lemmas
        self.assertEqual(block["simple_spanish_l3_lemmas_per_segment"]["S1"], ["el", "libro", "ser", "bueno"])
        self.assertEqual(block["simple_spanish_l3_lemmas_per_segment"]["S2"], ["él", "leer", "rápido"])
        
        # Check aggregated full-sentence lemmas
        self.assertEqual(block["simple_spanish_l3_full"]["lemmas"], ["el", "libro", "ser", "bueno", "él", "leer", "rápido"])
        self.assertEqual(block["llm_call_status"]["stage6"], "COMPLETED_SPACY")

    def test_stage8_lemmatize_diglot_map(self):
        """Tests that Stage 8 correctly lemmatizes the diglot map entries."""
        # ARRANGE
        stage = self.create_mock_stage(LemmatizeDiglotMap)
        test_data = {
            "content_blocks": [{
                "block_type": "sentence",
                "diglot_map_entries": [
                    # Viable case
                    {"english_word": "was", "exact_spanish_form": "estaba", "note": "viable", "spanish_lemma": ""},
                    # PROPER_NOUN case
                    {"english_word": "Alice", "exact_spanish_form": "PROPER_NOUN", "note": "PROPER_NOUN", "spanish_lemma": ""},
                    # NO_SUB case
                    {"english_word": "up", "exact_spanish_form": "NO_SUB", "note": "NO_SUB", "spanish_lemma": ""}
                ],
                "llm_call_status": {}
            }]
        }
        
        # ACT
        processed_data = stage._process_data(test_data)
        entries = processed_data["content_blocks"][0]["diglot_map_entries"]
        
        # ASSERT
        # Check viable lemma
        self.assertEqual(entries[0]["spanish_lemma"], "estar")
        # Check PROPER_NOUN fallback lemma
        self.assertEqual(entries[1]["spanish_lemma"], "alice")
        # Check NO_SUB fallback lemma
        self.assertEqual(entries[2]["spanish_lemma"], "up")
        self.assertEqual(processed_data["content_blocks"][0]["llm_call_status"]["stage8"], "COMPLETED_SPACY")

if __name__ == '__main__':
    unittest.main()