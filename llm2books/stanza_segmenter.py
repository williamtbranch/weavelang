# llm2books/stanza_segmenter.py

from abc import ABC, abstractmethod
from typing import List
import re
import stanza

# --- Abstract Base Class ---

class StanzaLanguageProcessor(ABC):
    """
    Abstract Base Class for a language-specific processor using Stanza.
    It encapsulates the entire segmentation and refinement logic.
    """
    def __init__(self, lang_code: str):
        # Each instance holds its own expensive pipeline object.
        self.nlp = stanza.Pipeline(lang_code, processors='tokenize,pos,constituency', use_gpu=True)

    @abstractmethod
    def segment_sentence(self, sentence_text: str, min_segment_size: int = 4, min_refine_size: int = 3) -> List[str]:
        """Orchestrates the full segmentation process for a sentence."""
        pass

# --- English Implementation ---

class EnglishStanzaProcessor(StanzaLanguageProcessor):
    """The concrete implementation for English."""
    def __init__(self):
        super().__init__('en')
        self._quote_chars = "‘“" # English-specific opening quotes

    def _count_real_words(self, text: str) -> int:
        return len(re.findall(r'\w+', text))

    def _segment_tree_recursive(self, tree, min_segment_size):
        if tree.is_leaf():
            return [(tree.label, 1)]

        segments_from_children = []
        for child in tree.children:
            is_word = child.label.isalpha() if child.is_leaf() else False
            if child.is_leaf():
                segments_from_children.append((child.label, 1 if is_word else 0))
            else:
                segments_from_children.extend(self._segment_tree_recursive(child, min_segment_size))

        stitched_segments = []
        for text, count in segments_from_children:
            if text in ("'s", "’s") and stitched_segments:
                prev_text, prev_count = stitched_segments.pop()
                stitched_segments.append((prev_text + text, prev_count))
            else:
                stitched_segments.append((text, count))
        
        final_segments = []
        buffer = []
        for text, count in stitched_segments:
            buffer.append((text, count))
            current_word_count = sum(c for t, c in buffer)
            if current_word_count >= min_segment_size:
                merged_text = " ".join(t for t, c in buffer).replace(" .", ".").replace(" ,", ",")
                final_segments.append((merged_text.strip(), current_word_count))
                buffer = []

        if buffer:
            leftover_text = " ".join(t for t, c in buffer).replace(" .", ".").replace(" ,", ",")
            leftover_count = sum(c for t, c in buffer)
            if final_segments:
                prev_text, prev_count = final_segments.pop()
                final_segments.append((f"{prev_text} {leftover_text}".strip(), prev_count + leftover_count))
            else:
                final_segments.append((leftover_text.strip(), leftover_count))
        
        return final_segments

    def _refine_segments(self, segments: List[str], min_refine_size: int) -> List[str]:
        refined_segments = []
        for segment in segments:
            match = re.search(f"[{self._quote_chars}]", segment)
            if match and match.start() > 0:
                quote_char = match.group(0)
                pre_quote_part, quote_part = segment.split(quote_char, 1)
                pre_quote_part = pre_quote_part.strip()
                quote_part = (quote_char + quote_part).strip()

                if self._count_real_words(pre_quote_part) < min_refine_size and refined_segments:
                    refined_segments[-1] = f"{refined_segments[-1]} {pre_quote_part}".strip()
                    if quote_part: refined_segments.append(quote_part)
                else:
                    if pre_quote_part: refined_segments.append(pre_quote_part)
                    if quote_part: refined_segments.append(quote_part)
            else:
                refined_segments.append(segment)
        return refined_segments

    def segment_sentence(self, sentence_text: str, min_segment_size: int = 4, min_refine_size: int = 3) -> List[str]:
        if not sentence_text.strip(): return []
        doc = self.nlp(sentence_text)
        if not doc.sentences: return []

        tree = doc.sentences[0].constituency
        if not tree or not tree.children: return [sentence_text]

        sentence_node = tree.children[0]
        initial_segments_tuples = self._segment_tree_recursive(sentence_node, min_segment_size)
        initial_segments = [text for text, count in initial_segments_tuples]
        
        return self._refine_segments(initial_segments, min_refine_size)

# --- Factory Function ---
# This will live in the orchestrator to manage the expensive model objects.
# We'll create a placeholder here for now.
def initialize_stanza_processors(lang_codes: List[str]) -> dict:
    processors = {}
    if 'en' in lang_codes:
        processors['en'] = EnglishStanzaProcessor()
    # Add 'es' when its processor is built
    return processors