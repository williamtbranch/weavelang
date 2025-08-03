# llm2books/tests/test_standardize.py

import pytest
from llm2books.standardize import split_boundary

class TestSplitBoundary:
    def test_standard_case_punctuation_then_space(self):
        # Boundary is ", "
        new_b1, new_b2 = split_boundary(",", " ")
        assert new_b1 == ", "
        assert new_b2 == ""

    def test_space_only_boundary(self):
        # Boundary is " ! "
        new_b1, new_b2 = split_boundary(" ", "! ")
        assert new_b1 == " "
        assert new_b2 == "! "

    def test_no_space_boundary(self):
        # Boundary is ',"'
        new_b1, new_b2 = split_boundary(",", "\"")
        assert new_b1 == ",\""
        assert new_b2 == ""

    def test_multiple_spaces(self):
        # Boundary is ".  "
        new_b1, new_b2 = split_boundary(".", "  ")
        assert new_b1 == ". "
        assert new_b2 == " "