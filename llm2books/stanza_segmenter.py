import re
from typing import List, Tuple, Dict
import stanza
from . import helper
from abc import ABC, abstractmethod

#
class StanzaLanguageProcessor(ABC):
    def __init__(self, lang_code: str):
        print(f"Initializing Stanza pipeline for '{lang_code}'...")
        try:
            self.nlp = stanza.Pipeline(lang_code, processors='tokenize,pos,constituency', use_gpu=False)
            print(f"Stanza pipeline for '{lang_code}' loaded.")
        except Exception as e:
            print(f"Failed to load Stanza pipeline for {lang_code}. Ensure models are downloaded. Error: {e}")
            raise
        self._quote_chars = ""
        self.min_segment_size = 5
        self.min_refine_size = 3

    # --- HELPER METHODS (UNCHANGED) ---
    def _count_real_words(self, text: str) -> int:
        return len(re.findall(r'\w+', text))

    def _get_hierarchical_segments(self, tree) -> List[Tuple[str, int]]:
        return self._recursive_helper(tree)

    def _recursive_helper(self, tree) -> List[Tuple[str, int]]:
        if tree.is_leaf(): return [(tree.label, 1)]
        segments_from_children = []
        for child in tree.children:
            if child.is_leaf():
                is_word = tree.label.isalpha()
                segments_from_children.append((child.label, 1 if is_word else 0))
            else:
                segments_from_children.extend(self._recursive_helper(child))
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
            current_word_count = sum(c for _, c in buffer)
            if current_word_count >= self.min_segment_size:
                merged_text = " ".join(t for t, _ in buffer)
                total_count = sum(c for _, c in buffer)
                final_segments.append((merged_text.strip(), total_count))
                buffer = []
        if buffer:
            leftover_text = " ".join(t for t, _ in buffer)
            leftover_count = sum(c for _, c in buffer)
            if final_segments:
                prev_text, prev_count = final_segments.pop()
                final_segments.append((f"{prev_text} {leftover_text}".strip(), prev_count + leftover_count))
            else:
                final_segments.append((leftover_text.strip(), leftover_count))
        return final_segments

    def _refine_for_quotes(self, segments: List[str]) -> List[str]:
        if not self._quote_chars: return segments
        refined_segments = []
        for segment in segments:
            match = re.search(f"([{self._quote_chars}])", segment)
            if match and match.start() > 0:
                quote_char = match.group(1)
                pre_quote, quote_part = segment.split(quote_char, 1)
                pre_quote, quote_part = pre_quote.strip(), (quote_char + quote_part).strip()
                if self._count_real_words(pre_quote) < self.min_refine_size and refined_segments:
                    refined_segments[-1] = f"{refined_segments[-1]} {pre_quote}".strip()
                    if quote_part: refined_segments.append(quote_part)
                else:
                    if pre_quote: refined_segments.append(pre_quote)
                    if quote_part: refined_segments.append(quote_part)
            else:
                refined_segments.append(segment)
        return refined_segments

    # --- DEFINITIVE ORCHESTRATION METHOD ---
    def segment_sentence(self, sentence_text: str) -> List[str]:
        original_sentence = re.sub(r'\s+', ' ', sentence_text).strip()
        if not original_sentence: return []
        
        doc = self.nlp(original_sentence)
        if not doc.sentences: return [original_sentence]
        tree = doc.sentences[0].constituency
        if not tree or not tree.children: return [original_sentence]

        # Step 1: Get the "Golden Token Stream" - our source of truth for characters.
        golden_stream = helper.create_golden_token_stream(original_sentence, doc.sentences[0])
        
        # Step 2: Get the hierarchical word groupings from Stanza.
        initial_tuples = self._get_hierarchical_segments(tree.children[0])
        hierarchical_strings = [text for text, _ in initial_tuples]
        refined_hierarchical_strings = self._refine_for_quotes(hierarchical_strings)
        
        # Step 3: Map each WORD from the golden stream to a segment index.
        word_tokens_in_stream = [tok for tok in golden_stream if tok['t'] == 'w']
        word_to_seg_idx = {}
        current_word_idx = 0
        for seg_idx, seg_str in enumerate(refined_hierarchical_strings):
            # This regex is robust for counting words, even with contractions
            num_words_in_seg = len(re.findall(r'\b\w+\b', seg_str))
            for i in range(num_words_in_seg):
                if current_word_idx < len(word_tokens_in_stream):
                    word_tokens_in_stream[current_word_idx]['seg_idx'] = seg_idx
                    current_word_idx += 1
        
        # Step 4: Distribute ALL golden tokens into segment buckets.
        num_segments = len(refined_hierarchical_strings)
        segment_buckets: List[List[Dict]] = [[] for _ in range(num_segments)]
        
        current_seg_idx_for_b = 0
        for token in golden_stream:
            if token['t'] == 'w':
                seg_idx = token.get('seg_idx', current_seg_idx_for_b)
                if seg_idx < num_segments:
                    segment_buckets[seg_idx].append(token)
                    current_seg_idx_for_b = seg_idx
            else: # It's a 'b' token
                if current_seg_idx_for_b < num_segments:
                    segment_buckets[current_seg_idx_for_b].append(token)

        # Step 5: Apply Smart Space Boundary rule on the TOKEN BUCKETS.
        for i in range(num_segments - 1):
            if not segment_buckets[i] or not segment_buckets[i+1]: continue
            if segment_buckets[i][-1]['t'] == 'w': segment_buckets[i].append({'t':'b', 'v':''})
            if segment_buckets[i+1][0]['t'] == 'w': segment_buckets[i+1].insert(0, {'t':'b', 'v':''})
            b1 = segment_buckets[i][-1]
            b2 = segment_buckets[i+1][0]
            combined = b1['v'] + b2['v']
            split_point = combined.find(' ')
            if split_point != -1:
                b1['v'] = combined[:split_point + 1]
                b2['v'] = combined[split_point + 1:]
            else:
                b1['v'] = combined
                b2['v'] = ""

        # Step 6: Flatten the buckets into the final strings.
        final_strings = ["".join(tok['v'] for tok in bucket) for bucket in segment_buckets]
        
        return [s for s in final_strings if s]

class EnglishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self):
        super().__init__('en')
        self._quote_chars = "‘“"
        self.min_segment_size = 4 # English-specific setting
        self.min_refine_size = 3

class SpanishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self):
        super().__init__('es')
        self._quote_chars = "«" # Spanish-specific setting
        # Spanish might benefit from a different segment size, we can tune this later
        self.min_segment_size = 4 
        self.min_refine_size = 3