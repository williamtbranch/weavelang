import re
from typing import List, Tuple, Optional
import stanza
from abc import ABC
from enum import Enum, auto

def count_real_words(text: str) -> int:
    """Helper to count 'real' words, ignoring punctuation."""
    return len(re.findall(r'\b\w+\b', text))

class Cut(Enum):
    """Represents the decision for a potential cut point between words."""
    UNKNOWN = auto()
    VETO = auto()
    MINOR = auto()
    MAJOR = auto()
    PRIORITY = auto()

class PhraseNode:
    """A simple class to represent a node in our custom phrase tree for analysis."""
    def __init__(self, phrase_type: str, children: List['PhraseNode'] = None):
        self.phrase_type = phrase_type
        self.children = children if children else []
        self.start_idx = -1
        self.end_idx = -1

class StanzaLanguageProcessor(ABC):
    """
    Final segmenter using a robust, multi-pass "cut-point analysis" algorithm.
    It identifies potential cut points between words and applies a hierarchy of
    rules to decide where to make the final splits.
    """
    PRIORITY_PUNCTUATION = {';', ':'}
    NO_CUT_AFTER_XPOS = {'IN', 'DT', 'MD', 'TO', 'WDT', 'WP', 'WP$'}
    VERB_GROUP_XPOS = {'MD', 'VB', 'VBD', 'VBG', 'VBN', 'VBP', 'VBZ'}
    NO_CUT_BEFORE_XPOS = {'POS', 'RP'}
    MAJOR_CUT_BEFORE_PHRASE = {'SBAR', 'PP'}
    OPENING_PUNCT = {'“', '"', '‘', '(', '[', '{'}
    CLOSING_PUNCT = {'”', '"', '’', ')', ']', '}'}
    NO_CUT_BEFORE_PUNCT = {',', '.', ';', ':', '!', '?'}

    def _apply_sbar_cuts(self, node: PhraseNode, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        """Applies a PRIORITY cut after an SBAR that is followed by a comma."""
        if node.phrase_type == 'SBAR':
            # The SBAR ends at word index node.end_idx.
            # The word *after* the SBAR is at index node.end_idx + 1.
            comma_idx = node.end_idx + 1
            if comma_idx < len(words) and words[comma_idx].text == ',':
                # The cut happens *after* the comma. The comma's word index is comma_idx.
                # The cut marker index is the same as the word index.
                cut_location = comma_idx
                if cut_location < len(cut_markers):
                    cut_markers[cut_location] = Cut.PRIORITY
        
        # Recurse through children
        for child in node.children:
            self._apply_sbar_cuts(child, words, cut_markers)


    def _apply_short_quote_protection(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut], max_internal_words: int = 8):
        """Vetoes any non-priority cuts inside short quoted phrases."""
        i = 0
        while i < len(words):
            word = words[i]
            if word.text in self.OPENING_PUNCT:
                start_quote_idx = i
                for j in range(i + 1, len(words)):
                    if words[j].text in self.CLOSING_PUNCT:
                        end_quote_idx = j
                        word_count = sum(1 for k in range(start_quote_idx + 1, end_quote_idx) if words[k].upos != 'PUNCT')
                        if 0 < word_count <= max_internal_words:
                            for k in range(start_quote_idx, end_quote_idx):
                                if k < len(cut_markers) and cut_markers[k] != Cut.PRIORITY:
                                    cut_markers[k] = Cut.VETO
                        i = end_quote_idx
                        break
                # This else block prevents an infinite loop on an unclosed quote
                else: 
                    i += 1
            else:
                i += 1

    def __init__(self, lang_code: str):
        print(f"Initializing Stanza pipeline for '{lang_code}'...")
        try:
            self.nlp = stanza.Pipeline(lang_code, processors='tokenize,pos,constituency', use_gpu=False, logging_level='WARN')
            print(f"Stanza pipeline for '{lang_code}' loaded.")
        except Exception as e:
            print(f"Failed to load Stanza pipeline for {lang_code}. Error: {e}")
            raise
        self.min_segment_words: int = 5
    
    def _build_phrase_tree_with_indices(self, tree, current_idx: int) -> Tuple[Optional[PhraseNode], int]:
        """Builds a simplified tree from Stanza's output, annotating with word indices."""
        if tree.is_leaf():
            return None, current_idx + 1
        
        child_nodes = []
        start_idx = current_idx
        for child in tree.children:
            child_node, current_idx = self._build_phrase_tree_with_indices(child, current_idx)
            if child_node:
                child_nodes.append(child_node)

        node = PhraseNode(tree.label, child_nodes)
        node.start_idx = start_idx
        node.end_idx = current_idx - 1
        return node, current_idx

    def _apply_vetoes(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        self._apply_short_quote_protection(words, cut_markers)
        self.NO_CUT_AFTER_XPOS = {'IN', 'DT', 'MD', 'TO', 'WDT', 'WP', 'WP$', 'POS'}
        for i in range(len(cut_markers)):
            if cut_markers[i] == Cut.PRIORITY: continue

            if words[i].xpos in self.NO_CUT_AFTER_XPOS: cut_markers[i] = Cut.VETO
            if words[i+1].xpos in self.NO_CUT_BEFORE_XPOS: cut_markers[i] = Cut.VETO
            # --- FIX 4: Add the missing rule for punctuation ---
            if words[i+1].text in self.NO_CUT_BEFORE_PUNCT: cut_markers[i] = Cut.VETO
            if (words[i].xpos in self.VERB_GROUP_XPOS and words[i+1].xpos in self.VERB_GROUP_XPOS):
                cut_markers[i] = Cut.VETO
            if words[i].text == '-' or (i + 1 < len(words) and words[i+1].text == '-'):
                 cut_markers[i] = Cut.VETO
            if words[i].text in self.CLOSING_PUNCT or words[i+1].text in self.OPENING_PUNCT:
                cut_markers[i] = Cut.VETO
    
    def _apply_cuts_by_phrase(self, node: PhraseNode, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]):
        if node.phrase_type in self.MAJOR_CUT_BEFORE_PHRASE:
            # --- FIX 5: Use raw token count for PP checks, not 'real word' count ---
            word_count = node.end_idx - node.start_idx + 1
            if node.phrase_type == 'PP' and word_count <= 2: pass
            else:
                cut_idx = node.start_idx - 1
                if 0 <= cut_idx < len(cut_markers) and cut_markers[cut_idx] not in [Cut.VETO, Cut.PRIORITY]:
                    cut_markers[cut_idx] = Cut.MAJOR
        
        for child in node.children:
            self._apply_cuts_by_phrase(child, words, cut_markers)

    #
    def _apply_final_merging(self, words: List[stanza.models.common.doc.Word], cut_markers: List[Cut]) -> List[str]:
        if not words: return []

        # 1. Initial aggressive segmentation based on all non-vetoed cuts.
        segments = []
        current_chunk_text = [words[0].text]
        for i, cut in enumerate(cut_markers):
            if cut != Cut.VETO:
                segments.append(" ".join(current_chunk_text))
                current_chunk_text = [words[i+1].text]
            else:
                current_chunk_text.append(words[i+1].text)
        segments.append(" ".join(current_chunk_text))
        
        # 2. Create a parallel list of boundary strengths between segments.
        boundaries = []
        for cut in cut_markers:
            if cut == Cut.VETO: continue # Vetoed boundaries don't exist in our segmented list
            if cut == Cut.PRIORITY: boundaries.append(2)
            elif cut == Cut.MAJOR: boundaries.append(1)
            else: boundaries.append(0) # MINOR

        # 3. Iteratively merge until all segments meet the minimum word count.
        while True:
            word_counts = [count_real_words(s) for s in segments]
            
            # Find the index of the first segment that's too small.
            min_idx = -1
            for i, count in enumerate(word_counts):
                if count < self.min_segment_words:
                    min_idx = i
                    break
            
            # If no small segments are found, we're done.
            if min_idx == -1:
                break

            # If only one segment remains and it's still too small, we must accept it.
            if len(segments) == 1:
                break

            # Decide which neighbor to merge with based on boundary strength.
            merge_backward = False
            if min_idx == 0:
                # First segment is too small, must merge forward.
                merge_backward = False
            elif min_idx == len(segments) - 1:
                # Last segment is too small, must merge backward.
                merge_backward = True
            else:
                # A segment in the middle is too small. Merge across the weaker boundary.
                # boundary[min_idx - 1] is the boundary *before* the small segment.
                # boundary[min_idx] is the boundary *after* the small segment.
                if boundaries[min_idx - 1] <= boundaries[min_idx]:
                    merge_backward = True
                else:
                    merge_backward = False

            # Rebuild the lists after merging to avoid index errors.
            new_segments = []
            new_boundaries = []
            if merge_backward:
                for i in range(len(segments)):
                    if i == min_idx - 1:
                        # Combine this segment with the next one.
                        new_segments.append(f"{segments[i]} {segments[i+1]}")
                    elif i == min_idx:
                        # This segment was already merged, so skip it.
                        continue
                    else:
                        new_segments.append(segments[i])
                # Rebuild boundaries list. The boundary at min_idx - 1 was removed.
                for i in range(len(boundaries)):
                    if i != min_idx - 1:
                        new_boundaries.append(boundaries[i])
            else: # Merge forward
                for i in range(len(segments)):
                    if i == min_idx:
                        # Combine this segment with the next one.
                        new_segments.append(f"{segments[i]} {segments[i+1]}")
                    elif i == min_idx + 1:
                        # This segment was already merged, so skip it.
                        continue
                    else:
                        new_segments.append(segments[i])
                # Rebuild boundaries list. The boundary at min_idx was removed.
                for i in range(len(boundaries)):
                    if i != min_idx:
                        new_boundaries.append(boundaries[i])
            
            segments = new_segments
            boundaries = new_boundaries

        # 4. Final cleanup of whitespace and punctuation.
        final_list = []
        for seg in segments:
            clean_seg = re.sub(r'\s+', ' ', seg).strip()
            clean_seg = re.sub(r'\s+([,.:;?!])', r'\1', clean_seg)
            clean_seg = re.sub(r'([‘“])\s+', r'\1', clean_seg)
            if clean_seg: final_list.append(clean_seg)

        return final_list


    def segment_sentence(self, sentence_text: str) -> List[str]:
        cleaned_sentence = re.sub(r'\s+', ' ', sentence_text).strip()
        if not cleaned_sentence: return []
        
        doc = self.nlp(cleaned_sentence)
        if not doc.sentences: return [cleaned_sentence]
        
        sent = doc.sentences[0]
        words = sent.words
        num_cuts = len(words) - 1
        if num_cuts < 0: return [cleaned_sentence]

        root_node, _ = self._build_phrase_tree_with_indices(sent.constituency, 0)
        if not root_node: return [cleaned_sentence]
        
        cut_markers: List[Cut] = [Cut.UNKNOWN] * num_cuts

        for i, word in enumerate(words[:-1]):
            if word.text in self.PRIORITY_PUNCTUATION:
                cut_markers[i] = Cut.PRIORITY

        self._apply_sbar_cuts(root_node, words, cut_markers)
        self._apply_vetoes(words, cut_markers)
        self._apply_cuts_by_phrase(root_node, words, cut_markers)

        for i in range(num_cuts):
            if cut_markers[i] == Cut.UNKNOWN: cut_markers[i] = Cut.MINOR

        return self._apply_final_merging(words, cut_markers)


class EnglishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self):
        super().__init__('en')
        self.min_segment_words = 5

class SpanishStanzaProcessor(StanzaLanguageProcessor):
    def __init__(self):
        super().__init__('es')
        self.min_segment_words = 5