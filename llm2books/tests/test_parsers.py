# llm2books/tests/test_parsers.py

import unittest
from unittest.mock import MagicMock
from llm2books.stages.generate_diglot_map import GenerateDiglotMap

# We need to import the class we're testing
from llm2books.stages.base import LLMStage

# --- FIXED DUMMY CLASS ---
# Since LLMStage is an Abstract Base Class (ABC), we can't instantiate it directly.
# We create a simple, concrete subclass that implements all abstract methods.
class DummyLLMStage(LLMStage):
    def get_system_prompt(self) -> str:
        return "dummy_prompt" # Not needed for this test
    def prepare_atomic_unit(self, block, s_idx):
        pass # Not needed for this test
    def process_llm_response(self, block, llm_response):
        pass # Not needed for this test


class TestLLMResponseParsers(unittest.TestCase):
    
    def setUp(self):
        """Set up a dummy stage instance to call the private parser methods."""
        # --- FIXED CONSTRUCTOR CALL ---
        # The constructor now takes cli_args and common_resources.
        mock_cli_args = MagicMock()
        mock_common_resources = MagicMock()
        self.stage = DummyLLMStage(
            book_stem="test_book",
            cli_args=mock_cli_args,
            common_resources=mock_common_resources,
            stage_number=99,
            stage_name="TestStage",
            parser_type='line' # Default, can be ignored
        )

    # --- Tests for _parse_llm_response_line ---

    def test_line_parser_happy_path(self):
        """Tests a perfect, well-formatted response."""
        mock_response = """
id 1: This is the first sentence.
id 2: This is the second.
id 3: And a third.
        """
        expected_ids = ["id 1", "id 2", "id 3"]
        
        parsed_data, errors = self.stage._parse_llm_response_line(mock_response, expected_ids)
        
        self.assertEqual(len(errors), 0)
        self.assertEqual(parsed_data["id 1"], "This is the first sentence.")
        self.assertEqual(parsed_data["id 2"], "This is the second.")
        self.assertEqual(parsed_data["id 3"], "And a third.")

    def test_line_parser_whitespace_and_casing(self):
        """Tests tolerance to extra whitespace and case differences in 'id'."""
        mock_response = """
ID 10 :   Some content with spaces.  
  id 11: Another line.
        """
        expected_ids = ["id 10", "id 11"]
        
        parsed_data, errors = self.stage._parse_llm_response_line(mock_response, expected_ids)
        
        self.assertEqual(len(errors), 0)
        # Note: The keys are lowercased by the parser.
        self.assertEqual(parsed_data["id 10"], "Some content with spaces.")
        self.assertEqual(parsed_data["id 11"], "Another line.")
        
    def test_line_parser_missing_ids(self):
        """Tests response where the LLM missed an ID."""
        mock_response = "id 1: Only the first sentence."
        expected_ids = ["id 1", "id 2"]
        
        parsed_data, errors = self.stage._parse_llm_response_line(mock_response, expected_ids)
        
        self.assertEqual(len(errors), 1)
        self.assertIn("Missing or empty content for IDs: id 2", errors[0])
        self.assertIn("id 1", parsed_data)
        self.assertNotIn("id 2", parsed_data)

    def test_line_parser_empty_content(self):
        """Tests response where an ID has no content, which is a validation failure."""
        mock_response = "id 5:\nid 6: Has content."
        expected_ids = ["id 5", "id 6"]
        
        parsed_data, errors = self.stage._parse_llm_response_line(mock_response, expected_ids)
        
        # The new parser considers an empty line a failure
        self.assertEqual(len(errors), 1)
        self.assertIn("Missing or empty content for IDs: id 5", errors[0])
        # It still parses what it can
        self.assertEqual(parsed_data.get("id 6"), "Has content.")


    def test_line_parser_malformed_lines(self):
        """Tests that it ignores lines that don't match the format."""
        mock_response = """
This is some commentary from the LLM.
id 20: This is a valid line.
Oops, I forgot the format.
id 21: Another valid line.
        """
        expected_ids = ["id 20", "id 21"]
        
        parsed_data, errors = self.stage._parse_llm_response_line(mock_response, expected_ids)

        self.assertEqual(len(errors), 0)
        self.assertEqual(len(parsed_data), 2)
        self.assertEqual(parsed_data["id 20"], "This is a valid line.")
        self.assertEqual(parsed_data["id 21"], "Another valid line.")

    def test_line_parser_empty_response(self):
        """Tests an empty string response from the LLM."""
        mock_response = ""
        expected_ids = ["id 1", "id 2"]
        
        parsed_data, errors = self.stage._parse_llm_response_line(mock_response, expected_ids)
        
        self.assertEqual(len(parsed_data), 0)
        self.assertEqual(len(errors), 1)
        self.assertIn("id 1", errors[0])
        self.assertIn("id 2", errors[0])

    # --- Tests for _parse_llm_response_block ---
    
    def test_block_parser_happy_path(self):
        """Tests a perfect block-formatted response."""
        mock_response = """
id 5_A1:
This is the first segment.
It has multiple lines.

id 5_A2: This segment is on one line.
id 5_A3:

And this segment has a leading newline.
"""
        expected_ids = ["id 5_A1", "id 5_A2", "id 5_A3"]
        self.stage.parser_type = "block"
        parsed_data, errors = self.stage._parse_llm_response_block(mock_response, expected_ids)
        
        self.assertEqual(len(errors), 0)
        self.assertEqual(parsed_data["id 5_a1"], "This is the first segment.\nIt has multiple lines.")
        self.assertEqual(parsed_data["id 5_a2"], "This segment is on one line.")
        self.assertEqual(parsed_data["id 5_a3"], "And this segment has a leading newline.")

    def test_block_parser_missing_colon(self):
        """Tests that the colon after the ID is optional."""
        mock_response = "id 100_S1\nContent without a colon."
        expected_ids = ["id 100_S1"]
        self.stage.parser_type = "block"
        parsed_data, errors = self.stage._parse_llm_response_block(mock_response, expected_ids)
        
        self.assertEqual(len(errors), 0)
        self.assertEqual(parsed_data["id 100_s1"], "Content without a colon.")

    def test_block_parser_empty_block(self):
        """Tests an ID followed immediately by another ID, which is a validation failure."""
        mock_response = "id 1\nSome content.\nid 2\nid 3\nMore content."
        expected_ids = ["id 1", "id 2", "id 3"]
        self.stage.parser_type = "block"
        parsed_data, errors = self.stage._parse_llm_response_block(mock_response, expected_ids)
        
        self.assertEqual(len(errors), 1)
        self.assertIn("Missing or empty content for IDs: id 2", errors[0])
        self.assertEqual(parsed_data.get("id 1"), "Some content.")
        self.assertEqual(parsed_data.get("id 3"), "More content.")

    def test_block_parser_final_block_capture(self):
        """Ensures the very last block in the text is captured."""
        mock_response = "id 50: This is the only block and it's at the end."
        expected_ids = ["id 50"]
        self.stage.parser_type = "block"
        parsed_data, errors = self.stage._parse_llm_response_block(mock_response, expected_ids)

        self.assertEqual(len(errors), 0)
        self.assertEqual(parsed_data["id 50"], "This is the only block and it's at the end.")

    def test_block_parser_missing_ids(self):
        """Tests a block response that is missing expected IDs."""
        mock_response = "id 1: Block one."
        expected_ids = ["id 1", "id 2"]
        self.stage.parser_type = "block"
        parsed_data, errors = self.stage._parse_llm_response_block(mock_response, expected_ids)
        
        self.assertEqual(len(errors), 1)
        self.assertIn("Missing or empty content for IDs: id 2", errors[0])
        self.assertIn("id 1", parsed_data)

