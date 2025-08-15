# llm2books/stanza_segmenter.py

from abc import ABC, abstractmethod
from typing import List, Tuple
import re
import stanza


class StanzaLanguageProcessor(ABC):
    """Abstract Base Class for a language-specific processor using Stanza."""
    def __init__(self, lang_code: str):
        print(f"Initializing Stanza pipeline for '{lang_code}'...")
        self.nlp = stanza.Pipeline(lang_code, processors='tokenize,pos,constituency', use_gpu=False)
        print(f"Stanza pipeline for '{lang_code}' loaded.")
        self._quote_chars = "" # To be defined by child classes

    # This is the main orchestration method, now moved to the base class
    def segment_sentence(self, sentence_text: str, min_segment_size: int = 4, min_refine_size: int = 3) -> List[str]:
        if not sentence_text.strip(): return []
        doc = self.nlp(sentence_text)
        if not doc.sentences: return []

        tree = doc.sentences[0].constituency
        if not tree or not tree.children: return [sentence_text]

        raw_segments = self._recursive_segment(tree.children[0])
        merged_tuples = self._merge_segments(raw_segments, min_segment_size)
        merged_strings = [text for text, _ in merged_tuples]
        return self._refine_for_quotes(merged_strings, min_refine_size)

    def _count_real_words(self, text: str) -> int:
        return len(re.findall(r'\w+', text))

    def _recursive_segment(self, tree) -> List[Tuple[str, int]]:
        if tree.is_leaf():
            is_word = any(c.isalpha() for c in tree.label)
            return [(tree.label, 1 if is_word else 0)]

        segments = []
        for child in tree.children:
            segments.extend(self._recursive_segment(child))
        
        stitched = []
        for text, count in segments:
            # This possessive rule is English-specific, might need adjustment for other languages
            if text in ("'s", "’s") and stitched:
                prev_text, prev_count = stitched.pop()
                stitched.append((prev_text + text, prev_count))
            else:
                stitched.append((text, count))
        return stitched

    def _merge_segments(self, segments: List[Tuple[str, int]], min_size: int) -> List[Tuple[str, int]]:
        final_segments = []
        buffer = []
        for text, count in segments:
            buffer.append((text, count))
            current_word_count = sum(c for _, c in buffer)
            if current_word_count >= min_size:
                merged_text = " ".join(t for t, _ in buffer)
                merged_text = re.sub(r'\s([,.!?;:])', r'\1', merged_text)
                final_segments.append((merged_text.strip(), current_word_count))
                buffer = []
        
        if buffer:
            leftover_text = " ".join(t for t, _ in buffer)
            leftover_text = re.sub(r'\s([,.!?;:])', r'\1', leftover_text)
            leftover_count = sum(c for _, c in buffer)
            if final_segments:
                prev_text, prev_count = final_segments.pop()
                final_segments.append((f"{prev_text} {leftover_text}".strip(), prev_count + leftover_count))
            else:
                final_segments.append((leftover_text.strip(), leftover_count))
        
        return final_segments

    def _refine_for_quotes(self, segments: List[str], min_size: int) -> List[str]:
        if not self._quote_chars: return segments # Skip if no quote chars defined
        refined = []
        for segment in segments:
            match = re.search(f"([{self._quote_chars}])", segment)
            if match and match.start() > 0:
                quote_char = match.group(1)
                pre_quote, quote_part = segment.split(quote_char, 1)
                pre_quote, quote_part = pre_quote.strip(), (quote_char + quote_part).strip()
                if self._count_real_words(pre_quote) < min_size and refined:
                    refined[-1] = f"{refined[-1]} {pre_quote}".strip()
                    if quote_part: refined.append(quote_part)
                else:
                    if pre_quote: refined.append(pre_quote)
                    if quote_part: refined.append(quote_part)
            else:
                refined.append(segment)
        return refined


class EnglishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self):
        super().__init__('en')
        self._quote_chars = "‘“"

# --- ADD THIS NEW CLASS ---
class SpanishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self):
        super().__init__('es')
        self._quote_chars = "«" # Spanish uses angle quotes