class TestGenerateDiglotMapProcessing(unittest.TestCase):

    def setUp(self):
        """Set up a dummy stage instance for testing Stage 7 processing."""
        # --- FIXED CONSTRUCTOR CALL ---
        mock_cli_args = MagicMock()
        mock_common_resources = MagicMock()
        self.stage = GenerateDiglotMap(
            book_stem="test_book",
            cli_args=mock_cli_args,
            common_resources=mock_common_resources,
        )

    def test_diglot_map_process_happy_path(self):
        """Tests the successful processing of a well-formatted diglot map block."""
        
        # ARRANGE
        block = {
            "original_sentence_s_id": "S5",
            "phrase_alignments_l3_to_english": [
                {"segment_id": "S1"},
                {"segment_id": "S2"}
            ]
        }
        
        # This simulates the parsed output for the whole sentence, where each key
        # corresponds to a segment's LLM ID.
        llm_response = {
            'id 5_s1': "Alice -> PROPER_NOUN\nwas -> estaba",
            'id 5_s2': "by -> junto"
        }
        
        # ACT
        self.stage.process_llm_response(block, llm_response)
        
        # ASSERT
        entries = block["diglot_map_entries"]
        self.assertEqual(len(entries), 3)
        
        self.assertEqual(entries[0]["english_word"], "Alice")
        self.assertEqual(entries[0]["exact_spanish_form"], "PROPER_NOUN")
        self.assertEqual(entries[0]["is_viable_for_substitution"], False)
        
        self.assertEqual(entries[1]["english_word"], "was")
        self.assertEqual(entries[1]["exact_spanish_form"], "estaba")
        self.assertEqual(entries[1]["is_viable_for_substitution"], True)
        
        self.assertEqual(entries[2]["english_word"], "by")
        self.assertEqual(entries[2]["exact_spanish_form"], "junto")
        self.assertEqual(entries[2]["is_viable_for_substitution"], True)

    def test_diglot_map_process_malformed_and_extra_lines(self):
        """Tests that malformed lines are ignored and only valid mappings are processed."""
        
        # ARRANGE
        block = {
            "original_sentence_s_id": "S10",
            "phrase_alignments_l3_to_english": [
                {"segment_id": "S1"}
            ]
        }
        llm_response = {
            'id 10_s1': """
This is LLM commentary.
word1 -> forma1
another bad line
word2 -> forma2
            """
        }
        
        # ACT
        self.stage.process_llm_response(block, llm_response)
        
        # ASSERT
        entries = block["diglot_map_entries"]
        self.assertEqual(len(entries), 2)
        self.assertEqual(entries[0]["english_word"], "word1")
        self.assertEqual(entries[1]["english_word"], "word2")

    def test_diglot_map_process_empty_response(self):
        """Tests that an empty LLM response results in an empty list of entries."""
        
        # ARRANGE
        block = {
            "original_sentence_s_id": "S15",
            "phrase_alignments_l3_to_english": [
                {"segment_id": "S1"}
            ]
        }
        llm_response = { 'id 15_s1': "" }
        
        # ACT
        self.stage.process_llm_response(block, llm_response)
        
        # ASSERT
        self.assertIn("diglot_map_entries", block)
        self.assertEqual(len(block["diglot_map_entries"]), 0)

if __name__ == '__main__':
    unittest.main()